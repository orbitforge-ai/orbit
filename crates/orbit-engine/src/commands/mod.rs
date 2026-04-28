pub mod agents;
pub mod auth;
pub mod bus;
pub mod chat;
pub mod global_settings;
pub mod llm;
pub mod memory;
pub mod permissions;
pub mod plugins;
pub mod project_board_columns;
pub mod project_boards;
pub mod project_workflows;
pub mod projects;
pub mod pulse;
pub mod runs;
pub mod schedules;
pub mod skills;
pub mod tasks;
// Embedded terminal (PTY) is a desktop-only feature: it exposes shell access
// to the user's local machine and depends on cli_launcher (also desktop-only).
#[cfg(feature = "desktop")]
pub mod terminals;
pub mod triggers;
pub mod users;
pub mod work_item_events;
pub mod work_items;
pub mod workflow_runs;
pub mod workspace;
