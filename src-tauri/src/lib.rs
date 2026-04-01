mod commands;
mod db;
mod error;
mod events;
mod executor;
mod memory_service;
mod models;
mod scheduler;

use std::path::PathBuf;
use tauri::Manager;
use tauri::menu::{ Menu, MenuItem };
use tauri::tray::TrayIconBuilder;

use commands::users::ActiveUser;
use db::connection::init as init_db;
use executor::engine::{ AgentSemaphores, ExecutorEngine, ExecutorTx, SessionExecutionRegistry };
use executor::permissions::PermissionRegistry;
use scheduler::SchedulerEngine;
use tauri_plugin_log::{ Builder, Target, TargetKind };
use tracing::info;

fn data_dir() -> PathBuf {
  let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
  PathBuf::from(home).join(".orbit")
}

fn log_dir() -> PathBuf {
  data_dir().join("logs")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder
    ::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_opener::init())
    .plugin(
      Builder::new()
        .targets([
          Target::new(TargetKind::Stdout),
          Target::new(TargetKind::LogDir { file_name: None }),
        ])
        .level(tauri_plugin_log::log::LevelFilter::Info)
        .level_for("reqwest", tauri_plugin_log::log::LevelFilter::Debug)
        .level_for("tungstenite", tauri_plugin_log::log::LevelFilter::Warn)
        .level_for("hyper", tauri_plugin_log::log::LevelFilter::Warn)
        .build()
    )
    .setup(|app| {
      let db_pool = init_db(data_dir())?;
      let log_dir = log_dir();
      std::fs::create_dir_all(&log_dir)?;

      // Create global skills directory
      let skills_dir = data_dir().join("skills");
      std::fs::create_dir_all(&skills_dir)?;

      // Create executor channel
      let (executor_tx, executor_rx) =
        tokio::sync::mpsc::unbounded_channel::<executor::engine::RunRequest>();
      let executor_tx_state = ExecutorTx(executor_tx.clone());
      let agent_semaphores = AgentSemaphores::new();
      let session_registry = SessionExecutionRegistry::new();
      let permission_registry = PermissionRegistry::new();

      // Start memory service sidecar (blocking but with timeout — app works without it)
      let memory_state: Option<memory_service::MemoryServiceState> = {
        let memory_data_dir = data_dir();
        match tauri::async_runtime::block_on(async {
          tokio::time::timeout(
            tokio::time::Duration::from_secs(90),
            memory_service::MemoryServiceState::start(memory_data_dir),
          ).await
        }) {
          Ok(Ok(state)) => {
            info!("Memory service started successfully");
            let health_state = state.clone();
            tauri::async_runtime::spawn(async move { health_state.health_loop().await });
            Some(state)
          }
          Ok(Err(e)) => {
            tracing::warn!("Memory service failed to start (agents will work without memory): {}", e);
            None
          }
          Err(_) => {
            tracing::warn!("Memory service startup timed out (agents will work without memory)");
            None
          }
        }
      };

      let memory_client = memory_state.as_ref().map(|s| s.client.clone());

      // Register managed state
      app.manage(db_pool.clone());
      app.manage(executor_tx_state);
      app.manage(agent_semaphores.clone());
      app.manage(session_registry.clone());
      app.manage(permission_registry.clone());
      app.manage(memory_state);
      app.manage(ActiveUser::new("default_user".to_string()));

      // Start execution engine (now takes tx clone for retry scheduling)
      let engine = ExecutorEngine::new(
        db_pool.clone(),
        executor_rx,
        executor_tx.clone(),
        app.handle().clone(),
        agent_semaphores,
        session_registry.clone(),
        permission_registry.clone(),
        log_dir.clone(),
        memory_client,
      );
      tauri::async_runtime::spawn(async move { engine.run().await });

      // Start scheduler engine
      let scheduler = SchedulerEngine::new(
        db_pool,
        ExecutorTx(executor_tx),
        app.handle().clone(),
        log_dir
      );
      tauri::async_runtime::spawn(async move { scheduler.run().await });

      // Menu bar tray icon
      let open_item = MenuItem::with_id(app, "open", "Open Orbit", true, None::<&str>)?;
      let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
      let tray_menu = Menu::with_items(app, &[&open_item, &quit_item])?;

      TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&tray_menu)
        .on_menu_event(|app, event| {
          match event.id.as_ref() {
            "open" => {
              if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
              }
            }
            "quit" => app.exit(0),
            _ => {}
          }
        })
        .build(app)?;

      info!("Orbit initialised");
      Ok(())
    })
    .on_window_event(|window, event| {
      if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        let _ = window.hide();
        api.prevent_close();
      }
    })
    .invoke_handler(
      tauri::generate_handler![
        // Tasks
        commands::tasks::list_tasks,
        commands::tasks::get_task,
        commands::tasks::create_task,
        commands::tasks::update_task,
        commands::tasks::delete_task,
        commands::tasks::trigger_task,
        // Schedules
        commands::schedules::list_schedules,
        commands::schedules::get_schedules_for_task,
        commands::schedules::create_schedule,
        commands::schedules::toggle_schedule,
        commands::schedules::delete_schedule,
        commands::schedules::preview_next_runs,
        // Runs
        commands::runs::list_runs,
        commands::runs::get_run,
        commands::runs::get_active_runs,
        commands::runs::read_run_log,
        commands::runs::get_agent_conversation,
        commands::runs::list_sub_agent_runs,
        // Agents
        commands::agents::list_agents,
        commands::agents::create_agent,
        commands::agents::update_agent,
        commands::agents::delete_agent,
        commands::agents::cancel_run,
        // Pulse
        commands::pulse::get_pulse_config,
        commands::pulse::update_pulse,
        // Chat
        commands::chat::list_chat_sessions,
        commands::chat::create_chat_session,
        commands::chat::rename_chat_session,
        commands::chat::archive_chat_session,
        commands::chat::unarchive_chat_session,
        commands::chat::delete_chat_session,
        commands::chat::get_chat_messages,
        commands::chat::send_chat_message,
        commands::chat::get_session_execution,
        commands::chat::cancel_agent_session,
        commands::chat::get_context_usage,
        commands::chat::compact_chat_session,
        // Workspace
        commands::workspace::get_workspace_path,
        commands::workspace::init_agent_workspace,
        commands::workspace::list_workspace_files,
        commands::workspace::read_workspace_file,
        commands::workspace::write_workspace_file,
        commands::workspace::delete_workspace_file,
        commands::workspace::get_agent_config,
        commands::workspace::update_agent_config,
        // LLM
        commands::llm::set_api_key,
        commands::llm::has_api_key,
        commands::llm::delete_api_key,
        commands::llm::trigger_agent_loop,
        // Bus
        commands::bus::list_bus_messages,
        commands::bus::get_bus_thread,
        commands::bus::list_bus_subscriptions,
        commands::bus::create_bus_subscription,
        commands::bus::toggle_bus_subscription,
        commands::bus::delete_bus_subscription,
        // Skills
        commands::skills::list_skills,
        commands::skills::get_skill_content,
        commands::skills::create_skill,
        commands::skills::delete_skill,
        // Permissions
        commands::permissions::respond_to_permission,
        commands::permissions::save_permission_rule,
        commands::permissions::delete_permission_rule,
        // Users
        commands::users::list_users,
        commands::users::create_user,
        commands::users::get_active_user,
        commands::users::set_active_user
      ]
    )
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
