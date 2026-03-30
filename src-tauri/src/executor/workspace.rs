use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;

/// Convert a name to a URL-safe slug (lowercase, hyphens, no special chars).
pub fn slugify(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
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

/// Agent workspace configuration stored in config.json.
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
}

fn default_search_provider() -> String {
    "brave".to_string()
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
                "list_files".to_string(),
                "web_search".to_string(),
                "activate_skill".to_string(),
                "finish".to_string(),
            ],
            compaction_threshold: None,
            compaction_retain_count: None,
            context_window_override: None,
            web_search_provider: default_search_provider(),
            disabled_skills: Vec::new(),
        }
    }
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

    let filename = resolved
        .file_name()
        .ok_or("invalid path: no filename")?;
    Ok(parent_canonical.join(filename))
}

const DEFAULT_SYSTEM_PROMPT: &str = r#"You are a helpful autonomous agent. Follow the user's goal and use the available tools to accomplish it.

When you are done, call the `finish` tool with a summary of what you accomplished.
"#;

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
    fs::create_dir_all(&skills_dir)
        .map_err(|e| format!("failed to create skills dir: {}", e))?;

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
        fs::write(&config_path, json)
            .map_err(|e| format!("failed to write config.json: {}", e))?;
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
pub fn list_workspace_files(
    agent_id: &str,
    relative_path: &str,
) -> Result<Vec<FileEntry>, String> {
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
    serde_json::from_str(&content).map_err(|e| format!("failed to parse config: {}", e))
}

/// Save the agent's workspace configuration to config.json.
pub fn save_agent_config(agent_id: &str, config: &AgentWorkspaceConfig) -> Result<(), String> {
    let dir = agent_dir(agent_id);
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create agent directory: {}", e))?;
    let config_path = dir.join("config.json");
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("failed to serialize config: {}", e))?;
    fs::write(&config_path, json).map_err(|e| format!("failed to write config: {}", e))
}
