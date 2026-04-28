// Engine modules now live in the `orbit-engine` crate. Re-export them at
// `crate::*` so existing `use crate::executor::…` paths inside command
// handlers and the Tauri builder continue to resolve without a sweeping
// rename.
pub use orbit_engine::app_context;
pub use orbit_engine::auth;
pub use orbit_engine::commands;
pub use orbit_engine::db;
pub use orbit_engine::error;
pub use orbit_engine::events;
pub use orbit_engine::executor;
pub use orbit_engine::memory_service;
pub use orbit_engine::models;
pub use orbit_engine::plugins;
pub use orbit_engine::scheduler;
pub use orbit_engine::shim;
pub use orbit_engine::triggers;
pub use orbit_engine::workflows;
pub use orbit_engine::{data_dir, plugins_dir, RuntimeAppHandleState};

use std::path::PathBuf;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};

use auth::{load_auth_state, supabase_credentials, AuthMode, AuthState};
use commands::users::ActiveUser;
use db::cloud::{CloudClientState, SupabaseClient};
use db::connection::init as init_db;
use executor::bg_processes::BgProcessRegistry;
use executor::engine::{
    AgentSemaphores, ExecutorEngine, ExecutorTx, SessionExecutionRegistry, UserQuestionRegistry,
};
use executor::mcp_server as mcp_bridge;
use executor::permissions::PermissionRegistry;
use executor::pty_session::PtyRegistry;
use plugins::PluginManager;
use scheduler::SchedulerEngine;
use std::sync::Arc;
use tauri_plugin_log::{Builder, Target, TargetKind};
use tracing::info;
use triggers::bindings::ProductionBindings;
use triggers::dispatcher::Dispatcher;

fn log_dir() -> PathBuf {
    data_dir().join("logs")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
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
                .build(),
        );

    #[cfg(all(debug_assertions, feature = "debug-mcp-bridge"))]
    let builder = builder.plugin(tauri_plugin_mcp_bridge::init());

    builder
        .setup(|app| {
            // Run the global settings migration before anything else can read
            // the file. Idempotent: a no-op if ~/.orbit/settings.json is
            // already present and parseable.
            executor::migration::migrate_global_settings();

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
                            session.refresh_token.clone(),
                            session.user_id.clone(),
                            session.email.clone(),
                        )))
                    } else {
                        None
                    }
                } else {
                    None
                };

            let cloud_state = CloudClientState::empty();
            cloud_state.set(cloud_client_opt.clone());

            // Background startup sync (pull only — no need to push on restart).
            // Emits `cloud:synced` when done so the frontend can invalidate its caches.
            // Skipped when DISABLE_CLOUD_SYNC=1.
            if !db::cloud::cloud_sync_disabled() {
                if let Some(client) = cloud_client_opt.clone() {
                    let pool = db_pool.0.clone();
                    let app_handle = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = client.pull_all_data(&pool).await {
                            tracing::warn!("Startup cloud pull failed: {}", e);
                        } else {
                            let _ = app_handle.emit("cloud:synced", ());
                        }
                    });
                }
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
            let user_question_registry = UserQuestionRegistry::new();
            let bg_process_registry = BgProcessRegistry::new();
            let pty_registry = PtyRegistry::new();

            // Initialise memory client from build-time API key (instant — no subprocess)
            let memory_state: Option<memory_service::MemoryServiceState> =
                memory_service::MemoryServiceState::try_create();
            if memory_state.is_some() {
                info!("Memory service initialised (mem0 cloud)");
            } else {
                info!("MEM0_API_KEY not set at build time — memory features disabled");
            }

            let memory_client = memory_state.as_ref().map(|s| s.client.clone());

            // Start the embedded MCP bridge on a random loopback port. The
            // handle is used by CLI-backed providers (claude-cli, codex-cli)
            // to expose Orbit's tool catalog to the CLI's inner agent loop
            // while keeping Orbit as the authority for tool dispatch and
            // permissions.
            let mcp_handle = tauri::async_runtime::block_on(async { mcp_bridge::start().await })
                .map_err(|e| {
                    Box::<dyn std::error::Error>::from(format!("failed to start MCP bridge: {}", e))
                })?;

            // Register managed state
            app.manage(auth_state);
            app.manage(cloud_state);
            app.manage(db_pool.clone());
            app.manage(RuntimeAppHandleState(app.handle().clone()));
            app.manage(executor_tx_state);
            app.manage(agent_semaphores.clone());
            app.manage(session_registry.clone());
            app.manage(permission_registry.clone());
            app.manage(user_question_registry);
            app.manage(bg_process_registry);
            app.manage(memory_state);
            app.manage(ActiveUser::new("default_user".to_string()));
            app.manage(mcp_handle);
            app.manage(pty_registry);

            // Plugin subsystem — loads ~/.orbit/plugins/registry.json and
            // every installed plugin's manifest. Subprocesses are lazy.
            let plugin_manager = std::sync::Arc::new(PluginManager::init(db_pool.clone()));
            plugin_manager.attach_log_emitter(app.handle());

            // Reply-target registry used by the `message` tool to route a
            // trigger-spawned agent's reply back to the originating channel.
            app.manage(triggers::reply_registry::ReplyRegistry::new());

            // Trigger dispatcher — plugins emit inbound events via
            // `trigger.emit` on their per-plugin JSON-RPC socket. The
            // dispatcher must be installed on the core-api server *before*
            // the core-api sockets start accepting connections.
            let dispatch_bindings = ProductionBindings::new(db_pool.clone(), app.handle().clone());
            let dispatcher = Arc::new(Dispatcher::new(dispatch_bindings));
            plugin_manager.set_core_api_dispatcher(dispatcher);

            plugin_manager.start_core_api_servers(db_pool.clone());
            plugins::oauth::spawn_loopback_listener(app.handle().clone(), plugin_manager.clone());
            app.manage(plugin_manager.clone());

            // Transport-agnostic bundle of shared state. The HTTP/WS shim
            // (Phase 1+) and, eventually, a standalone cloud server use this
            // instead of `tauri::State<T>` extractors. Fields are cloned views
            // of the same managed state registered above.
            let app_ctx = app_context::AppContext::new(
                db_pool.clone(),
                app.state::<AuthState>().inner().clone(),
                app.state::<CloudClientState>().inner().clone(),
                app.state::<ActiveUser>().inner().clone(),
                app.state::<ExecutorTx>().inner().clone(),
                app.state::<AgentSemaphores>().inner().clone(),
                app.state::<SessionExecutionRegistry>().inner().clone(),
                app.state::<PermissionRegistry>().inner().clone(),
                app.state::<UserQuestionRegistry>().inner().clone(),
                app.state::<BgProcessRegistry>().inner().clone(),
                app.state::<executor::mcp_server::McpServerHandle>().inner().clone(),
                plugin_manager,
                app.state::<Option<memory_service::MemoryServiceState>>().inner().clone(),
                Some(app.handle().clone()),
            );
            let app_ctx_arc = std::sync::Arc::new(app_ctx.clone());
            app.manage(app_ctx);

            // Spawn the HTTP+WS shim. Lets a browser tab connect to this
            // running Tauri process for dev, and is the same architecture
            // the future cloud server will run. Binds loopback-only with a
            // per-process bearer token at ~/.orbit/dev_token. Failure to
            // bind is logged but does not block Tauri startup.
            {
                let ctx = app_ctx_arc.clone();
                let dev_token_path = data_dir().join("dev_token");
                tauri::async_runtime::spawn(async move {
                    match shim::auth::BindMode::loopback_with_file(dev_token_path) {
                        Ok(mode) => {
                            let registry = shim::registry::build();
                            match shim::start(ctx, registry, mode, 8765).await {
                                Ok(addr) => tracing::info!("shim bound on {}", addr),
                                Err(e) => tracing::warn!("shim failed to bind: {}", e),
                            }
                        }
                        Err(e) => tracing::warn!("shim token init failed: {}", e),
                    }
                });
            }

            // Push the desired subscription set to every trigger-capable
            // plugin. Runs in the background so startup is not blocked by
            // plugin subprocess spin-up.
            {
                let handle = app.handle().clone();
                let db = db_pool.clone();
                tauri::async_runtime::spawn(async move {
                    triggers::subscriptions::reconcile_all(&handle, &db).await;
                });
            }

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
                cloud_client_opt.clone(),
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

            // Menu bar tray icon
            let open_item = MenuItem::with_id(app, "open", "Open Orbit", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&open_item, &quit_item])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&tray_menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
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
        .invoke_handler(tauri::generate_handler![
            // Auth
            commands::auth::get_auth_state,
            commands::auth::set_offline_mode,
            commands::auth::login,
            commands::auth::register,
            commands::auth::logout,
            commands::auth::force_cloud_sync,
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
            commands::schedules::get_schedules_for_workflow,
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
            commands::chat::respond_to_user_question,
            commands::chat::get_session_execution,
            commands::chat::get_chat_session_meta,
            commands::chat::cancel_agent_session,
            commands::chat::get_context_usage,
            commands::chat::compact_chat_session,
            commands::chat::get_message_reactions,
            // Workspace
            commands::workspace::get_workspace_path,
            commands::workspace::init_agent_workspace,
            commands::workspace::list_workspace_files,
            commands::workspace::read_workspace_file,
            commands::workspace::write_workspace_file,
            commands::workspace::delete_workspace_file,
            commands::workspace::create_workspace_dir,
            commands::workspace::rename_workspace_entry,
            commands::workspace::get_agent_config,
            commands::workspace::update_agent_config,
            commands::workspace::update_system_prompt,
            commands::workspace::list_agent_role_ids,
            // LLM
            commands::llm::set_api_key,
            commands::llm::has_api_key,
            commands::llm::delete_api_key,
            commands::llm::get_provider_status,
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
            // Global settings
            commands::global_settings::get_global_settings,
            commands::global_settings::update_global_settings,
            // Projects
            commands::projects::list_projects,
            commands::projects::get_project,
            commands::projects::create_project,
            commands::projects::update_project,
            commands::projects::delete_project,
            commands::projects::list_project_agents,
            commands::projects::list_project_agents_with_meta,
            commands::projects::list_agent_projects,
            commands::projects::add_agent_to_project,
            commands::projects::remove_agent_from_project,
            commands::projects::get_project_workspace_path,
            commands::projects::list_project_workspace_files,
            commands::projects::read_project_workspace_file,
            commands::projects::write_project_workspace_file,
            commands::projects::delete_project_workspace_file,
            commands::projects::create_project_workspace_dir,
            commands::projects::rename_project_workspace_entry,
            commands::project_boards::list_project_boards,
            commands::project_boards::create_project_board,
            commands::project_boards::update_project_board,
            commands::project_boards::delete_project_board,
            commands::project_board_columns::list_project_board_columns,
            commands::project_board_columns::create_project_board_column,
            commands::project_board_columns::update_project_board_column,
            commands::project_board_columns::delete_project_board_column,
            commands::project_board_columns::reorder_project_board_columns,
            // Work items (project board)
            commands::work_items::list_work_items,
            commands::work_items::get_work_item,
            commands::work_items::create_work_item,
            commands::work_items::update_work_item,
            commands::work_items::delete_work_item,
            commands::work_items::claim_work_item,
            commands::work_items::move_work_item,
            commands::work_items::reorder_work_items,
            commands::work_items::block_work_item,
            commands::work_items::complete_work_item,
            commands::work_items::list_work_item_comments,
            commands::work_items::create_work_item_comment,
            commands::work_items::update_work_item_comment,
            commands::work_items::delete_work_item_comment,
            commands::work_item_events::list_work_item_events,
            // Project workflows
            commands::project_workflows::list_project_workflows,
            commands::project_workflows::get_project_workflow,
            commands::project_workflows::create_project_workflow,
            commands::project_workflows::update_project_workflow,
            commands::project_workflows::delete_project_workflow,
            commands::project_workflows::set_project_workflow_enabled,
            // Workflow runs
            commands::workflow_runs::start_workflow_run,
            commands::workflow_runs::list_workflow_runs,
            commands::workflow_runs::list_project_workflow_runs,
            commands::workflow_runs::get_workflow_run,
            commands::workflow_runs::cancel_workflow_run,
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
            commands::memory::get_memory_health,
            // Plugins
            commands::plugins::list_plugins,
            commands::plugins::get_plugin_manifest,
            commands::plugins::plugin_call_tool,
            commands::plugins::list_plugin_surface_actions,
            commands::plugins::run_plugin_surface_action,
            commands::plugins::stage_plugin_install,
            commands::plugins::confirm_plugin_install,
            commands::plugins::cancel_plugin_install,
            commands::plugins::install_plugin_from_directory,
            commands::plugins::set_plugin_enabled,
            commands::plugins::reload_plugin,
            commands::plugins::reload_all_plugins,
            commands::plugins::uninstall_plugin,
            commands::plugins::set_plugin_oauth_config,
            commands::plugins::start_plugin_oauth,
            commands::plugins::disconnect_plugin_oauth,
            commands::plugins::get_plugin_runtime_log,
            commands::plugins::list_plugin_entities,
            commands::plugins::get_plugin_entity,
            commands::plugins::list_plugin_oauth_status,
            commands::plugins::set_plugin_secret,
            commands::plugins::delete_plugin_secret,
            commands::plugins::list_plugin_secret_status,
            // Triggers / listen bindings
            commands::triggers::list_agent_listen_bindings,
            commands::triggers::set_agent_listen_bindings,
            commands::triggers::plugin_list_channels,
            commands::triggers::list_trigger_capable_plugins,
            // Terminal (PTY)
            commands::terminals::open_terminal,
            commands::terminals::write_terminal,
            commands::terminals::resize_terminal,
            commands::terminals::close_terminal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
