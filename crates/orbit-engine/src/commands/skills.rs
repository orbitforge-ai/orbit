use crate::app_context::AppContext;
use crate::executor::skills::{self, SkillInfo};
use crate::executor::workspace;

#[tauri::command]
pub async fn list_skills(
    agent_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<SkillInfo>, String> {
    list_skills_inner(agent_id, &app).await
}

async fn list_skills_inner(agent_id: String, app: &AppContext) -> Result<Vec<SkillInfo>, String> {
    let db = app.db.clone();
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
    app: tauri::State<'_, AppContext>,
) -> Result<(), String> {
    delete_skill_inner(agent_id, skill_name, &app).await
}

async fn delete_skill_inner(
    agent_id: String,
    skill_name: String,
    app: &AppContext,
) -> Result<(), String> {
    let db = app.db.clone();
    tokio::task::spawn_blocking(move || {
        skills::delete_skill(&agent_id, &skill_name)?;
        skills::clear_skill_state_for_agent_sessions(&db, &agent_id, &skill_name)
    })
    .await
    .map_err(|e| e.to_string())?
}

mod http {
    use super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct AgentIdArgs {
        agent_id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GetContentArgs {
        agent_id: String,
        skill_name: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateArgs {
        agent_id: String,
        name: String,
        description: String,
        body: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DeleteArgs {
        agent_id: String,
        skill_name: String,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_skills", |ctx, args| async move {
            let a: AgentIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = list_skills_inner(a.agent_id, &ctx).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("get_skill_content", |_ctx, args| async move {
            let a: GetContentArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = get_skill_content(a.agent_id, a.skill_name).await?;
            Ok(serde_json::Value::String(r))
        });
        reg.register("create_skill", |_ctx, args| async move {
            let a: CreateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            create_skill(a.agent_id, a.name, a.description, a.body).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("delete_skill", |ctx, args| async move {
            let a: DeleteArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            delete_skill_inner(a.agent_id, a.skill_name, &ctx).await?;
            Ok(serde_json::Value::Null)
        });
    }
}

pub use http::register as register_http;
