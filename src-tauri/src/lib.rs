mod auth;
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

use auth::{load_auth_state, supabase_credentials, AuthMode, AuthState};
use commands::users::ActiveUser;
use db::cloud::{CloudClientState, SupabaseClient};
use db::connection::init as init_db;
use executor::engine::{ AgentSemaphores, ExecutorEngine, ExecutorTx, SessionExecutionRegistry };
use executor::permissions::PermissionRegistry;
use scheduler::SchedulerEngine;
use tauri_plugin_log::{ Builder, Target, TargetKind };
use tracing::info;
use std::sync::Arc;

pub fn data_dir() -> PathBuf {
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
      let initial_auth = load_auth_state(&data_dir());

      // If the persisted auth state is Cloud, restore the Supabase client and
      // trigger a background sync to pick up changes from other devices.
      let cloud_client_opt: Option<Arc<SupabaseClient>> =
        if let AuthMode::Cloud(ref session) = initial_auth {
          if let Ok((url, anon_key)) = supabase_credentials() {
            Some(Arc::new(SupabaseClient::new(
              url,
              anon_key,
              session.access_token.clone(),
              session.user_id.clone(),
            )))
          } else {
            None
          }
        } else {
          None
        };

      let cloud_state = CloudClientState::empty();
      cloud_state.set(cloud_client_opt.clone());

      // Background startup sync (pull only — no need to push on restart)
      if let Some(client) = cloud_client_opt.clone() {
        let pool = db_pool.0.clone();
        tauri::async_runtime::spawn(async move {
          if let Err(e) = client.pull_all_data(&pool).await {
            tracing::warn!("Startup cloud pull failed: {}", e);
          }
        });
      }

      let auth_state = AuthState::new(initial_auth);
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

      // Initialise memory client from build-time API key (instant — no subprocess)
      let memory_state: Option<memory_service::MemoryServiceState> =
        memory_service::MemoryServiceState::try_create();
      if memory_state.is_some() {
        info!("Memory service initialised (mem0 cloud)");
      } else {
        info!("MEM0_API_KEY not set at build time — memory features disabled");
      }

      let memory_client = memory_state.as_ref().map(|s| s.client.clone());

      // Register managed state
      app.manage(auth_state);
      app.manage(cloud_state);
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
        // Auth
        commands::auth::get_auth_state,
        commands::auth::set_offline_mode,
        commands::auth::login,
        commands::auth::register,
        commands::auth::logout,
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
        commands::users::set_active_user,
        // Memory
        commands::memory::search_memories,
        commands::memory::list_memories,
        commands::memory::add_memory,
        commands::memory::delete_memory,
        commands::memory::update_memory,
        commands::memory::get_memory_health
      ]
    )
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
