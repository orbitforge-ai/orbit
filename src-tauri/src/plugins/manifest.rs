//! Plugin manifest parsing and validation.
//!
//! The manifest (`plugin.json`) is the single source of truth for what a
//! plugin contributes: tools, entity types, OAuth providers, hooks, UI
//! surfaces, workflow triggers/nodes. Every other subsystem reads from this
//! struct, never from `plugin.json` directly.

use std::collections::HashSet;
use std::path::Path;

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::PLUGIN_HOST_API_VERSION;

pub const MAX_MANIFEST_BYTES: u64 = 256 * 1024; // 256 KiB is plenty for a manifest.

/// Parsed `plugin.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub schema_version: u32,
    pub host_api_version: String,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub runtime: RuntimeSpec,
    #[serde(default)]
    pub tools: Vec<ToolSpec>,
    #[serde(default)]
    pub entity_types: Vec<EntityTypeSpec>,
    #[serde(default)]
    pub oauth_providers: Vec<OAuthProviderSpec>,
    #[serde(default)]
    pub secrets: Vec<SecretSpec>,
    #[serde(default)]
    pub permissions: PermissionsSpec,
    #[serde(default)]
    pub hooks: HooksSpec,
    #[serde(default)]
    pub workflow: WorkflowSpec,
    #[serde(default)]
    pub ui: UiSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSpec {
    /// V1 supports `"mcp-stdio"`. Future `"mcp-remote"` documented as V2.
    #[serde(rename = "type")]
    pub kind: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// "safe" | "moderate" | "dangerous". Defaults to "moderate".
    #[serde(default = "default_risk_level")]
    pub risk_level: String,
    #[serde(default)]
    pub input_schema: Option<Value>,
}

fn default_risk_level() -> String {
    "moderate".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityTypeSpec {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub schema: Value,
    #[serde(default)]
    pub relations: Vec<RelationSpec>,
    #[serde(default)]
    pub list_fields: Vec<String>,
    #[serde(default)]
    pub title_field: Option<String>,
    #[serde(default)]
    pub indexed_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationSpec {
    pub name: String,
    /// The target entity type name (plugin-defined or core entity like
    /// `work_item`, `project`, `agent`).
    pub to: String,
    /// "one" | "many". Validated but currently advisory.
    #[serde(default = "default_cardinality")]
    pub cardinality: String,
}

fn default_cardinality() -> String {
    "one".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthProviderSpec {
    pub id: String,
    pub name: String,
    pub authorization_url: String,
    pub token_url: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    /// "public" (PKCE) | "confidential" (user-supplied client secret).
    #[serde(default = "default_client_type")]
    pub client_type: String,
    #[serde(default = "default_redirect_uri")]
    pub redirect_uri: String,
    /// Embedded `client_id` for public PKCE plugins that ship with an
    /// author-registered OAuth app. Not a secret. Confidential clients leave
    /// this empty and take the user's own `client_id` via Keychain.
    #[serde(default)]
    pub client_id: Option<String>,
}

fn default_client_type() -> String {
    "public".to_string()
}

fn default_redirect_uri() -> String {
    format!(
        "http://127.0.0.1:{}/oauth/callback",
        super::oauth::LOOPBACK_PORT
    )
}

/// User-supplied secret (e.g. a bot token pasted from a provider dashboard).
/// Stored in the OS keychain and injected into the subprocess env as
/// `env_var` at launch. Declarative so the UI can render the right input
/// fields without hardcoding per-plugin knowledge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretSpec {
    pub key: String,
    pub env_var: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionsSpec {
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub filesystem: Vec<String>,
    #[serde(default)]
    pub oauth: Vec<String>,
    /// Whitelist of core entity types this plugin's MCP subprocess may read
    /// through the `ORBIT_CORE_API_SOCKET` JSON-RPC channel.
    #[serde(default)]
    pub core_entities: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksSpec {
    #[serde(default)]
    pub subscribe: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSpec {
    #[serde(default)]
    pub triggers: Vec<WorkflowTriggerSpec>,
    #[serde(default)]
    pub nodes: Vec<WorkflowNodeSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTriggerSpec {
    pub kind: String,
    pub display_name: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub config_schema: Option<Value>,
    #[serde(default)]
    pub output_schema: Option<Value>,
    #[serde(default)]
    pub subscription_tool: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowNodeSpec {
    pub kind: String,
    pub display_name: String,
    #[serde(default)]
    pub icon: Option<String>,
    pub tool: String,
    #[serde(default)]
    pub field_options: Vec<WorkflowNodeFieldOptionSpec>,
    #[serde(default)]
    pub input_schema: Option<Value>,
    #[serde(default)]
    pub output_schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowNodeFieldOptionSpec {
    pub field: String,
    pub source_tool: String,
    pub format: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiSpec {
    #[serde(default)]
    pub sidebar_items: Vec<Value>,
    #[serde(default)]
    pub entity_detail_tabs: Vec<Value>,
    #[serde(default)]
    pub agent_chat_actions: Vec<Value>,
    #[serde(default)]
    pub slash_commands: Vec<Value>,
    #[serde(default)]
    pub settings_panels: Vec<Value>,
}

/// Load and validate a manifest from disk.
pub fn load_from_path(path: &Path) -> Result<PluginManifest, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("plugin.json not found at {}: {}", path.display(), e))?;
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(format!(
            "plugin.json too large ({} bytes; max {})",
            metadata.len(),
            MAX_MANIFEST_BYTES
        ));
    }
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("failed to read plugin.json: {}", e))?;
    parse_and_validate(&content)
}

/// Parse and validate a manifest from a string.
pub fn parse_and_validate(content: &str) -> Result<PluginManifest, String> {
    let manifest: PluginManifest =
        serde_json::from_str(content).map_err(|e| format!("invalid plugin.json: {}", e))?;
    validate(&manifest)?;
    Ok(manifest)
}

/// Structural and policy validation — id shape, host-API compat, runtime kind,
/// uniqueness constraints, tool/entity name sanity.
pub fn validate(manifest: &PluginManifest) -> Result<(), String> {
    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported schemaVersion {}; only 1 is supported",
            manifest.schema_version
        ));
    }

    check_host_api_compat(&manifest.host_api_version)?;

    check_reverse_dns_id(&manifest.id)?;

    if manifest.name.trim().is_empty() {
        return Err("manifest.name must not be empty".into());
    }

    Version::parse(&manifest.version).map_err(|_| {
        format!(
            "manifest.version {:?} is not valid semver",
            manifest.version
        )
    })?;

    match manifest.runtime.kind.as_str() {
        "mcp-stdio" => {}
        other => {
            return Err(format!(
                "unsupported runtime.type {:?} (V1 supports mcp-stdio)",
                other
            ))
        }
    }
    if manifest.runtime.command.trim().is_empty() {
        return Err("runtime.command must not be empty".into());
    }

    let mut tool_names: HashSet<&str> = HashSet::new();
    for tool in &manifest.tools {
        check_identifier(&tool.name, "tool.name")?;
        if tool.name.contains("__") {
            return Err(format!(
                "tool.name {:?} must not contain `__` (reserved as plugin namespace separator)",
                tool.name
            ));
        }
        if !tool_names.insert(tool.name.as_str()) {
            return Err(format!("duplicate tool name {:?}", tool.name));
        }
    }

    let mut entity_names: HashSet<&str> = HashSet::new();
    for entity in &manifest.entity_types {
        check_identifier(&entity.name, "entityTypes.name")?;
        if !entity_names.insert(entity.name.as_str()) {
            return Err(format!("duplicate entityTypes.name {:?}", entity.name));
        }
        if !entity.schema.is_object() {
            return Err(format!(
                "entityTypes[{}].schema must be an object (JSON Schema)",
                entity.name
            ));
        }
    }

    let mut oauth_ids: HashSet<&str> = HashSet::new();
    for provider in &manifest.oauth_providers {
        check_identifier(&provider.id, "oauthProviders.id")?;
        if !oauth_ids.insert(provider.id.as_str()) {
            return Err(format!("duplicate oauthProviders.id {:?}", provider.id));
        }
        match provider.client_type.as_str() {
            "public" | "confidential" => {}
            other => {
                return Err(format!(
                    "oauthProviders[{}].clientType {:?} is invalid (public|confidential)",
                    provider.id, other
                ))
            }
        }
    }

    let mut secret_keys: HashSet<&str> = HashSet::new();
    let mut secret_envs: HashSet<&str> = HashSet::new();
    for spec in &manifest.secrets {
        check_identifier(&spec.key, "secrets.key")?;
        if !secret_keys.insert(spec.key.as_str()) {
            return Err(format!("duplicate secrets.key {:?}", spec.key));
        }
        if spec.env_var.trim().is_empty() {
            return Err(format!("secrets[{}].envVar must not be empty", spec.key));
        }
        if !spec
            .env_var
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            || spec.env_var.starts_with(|c: char| c.is_ascii_digit())
        {
            return Err(format!(
                "secrets[{}].envVar {:?} must be SCREAMING_SNAKE_CASE",
                spec.key, spec.env_var
            ));
        }
        if !secret_envs.insert(spec.env_var.as_str()) {
            return Err(format!("duplicate secrets.envVar {:?}", spec.env_var));
        }
        if spec.display_name.trim().is_empty() {
            return Err(format!(
                "secrets[{}].displayName must not be empty",
                spec.key
            ));
        }
    }

    for trigger in &manifest.workflow.triggers {
        let expected = format!("trigger.{}.", slugify_id(&manifest.id));
        if !trigger.kind.starts_with(&expected) {
            return Err(format!(
                "workflow.triggers[].kind {:?} must start with {:?}",
                trigger.kind, expected
            ));
        }
    }
    for node in &manifest.workflow.nodes {
        let expected = format!("integration.{}.", slugify_id(&manifest.id));
        if !node.kind.starts_with(&expected) {
            return Err(format!(
                "workflow.nodes[].kind {:?} must start with {:?}",
                node.kind, expected
            ));
        }
        if node.tool.trim().is_empty() {
            return Err(format!(
                "workflow.nodes[{}].tool must not be empty",
                node.kind
            ));
        }
        for field_option in &node.field_options {
            if field_option.field.trim().is_empty() {
                return Err(format!(
                    "workflow.nodes[{}].fieldOptions[].field must not be empty",
                    node.kind
                ));
            }
            if field_option.source_tool.trim().is_empty() {
                return Err(format!(
                    "workflow.nodes[{}].fieldOptions[{}].sourceTool must not be empty",
                    node.kind, field_option.field
                ));
            }
            if field_option.format.trim().is_empty() {
                return Err(format!(
                    "workflow.nodes[{}].fieldOptions[{}].format must not be empty",
                    node.kind, field_option.field
                ));
            }
            if !manifest
                .tools
                .iter()
                .any(|tool| tool.name == field_option.source_tool)
            {
                return Err(format!(
                    "workflow.nodes[{}].fieldOptions[{}].sourceTool {:?} must match a declared tool",
                    node.kind, field_option.field, field_option.source_tool
                ));
            }
        }
    }

    Ok(())
}

fn check_host_api_compat(declared: &str) -> Result<(), String> {
    let req = VersionReq::parse(declared).map_err(|e| {
        format!(
            "hostApiVersion {:?} is not a valid semver range: {}",
            declared, e
        )
    })?;
    let current = Version::parse(PLUGIN_HOST_API_VERSION)
        .expect("PLUGIN_HOST_API_VERSION must be valid semver");
    if !req.matches(&current) {
        return Err(format!(
            "plugin requires host API {} but this build is {}",
            declared, PLUGIN_HOST_API_VERSION
        ));
    }
    Ok(())
}

fn check_reverse_dns_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("manifest.id must not be empty".into());
    }
    if id.contains("..") || id.contains('/') || id.contains('\\') {
        return Err(format!(
            "manifest.id {:?} contains forbidden characters",
            id
        ));
    }
    let parts: Vec<&str> = id.split('.').collect();
    if parts.len() < 2 {
        return Err(format!(
            "manifest.id {:?} must be reverse-DNS (e.g. com.example.plugin)",
            id
        ));
    }
    for part in parts {
        if part.is_empty()
            || !part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(format!(
                "manifest.id {:?} segment {:?} is invalid (alnum, `-`, `_` only)",
                id, part
            ));
        }
    }
    Ok(())
}

fn check_identifier(name: &str, field: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err(format!("{} must not be empty", field));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "{} {:?} must contain only alnum, `_`, `-`",
            field, name
        ));
    }
    Ok(())
}

/// Convert a reverse-DNS plugin id into its tool-name slug by replacing `.`
/// and `-` with `_`. Used to namespace tool names: `<slug>__<tool-name>`.
pub fn slugify_id(id: &str) -> String {
    id.chars()
        .map(|c| match c {
            '.' | '-' => '_',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PluginManifest {
        serde_json::from_str(
            r#"{
                "schemaVersion": 1,
                "hostApiVersion": "^1.0.0",
                "id": "com.orbit.hello",
                "name": "Hello",
                "version": "0.1.0",
                "runtime": { "type": "mcp-stdio", "command": "node", "args": ["server.js"] }
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn host_api_compat_accepts_compatible() {
        assert!(check_host_api_compat("^1.0.0").is_ok());
        assert!(check_host_api_compat("1.0.0").is_ok());
        assert!(check_host_api_compat(">=1.0.0, <2.0.0").is_ok());
    }

    #[test]
    fn host_api_compat_rejects_incompatible() {
        assert!(check_host_api_compat("^2.0.0").is_err());
        assert!(check_host_api_compat("0.9.0").is_err());
    }

    #[test]
    fn host_api_compat_rejects_malformed() {
        assert!(check_host_api_compat("not-a-range").is_err());
    }

    #[test]
    fn reverse_dns_accepts_valid_ids() {
        assert!(check_reverse_dns_id("com.orbit.github").is_ok());
        assert!(check_reverse_dns_id("com.example.my-plugin_v2").is_ok());
    }

    #[test]
    fn reverse_dns_rejects_invalid_ids() {
        assert!(check_reverse_dns_id("plugin").is_err());
        assert!(check_reverse_dns_id("../evil").is_err());
        assert!(check_reverse_dns_id("com/orbit/x").is_err());
        assert!(check_reverse_dns_id("com.orbit.x!").is_err());
    }

    #[test]
    fn slug_replaces_dots_and_dashes() {
        assert_eq!(slugify_id("com.orbit.github"), "com_orbit_github");
        assert_eq!(slugify_id("com.orbit.my-plugin"), "com_orbit_my_plugin");
    }

    #[test]
    fn validate_rejects_tool_with_double_underscore() {
        let mut m = fixture();
        m.tools.push(ToolSpec {
            name: "evil__tool".into(),
            description: None,
            risk_level: "moderate".into(),
            input_schema: None,
        });
        assert!(validate(&m).is_err());
    }

    #[test]
    fn validate_rejects_mis_prefixed_trigger() {
        let mut m = fixture();
        m.workflow.triggers.push(WorkflowTriggerSpec {
            kind: "trigger.other.x".into(),
            display_name: "x".into(),
            icon: None,
            config_schema: None,
            output_schema: None,
            subscription_tool: None,
        });
        assert!(validate(&m).is_err());
    }

    #[test]
    fn validate_accepts_correctly_prefixed_trigger() {
        let mut m = fixture();
        m.workflow.triggers.push(WorkflowTriggerSpec {
            kind: "trigger.com_orbit_hello.incoming".into(),
            display_name: "x".into(),
            icon: None,
            config_schema: None,
            output_schema: None,
            subscription_tool: None,
        });
        assert!(validate(&m).is_ok());
    }
}
