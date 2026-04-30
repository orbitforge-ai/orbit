use serde_json::json;
use tracing::{info, warn};

use crate::executor::llm_provider::ToolDefinition;
use crate::executor::skills;

use super::{context::ToolExecutionContext, ToolHandler};

pub struct ActivateSkillTool;

#[async_trait::async_trait]
impl ToolHandler for ActivateSkillTool {
    fn name(&self) -> &'static str {
        "activate_skill"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Activate a skill to load its full instructions into context. When a task matches one of the skills listed in <available-skills>, call this before proceeding. Pass the skill name exactly as shown.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "The name of the skill to activate (from <available-skills>)"
                    }
                },
                "required": ["skill_name"]
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        _app: &tauri::AppHandle,
        run_id: &str,
    ) -> Result<(String, bool), String> {
        let skill_name = input["skill_name"]
            .as_str()
            .ok_or("activate_skill: missing 'skill_name' field")?;

        info!(
            run_id = run_id,
            skill = skill_name,
            "agent tool: activate_skill"
        );

        let loaded_skill = skills::load_skill(&ctx.agent_id, skill_name, &ctx.disabled_skills)?;
        let instructions = loaded_skill.instructions;

        if let (Some(db), Some(session_id)) = (&ctx.db, ctx.current_session_id.as_deref()) {
            if let Err(err) = skills::upsert_active_skill(
                db,
                session_id,
                skill_name,
                &instructions,
                loaded_skill.metadata.source_path.as_deref(),
            ) {
                warn!(
                    session_id = session_id,
                    skill = skill_name,
                    error = %err,
                    "failed to persist activated skill state"
                );
            }
        }

        Ok((
            format!(
                "<skill-instructions name=\"{}\">\n{}\n</skill-instructions>",
                skill_name, instructions
            ),
            false,
        ))
    }
}
