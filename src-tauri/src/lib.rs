mod commands;
mod db;
mod error;
mod events;
mod executor;
mod models;
mod scheduler;

use std::path::PathBuf;
use tauri::Manager;

use db::connection::init as init_db;
use executor::engine::{ExecutorEngine, ExecutorTx};
use scheduler::SchedulerEngine;
use tauri_plugin_log::{Builder, Target, TargetKind};
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
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            Builder::new()
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir { file_name: None }),
                ])
                .build(),
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

            // Start execution engine
            let engine = ExecutorEngine::new(
                db_pool.clone(),
                executor_rx,
                app.handle().clone(),
                log_dir.clone(),
            );
            tauri::async_runtime::spawn(async move { engine.run().await });

            // Start scheduler engine
            let scheduler = SchedulerEngine::new(
                db_pool,
                ExecutorTx(executor_tx),
                app.handle().clone(),
                log_dir,
            );
            tauri::async_runtime::spawn(async move { scheduler.run().await });

            info!("Orbit initialised");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
            // Agents
            commands::agents::list_agents,
            commands::agents::create_agent,
            commands::agents::delete_agent
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
