mod commands;
mod db;
mod error;
mod events;
mod executor;
mod models;
mod scheduler;

use std::path::PathBuf;
use tauri::Manager;
use tauri::menu::{ Menu, MenuItem };
use tauri::tray::TrayIconBuilder;

use db::connection::init as init_db;
use executor::engine::{ ExecutorEngine, ExecutorTx };
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
        .build()
    )
    .setup(|app| {
      let db_pool = init_db(data_dir())?;
      let log_dir = log_dir();
      std::fs::create_dir_all(&log_dir)?;

      // Create executor channel
      let (executor_tx, executor_rx) =
        tokio::sync::mpsc::unbounded_channel::<executor::engine::RunRequest>();
      let executor_tx_state = ExecutorTx(executor_tx.clone());

      // Register managed state
      app.manage(db_pool.clone());
      app.manage(executor_tx_state);

      // Start execution engine (now takes tx clone for retry scheduling)
      let engine = ExecutorEngine::new(
        db_pool.clone(),
        executor_rx,
        executor_tx.clone(),
        app.handle().clone(),
        log_dir.clone()
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
        commands::llm::trigger_agent_loop
      ]
    )
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
