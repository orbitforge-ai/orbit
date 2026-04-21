use crate::db::connection::DbPool;
use crate::executor::skills::{self, SkillInfo};
use crate::executor::workspace;

#[tauri::command]
pub async fn list_skills(
    agent_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<SkillInfo>, String> {
    let db = db.inner().clone();
    tokio::task::spawn_blocking(move || {
        let ws_config = workspace::load_agent_config(&agent_id).unwrap_or_default();
        skills::clear_disabled_skill_state_for_agent(&db, &agent_id, &ws_config.disabled_skills)?;
        let active_names =
            skills::load_active_skill_names_for_agent(&db, &agent_id, &ws_config.disabled_skills)?;
        let catalog = skills::discover_skills(&agent_id, &[]);

        Ok(catalog
            .skills
            .into_iter()
            .map(|s| SkillInfo {
                enabled: !ws_config.disabled_skills.contains(&s.name),
                active: active_names.contains(&s.name),
                source_path: s.source_path.map(|p| p.to_string_lossy().to_string()),
                name: s.name,
                description: s.description,
                source: s.source,
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_skill_content(agent_id: String, skill_name: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        skills::load_skill_instructions(&agent_id, &skill_name, &[])
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_skill(
    agent_id: String,
    name: String,
    description: String,
    body: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || skills::create_skill(&agent_id, &name, &description, &body))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_skill(
    agent_id: String,
    skill_name: String,
    db: tauri::State<'_, DbPool>,
) -> Result<(), String> {
    let db = db.inner().clone();
    tokio::task::spawn_blocking(move || {
        skills::delete_skill(&agent_id, &skill_name)?;
        skills::clear_skill_state_for_agent_sessions(&db, &agent_id, &skill_name)
    })
    .await
    .map_err(|e| e.to_string())?
}
