use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;

/// Convert a name to a URL-safe slug (lowercase, hyphens, no special chars).
pub fn slugify(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // Collapse multiple hyphens and trim
    let mut result = String::new();
    let mut prev_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_hyphen && !result.is_empty() {
                result.push('-');
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }
    result.trim_end_matches('-').to_string()
}

/// Root directory for all agent workspaces.
pub fn agents_root() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".orbit").join("agents")
}

/// Directory for a specific agent.
pub fn agent_dir(agent_id: &str) -> PathBuf {
    agents_root().join(agent_id)
}

/// Root directory for all project workspaces.
pub fn projects_root() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".orbit").join("projects")
}

/// Workspace directory for a specific project (the shared files directory).
pub fn project_workspace_dir(project_id: &str) -> PathBuf {
    projects_root().join(project_id).join("workspace")
}

/// Create the project workspace directory on disk.
pub fn init_project_workspace(project_id: &str) -> Result<(), String> {
    let workspace = project_workspace_dir(project_id);
    fs::create_dir_all(&workspace)
        .map_err(|e| format!("failed to create project workspace: {}", e))?;
    info!(project_id = project_id, path = %workspace.display(), "Initialised project workspace");
    Ok(())
}

/// List files in a project workspace directory (path-sandboxed to the workspace).
pub fn list_project_workspace_files(
    project_id: &str,
    relative_path: &str,
) -> Result<Vec<FileEntry>, String> {
    let root = project_workspace_dir(project_id);
    if !root.exists() {
        fs::create_dir_all(&root)
            .map_err(|e| format!("failed to create project workspace: {}", e))?;
    }
    let path = validate_path(&root, relative_path)?;
    if !path.is_dir() {
        return Err(format!("not a directory: {}", relative_path));
    }
    let mut entries = Vec::new();
    let read_dir = fs::read_dir(&path).map_err(|e| format!("failed to read directory: {}", e))?;
    for entry in read_dir {
        let entry = entry.map_err(|e| format!("failed to read entry: {}", e))?;
        let metadata = entry
            .metadata()
            .map_err(|e| format!("failed to read metadata: {}", e))?;
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                Some(dt.to_rfc3339())
            })
            .unwrap_or_default();
        entries.push(FileEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            is_dir: metadata.is_dir(),
            size_bytes: metadata.len(),
            modified_at,
        });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(entries)
}

/// Read a file from a project workspace (path-sandboxed).
pub fn read_project_workspace_file(
    project_id: &str,
    relative_path: &str,
) -> Result<String, String> {
    let root = project_workspace_dir(project_id);
    let path = validate_path(&root, relative_path)?;
    fs::read_to_string(&path).map_err(|e| format!("failed to read file: {}", e))
}

/// Write a file to a project workspace (path-sandboxed).
pub fn write_project_workspace_file(
    project_id: &str,
    relative_path: &str,
    content: &str,
) -> Result<(), String> {
    let root = project_workspace_dir(project_id);
    let path = validate_path(&root, relative_path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create parent directories: {}", e))?;
    }
    fs::write(&path, content).map_err(|e| format!("failed to write file: {}", e))
}

/// Delete a file from a project workspace (path-sandboxed).
pub fn delete_project_workspace_file(project_id: &str, relative_path: &str) -> Result<(), String> {
    let root = project_workspace_dir(project_id);
    let path = validate_path(&root, relative_path)?;
    if path.is_dir() {
        fs::remove_dir_all(&path).map_err(|e| format!("failed to delete directory: {}", e))
    } else {
        fs::remove_file(&path).map_err(|e| format!("failed to delete file: {}", e))
    }
}

/// Agent workspace configuration stored in config.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIdentityConfig {
    #[serde(default = "default_identity_preset_id")]
    pub preset_id: String,
    #[serde(default = "default_identity_name")]
    pub identity_name: String,
    #[serde(default = "default_identity_voice")]
    pub voice: String,
    #[serde(default = "default_identity_vibe")]
    pub vibe: String,
    #[serde(default = "default_identity_warmth")]
    pub warmth: u8,
    #[serde(default = "default_identity_directness")]
    pub directness: u8,
    #[serde(default = "default_identity_humor")]
    pub humor: u8,
    #[serde(default)]
    pub custom_note: Option<String>,
    #[serde(default)]
    pub avatar_enabled: bool,
    #[serde(default = "default_avatar_archetype")]
    pub avatar_archetype: String,
    #[serde(default)]
    pub avatar_speak_aloud: bool,
}

/// A saved permission rule for fine-grained tool access control.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRule {
    /// Unique ID for this rule.
    pub id: String,
    /// Tool name this rule applies to (e.g. "shell_command", "write_file", "send_message").
    pub tool: String,
    /// Glob-like pattern matched against tool input.
    /// shell_command: matched against command string (e.g. "echo *", "git commit *").
    /// write_file: matched against path (e.g. "*.md", "src/**").
    /// send_message: matched against target agent name (e.g. "research-agent").
    pub pattern: String,
    /// The decision: "allow" or "deny".
    pub decision: String,
    /// When this rule was created (ISO 8601).
    pub created_at: String,
    /// Optional human-readable description.
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkspaceConfig {
    pub provider: String,
    pub model: String,
    pub temperature: f64,
    pub max_iterations: u32,
    pub max_total_tokens: u32,
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub compaction_threshold: Option<f64>,
    #[serde(default)]
    pub compaction_retain_count: Option<u32>,
    #[serde(default)]
    pub context_window_override: Option<u32>,
    #[serde(default = "default_search_provider")]
    pub web_search_provider: String,
    #[serde(default)]
    pub disabled_skills: Vec<String>,
    #[serde(default = "default_agent_identity")]
    pub identity: AgentIdentityConfig,
    #[serde(default)]
    pub permission_rules: Vec<PermissionRule>,
    #[serde(default = "default_permission_mode")]
    pub permission_mode: String,
    #[serde(default = "default_true")]
    pub memory_enabled: bool,
    #[serde(default = "default_staleness_days")]
    pub memory_staleness_threshold_days: u32,
    #[serde(default)]
    pub role_id: Option<String>,
    #[serde(default)]
    pub role_system_instructions: Option<String>,
}

fn default_permission_mode() -> String {
    "normal".to_string()
}

fn default_true() -> bool {
    true
}

fn default_staleness_days() -> u32 {
    30
}

fn default_search_provider() -> String {
    "brave".to_string()
}

fn default_identity_preset_id() -> String {
    "balanced_assistant".to_string()
}

fn default_identity_name() -> String {
    "Balanced Assistant".to_string()
}

fn default_identity_voice() -> String {
    "neutral".to_string()
}

fn default_identity_vibe() -> String {
    "balanced, clear, and approachable".to_string()
}

fn default_identity_warmth() -> u8 {
    55
}

fn default_identity_directness() -> u8 {
    55
}

fn default_identity_humor() -> u8 {
    20
}

fn default_avatar_archetype() -> String {
    "auto".to_string()
}

pub fn default_agent_identity() -> AgentIdentityConfig {
    builtin_identity_preset("balanced_assistant")
}

impl Default for AgentWorkspaceConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            temperature: 0.7,
            max_iterations: 25,
            max_total_tokens: 200_000,
            allowed_tools: vec![
                "shell_command".to_string(),
                "read_file".to_string(),
                "write_file".to_string(),
                "edit_file".to_string(),
                "list_files".to_string(),
                "grep".to_string(),
                "web_search".to_string(),
                "web_fetch".to_string(),
                "activate_skill".to_string(),
                "remember".to_string(),
                "search_memory".to_string(),
                "finish".to_string(),
            ],
            compaction_threshold: None,
            compaction_retain_count: None,
            context_window_override: None,
            web_search_provider: default_search_provider(),
            disabled_skills: Vec::new(),
            identity: default_agent_identity(),
            permission_rules: Vec::new(),
            permission_mode: default_permission_mode(),
            memory_enabled: true,
            memory_staleness_threshold_days: default_staleness_days(),
            role_id: None,
            role_system_instructions: None,
        }
    }
}

pub fn builtin_identity_preset(preset_id: &str) -> AgentIdentityConfig {
    match preset_id {
        "warm_guide" => AgentIdentityConfig {
            preset_id: "warm_guide".to_string(),
            identity_name: "Warm Guide".to_string(),
            voice: "warm".to_string(),
            vibe: "encouraging and supportive".to_string(),
            warmth: 80,
            directness: 40,
            humor: 25,
            custom_note: None,
            avatar_enabled: false,
            avatar_archetype: default_avatar_archetype(),
            avatar_speak_aloud: false,
        },
        "crisp_operator" => AgentIdentityConfig {
            preset_id: "crisp_operator".to_string(),
            identity_name: "Crisp Operator".to_string(),
            voice: "crisp".to_string(),
            vibe: "efficient, composed, and no-nonsense".to_string(),
            warmth: 25,
            directness: 85,
            humor: 5,
            custom_note: None,
            avatar_enabled: false,
            avatar_archetype: default_avatar_archetype(),
            avatar_speak_aloud: false,
        },
        "calm_analyst" => AgentIdentityConfig {
            preset_id: "calm_analyst".to_string(),
            identity_name: "Calm Analyst".to_string(),
            voice: "calm".to_string(),
            vibe: "measured, thoughtful, and analytical".to_string(),
            warmth: 40,
            directness: 70,
            humor: 10,
            custom_note: None,
            avatar_enabled: false,
            avatar_archetype: default_avatar_archetype(),
            avatar_speak_aloud: false,
        },
        "playful_creative" => AgentIdentityConfig {
            preset_id: "playful_creative".to_string(),
            identity_name: "Playful Creative".to_string(),
            voice: "bright".to_string(),
            vibe: "inventive, lively, and imaginative".to_string(),
            warmth: 70,
            directness: 45,
            humor: 60,
            custom_note: None,
            avatar_enabled: false,
            avatar_archetype: default_avatar_archetype(),
            avatar_speak_aloud: false,
        },
        "steady_coach" => AgentIdentityConfig {
            preset_id: "steady_coach".to_string(),
            identity_name: "Steady Coach".to_string(),
            voice: "steady".to_string(),
            vibe: "confident, motivating, and grounded".to_string(),
            warmth: 65,
            directness: 65,
            humor: 15,
            custom_note: None,
            avatar_enabled: false,
            avatar_archetype: default_avatar_archetype(),
            avatar_speak_aloud: false,
        },
        _ => AgentIdentityConfig {
            preset_id: "balanced_assistant".to_string(),
            identity_name: "Balanced Assistant".to_string(),
            voice: "neutral".to_string(),
            vibe: "balanced, clear, and approachable".to_string(),
            warmth: 55,
            directness: 55,
            humor: 20,
            custom_note: None,
            avatar_enabled: false,
            avatar_archetype: default_avatar_archetype(),
            avatar_speak_aloud: false,
        },
    }
}

pub fn normalize_agent_identity(identity: &AgentIdentityConfig) -> AgentIdentityConfig {
    if identity.preset_id != "custom" {
        let mut preset = builtin_identity_preset(&identity.preset_id);
        // Preserve avatar settings even for named presets
        preset.avatar_enabled = identity.avatar_enabled;
        preset.avatar_archetype = identity.avatar_archetype.clone();
        preset.avatar_speak_aloud = identity.avatar_speak_aloud;
        return preset;
    }

    let base = default_agent_identity();
    AgentIdentityConfig {
        preset_id: "custom".to_string(),
        identity_name: sanitize_text(&identity.identity_name, 60)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Custom Identity".to_string()),
        voice: sanitize_text(&identity.voice, 40)
            .filter(|value| !value.is_empty())
            .unwrap_or(base.voice),
        vibe: sanitize_text(&identity.vibe, 80)
            .filter(|value| !value.is_empty())
            .unwrap_or(base.vibe),
        warmth: identity.warmth.min(100),
        directness: identity.directness.min(100),
        humor: identity.humor.min(100),
        custom_note: sanitize_text_option(identity.custom_note.as_deref(), 240),
        avatar_enabled: identity.avatar_enabled,
        avatar_archetype: identity.avatar_archetype.clone(),
        avatar_speak_aloud: identity.avatar_speak_aloud,
    }
}

pub fn normalize_agent_config(config: AgentWorkspaceConfig) -> AgentWorkspaceConfig {
    AgentWorkspaceConfig {
        identity: normalize_agent_identity(&config.identity),
        ..config
    }
}

pub fn identity_score_descriptor(value: u8) -> &'static str {
    match value {
        0..=33 => "low",
        34..=66 => "medium",
        _ => "high",
    }
}

pub fn build_identity_prompt_summary(agent_name: &str, identity: &AgentIdentityConfig) -> String {
    let resolved = normalize_agent_identity(identity);
    let resolved_agent_name = sanitize_text(agent_name, 80)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "this agent".to_string());
    let mut summary = format!(
        "You are {}. These identity settings shape only your tone and conversational style, not your name, role, or self-description. If asked who you are, identify yourself as {}, not as an identity preset or style label.",
        resolved_agent_name, resolved_agent_name
    );

    if resolved.preset_id == "custom" && !resolved.identity_name.is_empty() {
        summary.push_str(&format!(
            " Internal style label: '{}'.",
            resolved.identity_name
        ));
    }

    summary.push_str(&format!(
        " Communicate in a {} voice that feels {}, with {} warmth, {} directness, and {} humor.",
        resolved.voice,
        resolved.vibe,
        identity_score_descriptor(resolved.warmth),
        identity_score_descriptor(resolved.directness),
        identity_score_descriptor(resolved.humor)
    ));

    if let Some(custom_note) = resolved.custom_note.as_deref() {
        if !custom_note.is_empty() {
            summary.push_str(&format!(" Additional identity note: {}.", custom_note));
        }
    }

    summary
}

fn sanitize_text(value: &str, max_len: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed.chars().take(max_len).collect())
}

fn sanitize_text_option(value: Option<&str>, max_len: usize) -> Option<String> {
    value.and_then(|text| sanitize_text(text, max_len))
}

/// File entry returned by list operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub modified_at: String,
}

/// Validate that a path stays within the agent's root directory.
/// Returns the resolved absolute path on success.
fn validate_path(base: &Path, requested: &str) -> Result<PathBuf, String> {
    let resolved = base.join(requested);

    // For existing paths, canonicalize directly
    if resolved.exists() {
        let canonical = resolved
            .canonicalize()
            .map_err(|e| format!("failed to resolve path: {}", e))?;
        let base_canonical = base
            .canonicalize()
            .map_err(|e| format!("failed to resolve base path: {}", e))?;
        if !canonical.starts_with(&base_canonical) {
            return Err(format!("path escapes agent workspace: {}", requested));
        }
        return Ok(canonical);
    }

    // For new files, canonicalize the parent and append the filename
    let parent = resolved.parent().ok_or("invalid path: no parent")?;
    if !parent.exists() {
        // Allow creating parent dirs, but validate the deepest existing ancestor
        let mut ancestor = parent.to_path_buf();
        while !ancestor.exists() {
            ancestor = ancestor
                .parent()
                .ok_or("invalid path: no existing ancestor")?
                .to_path_buf();
        }
        let ancestor_canonical = ancestor
            .canonicalize()
            .map_err(|e| format!("failed to resolve ancestor: {}", e))?;
        let base_canonical = base
            .canonicalize()
            .map_err(|e| format!("failed to resolve base path: {}", e))?;
        if !ancestor_canonical.starts_with(&base_canonical) {
            return Err(format!("path escapes agent workspace: {}", requested));
        }
        return Ok(resolved);
    }

    let parent_canonical = parent
        .canonicalize()
        .map_err(|e| format!("failed to resolve parent: {}", e))?;
    let base_canonical = base
        .canonicalize()
        .map_err(|e| format!("failed to resolve base path: {}", e))?;
    if !parent_canonical.starts_with(&base_canonical) {
        return Err(format!("path escapes agent workspace: {}", requested));
    }

    let filename = resolved.file_name().ok_or("invalid path: no filename")?;
    Ok(parent_canonical.join(filename))
}

pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are a helpful autonomous agent. Follow the user's goal and use the available tools to accomplish it.

When the user asks you to recall something they previously told you, asks what you remember, or asks for a preference/fact that may be stored in memory, use `search_memory` before answering. If the user shares a durable preference, instruction, or project fact that should persist, use `remember`.

When you are done, call the `finish` tool with a summary of what you accomplished.
"#;

/// Versioned blob stored in the `model_config` SQLite column and synced to Supabase.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredModelConfig {
    pub version: u32,
    pub config: AgentWorkspaceConfig,
    pub system_prompt: String,
}

/// Serialize the agent's on-disk config.json + system_prompt.md into the model_config blob.
pub fn serialize_model_config(agent_id: &str) -> Result<String, String> {
    let config = load_agent_config(agent_id).unwrap_or_default();
    let system_prompt = read_workspace_file(agent_id, "system_prompt.md")
        .unwrap_or_else(|_| DEFAULT_SYSTEM_PROMPT.to_string());
    let stored = StoredModelConfig {
        version: 1,
        config,
        system_prompt,
    };
    serde_json::to_string(&stored).map_err(|e| format!("failed to serialize model_config: {}", e))
}

/// Deserialize a model_config blob and write config.json + system_prompt.md to disk.
/// No-op for empty or legacy "{}" blobs.
pub fn apply_model_config_to_disk(agent_id: &str, model_config_json: &str) -> Result<(), String> {
    if model_config_json.is_empty() || model_config_json == "{}" {
        return Ok(());
    }
    let stored: StoredModelConfig = serde_json::from_str(model_config_json)
        .map_err(|e| format!("failed to parse model_config: {}", e))?;
    let dir = agent_dir(agent_id);
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create agent dir: {}", e))?;
    save_agent_config(agent_id, &stored.config)?;
    fs::write(dir.join("system_prompt.md"), &stored.system_prompt)
        .map_err(|e| format!("failed to write system_prompt.md: {}", e))
}

const DEFAULT_PULSE_PROMPT: &str = r#"# Agent Pulse

Describe what this agent should do on each pulse cycle.

For example:
- Check system status and report anomalies
- Summarize recent activity
- Review and prioritize pending items
"#;

/// Create the workspace directory structure for a new agent.
pub fn init_agent_workspace(agent_id: &str) -> Result<(), String> {
    let root = agent_dir(agent_id);
    let memory_dir = root.join("memory");
    let workspace_dir = root.join("workspace");
    let skills_dir = root.join("skills");

    fs::create_dir_all(&memory_dir).map_err(|e| format!("failed to create memory dir: {}", e))?;
    fs::create_dir_all(&workspace_dir)
        .map_err(|e| format!("failed to create workspace dir: {}", e))?;
    fs::create_dir_all(&skills_dir).map_err(|e| format!("failed to create skills dir: {}", e))?;

    // Write default system prompt if it doesn't exist
    let prompt_path = root.join("system_prompt.md");
    if !prompt_path.exists() {
        fs::write(&prompt_path, DEFAULT_SYSTEM_PROMPT)
            .map_err(|e| format!("failed to write system_prompt.md: {}", e))?;
    }

    // Write default config if it doesn't exist
    let config_path = root.join("config.json");
    if !config_path.exists() {
        let default_config = AgentWorkspaceConfig::default();
        let json = serde_json::to_string_pretty(&default_config)
            .map_err(|e| format!("failed to serialize config: {}", e))?;
        fs::write(&config_path, json).map_err(|e| format!("failed to write config.json: {}", e))?;
    }

    // Write default pulse prompt if it doesn't exist
    let pulse_path = root.join("pulse.md");
    if !pulse_path.exists() {
        fs::write(&pulse_path, DEFAULT_PULSE_PROMPT)
            .map_err(|e| format!("failed to write pulse.md: {}", e))?;
    }

    info!(agent_id = agent_id, path = %root.display(), "Initialised agent workspace");
    Ok(())
}

/// Read a file from the agent's workspace (path-sandboxed).
pub fn read_workspace_file(agent_id: &str, relative_path: &str) -> Result<String, String> {
    let root = agent_dir(agent_id);
    let path = validate_path(&root, relative_path)?;
    fs::read_to_string(&path).map_err(|e| format!("failed to read file: {}", e))
}

/// Write a file to the agent's workspace (path-sandboxed).
pub fn write_workspace_file(
    agent_id: &str,
    relative_path: &str,
    content: &str,
) -> Result<(), String> {
    let root = agent_dir(agent_id);
    let path = validate_path(&root, relative_path)?;

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create parent directories: {}", e))?;
    }

    fs::write(&path, content).map_err(|e| format!("failed to write file: {}", e))
}

/// List files in a directory within the agent's workspace (path-sandboxed).
pub fn list_workspace_files(agent_id: &str, relative_path: &str) -> Result<Vec<FileEntry>, String> {
    let root = agent_dir(agent_id);
    let path = validate_path(&root, relative_path)?;

    if !path.is_dir() {
        return Err(format!("not a directory: {}", relative_path));
    }

    let mut entries = Vec::new();
    let read_dir = fs::read_dir(&path).map_err(|e| format!("failed to read directory: {}", e))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("failed to read entry: {}", e))?;
        let metadata = entry
            .metadata()
            .map_err(|e| format!("failed to read metadata: {}", e))?;

        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                Some(dt.to_rfc3339())
            })
            .unwrap_or_default();

        entries.push(FileEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            is_dir: metadata.is_dir(),
            size_bytes: metadata.len(),
            modified_at,
        });
    }

    entries.sort_by(|a, b| {
        // Directories first, then alphabetical
        b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name))
    });

    Ok(entries)
}

/// Delete a file from the agent's workspace (path-sandboxed).
pub fn delete_workspace_file(agent_id: &str, relative_path: &str) -> Result<(), String> {
    let root = agent_dir(agent_id);
    let path = validate_path(&root, relative_path)?;

    if path == root.canonicalize().unwrap_or(root.clone()) {
        return Err("cannot delete agent root directory".to_string());
    }

    if path.is_dir() {
        fs::remove_dir_all(&path).map_err(|e| format!("failed to delete directory: {}", e))
    } else {
        fs::remove_file(&path).map_err(|e| format!("failed to delete file: {}", e))
    }
}

/// Load the agent's workspace configuration from config.json.
pub fn load_agent_config(agent_id: &str) -> Result<AgentWorkspaceConfig, String> {
    let config_path = agent_dir(agent_id).join("config.json");
    if !config_path.exists() {
        return Ok(AgentWorkspaceConfig::default());
    }
    let content =
        fs::read_to_string(&config_path).map_err(|e| format!("failed to read config: {}", e))?;
    let config: AgentWorkspaceConfig =
        serde_json::from_str(&content).map_err(|e| format!("failed to parse config: {}", e))?;
    Ok(normalize_agent_config(config))
}

/// Save the agent's workspace configuration to config.json.
pub fn save_agent_config(agent_id: &str, config: &AgentWorkspaceConfig) -> Result<(), String> {
    let dir = agent_dir(agent_id);
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create agent directory: {}", e))?;
    let config_path = dir.join("config.json");
    let normalized = normalize_agent_config(config.clone());
    let json = serde_json::to_string_pretty(&normalized)
        .map_err(|e| format!("failed to serialize config: {}", e))?;
    fs::write(&config_path, json).map_err(|e| format!("failed to write config: {}", e))
}

#[cfg(test)]
mod tests {
    use super::{
        build_identity_prompt_summary, builtin_identity_preset, default_agent_identity,
        identity_score_descriptor, normalize_agent_identity, AgentIdentityConfig,
        AgentWorkspaceConfig,
    };

    #[test]
    fn missing_identity_defaults_to_balanced_assistant() {
        let parsed: AgentWorkspaceConfig = serde_json::from_str(
            r#"{
                "provider": "anthropic",
                "model": "claude-sonnet-4-20250514",
                "temperature": 0.7,
                "maxIterations": 25,
                "maxTotalTokens": 200000,
                "allowedTools": ["finish"],
                "webSearchProvider": "brave",
                "disabledSkills": []
            }"#,
        )
        .expect("config should deserialize");

        assert_eq!(parsed.identity.preset_id, "balanced_assistant");
        assert_eq!(parsed.identity.identity_name, "Balanced Assistant");
    }

    #[test]
    fn built_in_preset_resolution_returns_exact_values() {
        let preset = normalize_agent_identity(&AgentIdentityConfig {
            preset_id: "warm_guide".to_string(),
            ..default_agent_identity()
        });

        assert_eq!(preset.identity_name, "Warm Guide");
        assert_eq!(preset.voice, "warm");
        assert_eq!(preset.warmth, 80);
        assert_eq!(preset.directness, 40);
        assert_eq!(preset.humor, 25);
    }

    #[test]
    fn score_descriptor_uses_expected_buckets() {
        assert_eq!(identity_score_descriptor(0), "low");
        assert_eq!(identity_score_descriptor(33), "low");
        assert_eq!(identity_score_descriptor(34), "medium");
        assert_eq!(identity_score_descriptor(66), "medium");
        assert_eq!(identity_score_descriptor(67), "high");
        assert_eq!(identity_score_descriptor(100), "high");
    }

    #[test]
    fn identity_prompt_summary_includes_custom_note() {
        let summary = build_identity_prompt_summary(
            "Orbit",
            &AgentIdentityConfig {
                preset_id: "custom".to_string(),
                identity_name: "Studio Host".to_string(),
                voice: "bright".to_string(),
                vibe: "inventive and welcoming".to_string(),
                warmth: 70,
                directness: 45,
                humor: 60,
                custom_note: Some("Keep the energy grounded.".to_string()),
                avatar_enabled: false,
                avatar_archetype: "auto".to_string(),
                avatar_speak_aloud: false,
            },
        );

        assert!(summary.contains("You are Orbit."));
        assert!(summary.contains("If asked who you are, identify yourself as Orbit"));
        assert!(summary.contains("Internal style label: 'Studio Host'."));
        assert!(summary.contains("high warmth"));
        assert!(summary.contains("medium directness"));
        assert!(summary.contains("Additional identity note: Keep the energy grounded."));
    }

    #[test]
    fn built_in_identity_prompt_summary_does_not_expose_preset_name() {
        let summary = build_identity_prompt_summary(
            "Orbit",
            &AgentIdentityConfig {
                preset_id: "warm_guide".to_string(),
                ..default_agent_identity()
            },
        );

        assert!(summary.contains("tone and conversational style"));
        assert!(summary.contains("identify yourself as Orbit"));
        assert!(!summary.contains("Warm Guide"));
    }

    #[test]
    fn unknown_preset_falls_back_to_balanced_assistant() {
        let preset = builtin_identity_preset("does_not_exist");

        assert_eq!(preset.preset_id, "balanced_assistant");
        assert_eq!(preset.identity_name, "Balanced Assistant");
    }
}
