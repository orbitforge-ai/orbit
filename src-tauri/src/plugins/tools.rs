//! Tool handlers exposed to the agent:
//! - `EntityToolHandler` auto-generated per manifest `entityTypes[]`; routes
//!   `list | get | create | update | delete | link | unlink | list_relations`
//!   directly into `plugins::entities`.
//! - `PluginToolHandler` bridges a manifest-declared behavior tool to the
//!   plugin's MCP subprocess. V1 emits a clear error if called before the
//!   subprocess transport lands; the tool is still surfaced to the LLM so the
//!   agent sees the plugin's full contract.
//!
//! Agent-facing tool names are always `<plugin-id-slug>__<tool-name>`.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::executor::llm_provider::ToolDefinition;
use crate::executor::tools::context::ToolExecutionContext;
use crate::executor::tools::ToolHandler;

use super::entities;
use super::manifest::{EntityTypeSpec, PluginManifest, ToolSpec};

/// Compute the namespaced tool name for a plugin-declared behavior tool.
pub fn tool_name(plugin_id: &str, tool_name: &str) -> String {
    format!("{}__{}", super::manifest::slugify_id(plugin_id), tool_name)
}

/// Compute the namespaced tool name for an auto-generated entity CRUD tool.
pub fn entity_tool_name(plugin_id: &str, entity_type: &str) -> String {
    format!("{}__{}", super::manifest::slugify_id(plugin_id), entity_type)
}

pub struct PluginToolHandler {
    pub plugin_id: String,
    pub spec: ToolSpec,
    pub namespaced_name: String,
    pub manifest: PluginManifest,
}

#[async_trait]
impl ToolHandler for PluginToolHandler {
    fn name(&self) -> &'static str {
        // ToolHandler's `name()` returns &'static str; we leak the namespaced
        // name to satisfy that interface. The leak is bounded by the number
        // of plugin tools currently exposed — tiny.
        Box::leak(self.namespaced_name.clone().into_boxed_str())
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.namespaced_name.clone(),
            description: self
                .spec
                .description
                .clone()
                .unwrap_or_else(|| format!("Plugin tool from {}", self.plugin_id)),
            input_schema: self
                .spec
                .input_schema
                .clone()
                .unwrap_or_else(|| json!({ "type": "object" })),
        }
    }

    async fn execute(
        &self,
        _ctx: &ToolExecutionContext,
        input: &Value,
        app: &tauri::AppHandle,
        _run_id: &str,
    ) -> Result<(String, bool), String> {
        let manager = super::from_state(app);
        let extra_env = super::oauth::build_env_for_subprocess(&self.manifest);
        let response = manager
            .runtime
            .call_tool(&self.manifest, &self.spec.name, input, &extra_env)
            .await?;
        let text = serde_json::to_string(&response)
            .map_err(|e| format!("failed to serialise plugin tool result: {}", e))?;
        Ok((text, false))
    }
}

pub struct EntityToolHandler {
    pub plugin_id: String,
    pub spec: EntityTypeSpec,
    pub namespaced_name: String,
}

#[async_trait]
impl ToolHandler for EntityToolHandler {
    fn name(&self) -> &'static str {
        Box::leak(self.namespaced_name.clone().into_boxed_str())
    }

    fn definition(&self) -> ToolDefinition {
        let data_schema = self.spec.schema.clone();
        ToolDefinition {
            name: self.namespaced_name.clone(),
            description: format!(
                "CRUD for {} (plugin entity {}).",
                self.spec.display_name.as_deref().unwrap_or(&self.spec.name),
                self.plugin_id
            ),
            input_schema: json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "list", "get", "create", "update", "delete",
                            "link", "unlink", "list_relations"
                        ]
                    },
                    "id": { "type": "string" },
                    "projectId": { "type": "string" },
                    "data": data_schema,
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500 },
                    "offset": { "type": "integer", "minimum": 0 },
                    "relation": { "type": "string" },
                    "toKind": { "type": "string", "enum": ["plugin", "core"] },
                    "toType": { "type": "string" },
                    "toId": { "type": "string" }
                }
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &Value,
        _app: &tauri::AppHandle,
        _run_id: &str,
    ) -> Result<(String, bool), String> {
        let db = ctx
            .db
            .as_ref()
            .ok_or_else(|| "entity tool requires a DB-backed context".to_string())?;
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing `action` field".to_string())?;

        let plugin_id = self.plugin_id.as_str();
        let entity_type = self.spec.name.as_str();
        let agent_id = ctx.current_agent_id.as_deref();

        let result = match action {
            "list" => {
                let mut filter = entities::ListFilter::default();
                filter.project_id = input
                    .get("projectId")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                filter.limit = input.get("limit").and_then(Value::as_i64);
                filter.offset = input.get("offset").and_then(Value::as_i64);
                let rows = entities::list(db, plugin_id, entity_type, &filter)?;
                json!({ "items": rows })
            }
            "get" => {
                let id = input
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "`get` requires `id`".to_string())?;
                match entities::get(db, id)? {
                    Some(e) => json!(e),
                    None => return Err(format!("plugin_entity {:?} not found", id)),
                }
            }
            "create" => {
                let data = input
                    .get("data")
                    .cloned()
                    .ok_or_else(|| "`create` requires `data`".to_string())?;
                let project_id = input
                    .get("projectId")
                    .and_then(Value::as_str);
                let entity = entities::create(
                    db,
                    plugin_id,
                    entity_type,
                    project_id,
                    &data,
                    agent_id,
                )?;
                spawn_cloud_upsert_entity(ctx, &entity);
                json!(entity)
            }
            "update" => {
                let id = input
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "`update` requires `id`".to_string())?;
                let data = input
                    .get("data")
                    .cloned()
                    .ok_or_else(|| "`update` requires `data`".to_string())?;
                let entity = entities::update(db, id, &data)?;
                spawn_cloud_upsert_entity(ctx, &entity);
                json!(entity)
            }
            "delete" => {
                let id = input
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "`delete` requires `id`".to_string())?;
                entities::delete(db, id)?;
                spawn_cloud_delete_entity(ctx, id);
                json!({ "deleted": id })
            }
            "link" => {
                let from_id = input
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "`link` requires `id` (source entity id)".to_string())?;
                let relation = input
                    .get("relation")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "`link` requires `relation`".to_string())?;
                let to_kind = input
                    .get("toKind")
                    .and_then(Value::as_str)
                    .unwrap_or("plugin");
                let to_type = input
                    .get("toType")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "`link` requires `toType`".to_string())?;
                let to_id = input
                    .get("toId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "`link` requires `toId`".to_string())?;
                let rel = entities::link(
                    db,
                    "plugin",
                    entity_type,
                    from_id,
                    to_kind,
                    to_type,
                    to_id,
                    relation,
                )?;
                spawn_cloud_upsert_relation(ctx, &rel);
                json!(rel)
            }
            "unlink" => {
                let from_id = input
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "`unlink` requires `id`".to_string())?;
                let to_id = input
                    .get("toId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "`unlink` requires `toId`".to_string())?;
                let relation = input
                    .get("relation")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "`unlink` requires `relation`".to_string())?;
                entities::unlink(db, from_id, to_id, relation)?;
                // Cloud sync for relation delete is a soft no-op — we don't
                // have the relation id here. Relations re-sync on next pull.
                json!({ "unlinked": { "from": from_id, "to": to_id, "relation": relation } })
            }
            "list_relations" => {
                let id = input
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "`list_relations` requires `id`".to_string())?;
                let rows = entities::list_relations(db, id)?;
                json!({ "items": rows })
            }
            other => return Err(format!("unknown action {:?}", other)),
        };

        let text = serde_json::to_string(&result)
            .map_err(|e| format!("failed to serialise entity tool result: {}", e))?;
        Ok((text, false))
    }
}

/// Build the full tool handler list contributed by every enabled plugin. Used
/// by the agent executor to append to its static tool list.
pub fn build_handlers(manifests: &[PluginManifest]) -> Vec<Box<dyn ToolHandler>> {
    let mut out: Vec<Box<dyn ToolHandler>> = Vec::new();
    for manifest in manifests {
        for entity in &manifest.entity_types {
            let namespaced_name = entity_tool_name(&manifest.id, &entity.name);
            out.push(Box::new(EntityToolHandler {
                plugin_id: manifest.id.clone(),
                spec: entity.clone(),
                namespaced_name,
            }));
        }
        for tool in &manifest.tools {
            let namespaced_name = tool_name(&manifest.id, &tool.name);
            out.push(Box::new(PluginToolHandler {
                plugin_id: manifest.id.clone(),
                spec: tool.clone(),
                namespaced_name,
                manifest: manifest.clone(),
            }));
        }
    }
    out
}

/// Does `tool_name` look like a plugin-contributed tool (contains `__`)?
pub fn is_plugin_tool_name(tool_name: &str) -> bool {
    tool_name.contains("__")
}

fn spawn_cloud_upsert_entity(ctx: &ToolExecutionContext, entity: &entities::PluginEntity) {
    if let Some(client) = ctx.cloud_client.clone() {
        let entity = entity.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = client.upsert_plugin_entity(&entity).await {
                tracing::warn!("cloud upsert plugin_entity: {}", e);
            }
        });
    }
}

fn spawn_cloud_delete_entity(ctx: &ToolExecutionContext, id: &str) {
    if let Some(client) = ctx.cloud_client.clone() {
        let id = id.to_string();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = client.delete_plugin_entity(&id).await {
                tracing::warn!("cloud delete plugin_entity: {}", e);
            }
        });
    }
}

fn spawn_cloud_upsert_relation(
    ctx: &ToolExecutionContext,
    relation: &entities::PluginEntityRelation,
) {
    if let Some(client) = ctx.cloud_client.clone() {
        let relation = relation.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = client.upsert_plugin_entity_relation(&relation).await {
                tracing::warn!("cloud upsert plugin_entity_relation: {}", e);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_tool_name_matches_spec() {
        assert_eq!(entity_tool_name("com.orbit.social", "content"), "com_orbit_social__content");
    }

    #[test]
    fn plugin_tool_name_matches_spec() {
        assert_eq!(tool_name("com.orbit.github", "clone_repo"), "com_orbit_github__clone_repo");
    }

    #[test]
    fn namespaced_name_detection() {
        assert!(is_plugin_tool_name("com_orbit_github__clone_repo"));
        assert!(!is_plugin_tool_name("read_file"));
    }
}
