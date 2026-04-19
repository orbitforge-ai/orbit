use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

use super::workspace;

// ─── Core types ─────────────────────────────────────────────────────────────

/// Where a skill was discovered from (priority order: agent-local wins).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    AgentLocal,
    OrbitGlobal,
    Standard,
    BuiltIn,
}

impl std::fmt::Display for SkillSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillSource::AgentLocal => write!(f, "agent"),
            SkillSource::OrbitGlobal => write!(f, "global"),
            SkillSource::Standard => write!(f, "standard"),
            SkillSource::BuiltIn => write!(f, "built-in"),
        }
    }
}

/// Tier 1 metadata parsed from SKILL.md frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
    /// Absolute path to the skill directory (None for built-in skills).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<PathBuf>,
    pub source: SkillSource,
}

/// Serializable skill info sent to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

/// Catalog of all discovered skills for an agent.
pub struct SkillCatalog {
    pub skills: Vec<SkillMetadata>,
}

// ─── Built-in skills ────────────────────────────────────────────────────────

struct BuiltInSkill {
    name: &'static str,
    content: &'static str,
}

const BUILTIN_SKILLS: &[BuiltInSkill] = &[
    BuiltInSkill {
        name: "code-review",
        content: include_str!("builtin_skills/code-review/SKILL.md"),
    },
    BuiltInSkill {
        name: "write-tests",
        content: include_str!("builtin_skills/write-tests/SKILL.md"),
    },
    BuiltInSkill {
        name: "create-plugin",
        content: include_str!("builtin_skills/create-plugin/SKILL.md"),
    },
];

// ─── SKILL.md parsing ───────────────────────────────────────────────────────

/// Parse a SKILL.md file into metadata + body content.
/// Uses a lightweight YAML frontmatter parser (no external dep).
pub fn parse_skill_md(
    content: &str,
    source: SkillSource,
    source_path: Option<PathBuf>,
) -> Result<(SkillMetadata, String), String> {
    let trimmed = content.trim_start();

    // Must start with ---
    if !trimmed.starts_with("---") {
        return Err("SKILL.md must start with YAML frontmatter (---)".to_string());
    }

    // Find closing ---
    let after_open = &trimmed[3..];
    let close_idx = after_open
        .find("\n---")
        .ok_or("Missing closing --- for frontmatter")?;

    let yaml_block = &after_open[..close_idx].trim();
    let body = after_open[close_idx + 4..].trim().to_string();

    // Parse YAML key-value pairs
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut license: Option<String> = None;
    let mut compatibility: Option<String> = None;
    let mut allowed_tools: Option<String> = None;
    let mut extra_metadata: Option<HashMap<String, String>> = None;
    let mut in_metadata_block = false;

    for line in yaml_block.lines() {
        let line_trimmed = line.trim();
        if line_trimmed.is_empty() || line_trimmed.starts_with('#') {
            continue;
        }

        // Detect indented metadata sub-keys
        if in_metadata_block && (line.starts_with("  ") || line.starts_with('\t')) {
            if let Some(pos) = line_trimmed.find(':') {
                let key = line_trimmed[..pos].trim().to_string();
                let val = line_trimmed[pos + 1..].trim().trim_matches('"').to_string();
                extra_metadata
                    .get_or_insert_with(HashMap::new)
                    .insert(key, val);
            }
            continue;
        }
        in_metadata_block = false;

        if let Some(pos) = line_trimmed.find(':') {
            let key = line_trimmed[..pos].trim();
            let val = line_trimmed[pos + 1..].trim().trim_matches('"').to_string();

            match key {
                "name" => name = Some(val),
                "description" => description = Some(val),
                "license" => license = Some(val),
                "compatibility" => compatibility = Some(val),
                "allowed-tools" => allowed_tools = Some(val),
                "metadata" => {
                    in_metadata_block = true;
                }
                _ => {
                    warn!(key = key, "Unknown SKILL.md frontmatter key, ignoring");
                }
            }
        }
    }

    let name = name.ok_or("SKILL.md missing required 'name' field")?;
    let description = description
        .filter(|d| !d.is_empty())
        .ok_or("SKILL.md missing required 'description' field")?;

    // Lenient validation — warn but don't reject
    if name.len() > 64 {
        warn!(name = %name, "Skill name exceeds 64 characters");
    }
    if description.len() > 1024 {
        warn!(name = %name, "Skill description exceeds 1024 characters");
    }

    Ok((
        SkillMetadata {
            name,
            description,
            license,
            compatibility,
            allowed_tools,
            metadata: extra_metadata,
            source_path,
            source,
        },
        body,
    ))
}

// ─── Discovery ──────────────────────────────────────────────────────────────

/// Scan a directory for skill subdirectories containing SKILL.md.
fn scan_skills_dir(dir: &Path, source: SkillSource) -> Vec<SkillMetadata> {
    let mut skills = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return skills,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Skip hidden dirs, node_modules, .git etc.
        let dir_name = entry.file_name().to_string_lossy().to_string();
        if dir_name.starts_with('.') || dir_name == "node_modules" {
            continue;
        }

        let skill_md_path = path.join("SKILL.md");
        if !skill_md_path.exists() {
            continue;
        }

        match fs::read_to_string(&skill_md_path) {
            Ok(content) => {
                match parse_skill_md(&content, source.clone(), Some(path.clone())) {
                    Ok((mut meta, _body)) => {
                        // Warn if name doesn't match directory
                        if meta.name != dir_name {
                            warn!(
                                skill = %meta.name,
                                dir = %dir_name,
                                "Skill name doesn't match directory name"
                            );
                        }
                        meta.source_path = Some(path);
                        skills.push(meta);
                    }
                    Err(e) => {
                        warn!(path = %skill_md_path.display(), error = %e, "Failed to parse SKILL.md");
                    }
                }
            }
            Err(e) => {
                warn!(path = %skill_md_path.display(), error = %e, "Failed to read SKILL.md");
            }
        }
    }

    skills
}

/// Discover all available skills for an agent (Tier 1 — metadata only).
/// Returns skills in priority order with deduplication: agent-local > global > standard > built-in.
pub fn discover_skills(agent_id: &str, disabled_skills: &[String]) -> SkillCatalog {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut all_skills: Vec<SkillMetadata> = Vec::new();

    // Scan paths in priority order
    let scan_sources: Vec<(PathBuf, SkillSource)> = vec![
        (
            workspace::agent_dir(agent_id).join("skills"),
            SkillSource::AgentLocal,
        ),
        (global_skills_dir(), SkillSource::OrbitGlobal),
        (standard_skills_dir(), SkillSource::Standard),
    ];

    for (dir, source) in scan_sources {
        let skills = scan_skills_dir(&dir, source);
        for skill in skills {
            if !seen.contains_key(&skill.name) {
                seen.insert(skill.name.clone(), all_skills.len());
                all_skills.push(skill);
            } else {
                debug!(name = %skill.name, "Skill shadowed by higher-priority source");
            }
        }
    }

    // Add built-in skills (lowest priority)
    for builtin in BUILTIN_SKILLS {
        if !seen.contains_key(builtin.name) {
            if let Ok((meta, _body)) = parse_skill_md(builtin.content, SkillSource::BuiltIn, None) {
                seen.insert(meta.name.clone(), all_skills.len());
                all_skills.push(meta);
            }
        }
    }

    // Filter out explicitly disabled skills
    all_skills.retain(|s| !disabled_skills.contains(&s.name));

    // Sort by name for stable ordering
    all_skills.sort_by(|a, b| a.name.cmp(&b.name));

    SkillCatalog { skills: all_skills }
}

// ─── Tier 2: Full instruction loading ───────────────────────────────────────

/// Load the full instructions (body) of a skill by name.
pub fn load_skill_instructions(
    agent_id: &str,
    skill_name: &str,
    disabled_skills: &[String],
) -> Result<String, String> {
    if disabled_skills.contains(&skill_name.to_string()) {
        return Err(format!("Skill '{}' is disabled", skill_name));
    }

    // Check built-in skills first (fast path)
    for builtin in BUILTIN_SKILLS {
        if builtin.name == skill_name {
            let (_meta, body) = parse_skill_md(builtin.content, SkillSource::BuiltIn, None)?;
            return Ok(body);
        }
    }

    // Scan filesystem sources
    let search_dirs: Vec<(PathBuf, SkillSource)> = vec![
        (
            workspace::agent_dir(agent_id).join("skills"),
            SkillSource::AgentLocal,
        ),
        (global_skills_dir(), SkillSource::OrbitGlobal),
        (standard_skills_dir(), SkillSource::Standard),
    ];

    for (dir, source) in search_dirs {
        let skill_dir = dir.join(skill_name);
        let skill_md = skill_dir.join("SKILL.md");
        if skill_md.exists() {
            let content = fs::read_to_string(&skill_md)
                .map_err(|e| format!("Failed to read {}: {}", skill_md.display(), e))?;
            let (_meta, body) = parse_skill_md(&content, source, Some(skill_dir.clone()))?;

            // List bundled resources
            let resources = list_skill_resources(&skill_dir);
            if resources.is_empty() {
                return Ok(body);
            }

            let resource_list = resources
                .iter()
                .map(|r| format!("  <file>{}</file>", r))
                .collect::<Vec<_>>()
                .join("\n");

            return Ok(format!(
                "{}\n\nSkill directory: {}\nRelative paths in this skill resolve against the skill directory.\n\n<skill-resources>\n{}\n</skill-resources>",
                body,
                skill_dir.display(),
                resource_list
            ));
        }
    }

    Err(format!("Skill '{}' not found", skill_name))
}

/// List supporting files in a skill directory (scripts/, references/, assets/).
fn list_skill_resources(skill_dir: &Path) -> Vec<String> {
    let mut resources = Vec::new();

    for subdir in &["scripts", "references", "assets"] {
        let dir = skill_dir.join(subdir);
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    if let Some(name) = path.file_name() {
                        resources.push(format!("{}/{}", subdir, name.to_string_lossy()));
                    }
                }
            }
        }
    }

    resources
}

// ─── Catalog XML builder ────────────────────────────────────────────────────

/// Build the XML catalog block for the system prompt (Tier 1 disclosure).
pub fn build_catalog_xml(catalog: &SkillCatalog) -> String {
    if catalog.skills.is_empty() {
        return String::new();
    }

    let mut xml = String::from("\n\n<available-skills>\nWhen a task matches a skill's description, call the activate_skill tool with its name to load detailed instructions.\n");

    for skill in &catalog.skills {
        xml.push_str(&format!(
            "  <skill name=\"{}\">{}</skill>\n",
            skill.name, skill.description
        ));
    }

    xml.push_str("</available-skills>");
    xml
}

// ─── Skill creation ─────────────────────────────────────────────────────────

/// Create a new skill in the agent's local skills directory.
pub fn create_skill(
    agent_id: &str,
    name: &str,
    description: &str,
    body: &str,
) -> Result<(), String> {
    // Validate name
    validate_skill_name(name)?;

    let skills_dir = workspace::agent_dir(agent_id).join("skills").join(name);
    if skills_dir.exists() {
        return Err(format!("Skill '{}' already exists", name));
    }

    fs::create_dir_all(&skills_dir)
        .map_err(|e| format!("Failed to create skill directory: {}", e))?;

    let content = format!(
        "---\nname: {}\ndescription: {}\n---\n\n{}\n",
        name, description, body
    );

    fs::write(skills_dir.join("SKILL.md"), content)
        .map_err(|e| format!("Failed to write SKILL.md: {}", e))?;

    Ok(())
}

/// Delete a skill from the agent's local skills directory.
pub fn delete_skill(agent_id: &str, skill_name: &str) -> Result<(), String> {
    let skill_dir = workspace::agent_dir(agent_id)
        .join("skills")
        .join(skill_name);
    if !skill_dir.exists() {
        return Err(format!(
            "Skill '{}' not found in agent-local skills",
            skill_name
        ));
    }

    // Only allow deleting agent-local skills
    let skill_md = skill_dir.join("SKILL.md");
    if !skill_md.exists() {
        return Err(format!("'{}' is not a valid skill directory", skill_name));
    }

    fs::remove_dir_all(&skill_dir).map_err(|e| format!("Failed to delete skill: {}", e))?;

    Ok(())
}

// ─── Keyword relevance filtering ────────────────────────────────────────────

/// Stop words excluded from keyword matching (common English words that add noise).
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should", "may", "might", "shall", "can",
    "need", "must", "i", "me", "my", "we", "our", "you", "your", "he", "she", "it", "they", "them",
    "this", "that", "these", "those", "what", "which", "who", "whom", "how", "when", "where",
    "why", "and", "or", "but", "not", "no", "nor", "so", "if", "then", "in", "on", "at", "to",
    "for", "of", "with", "by", "from", "as", "into", "about", "between", "through", "after",
    "before", "above", "up", "out", "off", "over", "under", "again", "just", "also", "very", "too",
    "more", "most", "some", "any", "all", "each", "every", "please", "help", "want", "like",
    "make", "use", "using", "used", "get", "got", "let", "set",
];

/// Extract meaningful keywords from text. Lowercase, split on non-alphanumeric,
/// drop stop words and very short tokens.
fn extract_keywords(text: &str) -> HashSet<String> {
    let stop: HashSet<&str> = STOP_WORDS.iter().copied().collect();

    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3 && !stop.contains(w))
        .map(|w| w.to_string())
        .collect()
}

/// Score how relevant a skill is to a set of query keywords.
/// Returns a value between 0.0 and 1.0.
/// Uses weighted keyword overlap: matches in name count double.
fn relevance_score(skill: &SkillMetadata, query_keywords: &HashSet<String>) -> f64 {
    if query_keywords.is_empty() {
        return 0.0;
    }

    let name_keywords = extract_keywords(&skill.name);
    let desc_keywords = extract_keywords(&skill.description);

    // Name matches are high signal (weight 2x)
    let name_hits = query_keywords.intersection(&name_keywords).count() as f64 * 2.0;
    // Description matches are normal signal
    let desc_hits = query_keywords.intersection(&desc_keywords).count() as f64;

    let total_hits = name_hits + desc_hits;
    let max_possible = query_keywords.len() as f64;

    // Normalize to 0..1, capped at 1.0
    (total_hits / max_possible).min(1.0)
}

/// Minimum relevance score for a skill to be included in the catalog.
const RELEVANCE_THRESHOLD: f64 = 0.15;

/// Filter a catalog to only include skills relevant to the given context text.
/// Agent-local skills are always included (user explicitly installed them).
/// Returns the full catalog if context_text is empty or None.
pub fn filter_relevant_skills(catalog: SkillCatalog, context_text: Option<&str>) -> SkillCatalog {
    let context_text = match context_text {
        Some(t) if !t.trim().is_empty() => t,
        _ => return catalog, // No context to filter on — return all
    };

    let query_keywords = extract_keywords(context_text);
    if query_keywords.is_empty() {
        return catalog;
    }

    let total_before = catalog.skills.len();

    let filtered: Vec<SkillMetadata> = catalog
        .skills
        .into_iter()
        .filter(|skill| {
            // Always include agent-local skills
            if skill.source == SkillSource::AgentLocal {
                return true;
            }
            relevance_score(skill, &query_keywords) >= RELEVANCE_THRESHOLD
        })
        .collect();

    debug!(
        total_before = total_before,
        total_after = filtered.len(),
        "Filtered skills by relevance"
    );

    SkillCatalog { skills: filtered }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn validate_skill_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("Skill name must be 1-64 characters".to_string());
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err("Skill name must not start or end with a hyphen".to_string());
    }
    if name.contains("--") {
        return Err("Skill name must not contain consecutive hyphens".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(
            "Skill name may only contain lowercase letters, digits, and hyphens".to_string(),
        );
    }
    Ok(())
}

/// Global skills directory: ~/.orbit/skills/
pub fn global_skills_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".orbit").join("skills")
}

/// Cross-client standard skills directory: ~/.agents/skills/
fn standard_skills_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".agents").join("skills")
}
