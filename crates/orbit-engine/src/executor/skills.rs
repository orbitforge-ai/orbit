use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

use crate::db::connection::DbPool;
use crate::executor::tools::helpers::{compile_globs, matches_globs};

use super::workspace;

const MAX_LISTING_DESC_CHARS: usize = 250;
const MAX_CATALOG_CHARS: usize = 8_000;
const CATALOG_HEADER: &str = "\n\n<available-skills>\nWhen a task matches one of these skills, call the activate_skill tool with the exact skill name before proceeding.\nDo not claim a skill is loaded unless it appears under <active-skills> or you have just activated it.\n";
const CATALOG_FOOTER: &str = "</available-skills>";

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
    pub when_to_use: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub paths: Vec<String>,
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
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

/// Catalog of all discovered skills for an agent.
pub struct SkillCatalog {
    pub skills: Vec<SkillMetadata>,
}

#[derive(Debug, Clone)]
pub struct ActiveSkillRecord {
    pub skill_name: String,
    pub instructions: String,
    pub source_path: Option<String>,
    pub activated_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedSkill {
    pub(crate) metadata: SkillMetadata,
    pub(crate) instructions: String,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrontmatterBlock {
    None,
    Metadata,
    Paths,
}

/// Parse a SKILL.md file into metadata + body content.
/// Uses a lightweight YAML frontmatter parser (no external dep).
pub fn parse_skill_md(
    content: &str,
    source: SkillSource,
    source_path: Option<PathBuf>,
) -> Result<(SkillMetadata, String), String> {
    let trimmed = content.trim_start();

    if !trimmed.starts_with("---") {
        return Err("SKILL.md must start with YAML frontmatter (---)".to_string());
    }

    let after_open = &trimmed[3..];
    let close_idx = after_open
        .find("\n---")
        .ok_or("Missing closing --- for frontmatter")?;

    let yaml_block = &after_open[..close_idx].trim();
    let body = after_open[close_idx + 4..].trim().to_string();

    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut when_to_use: Option<String> = None;
    let mut license: Option<String> = None;
    let mut compatibility: Option<String> = None;
    let mut allowed_tools: Option<String> = None;
    let mut extra_metadata: Option<HashMap<String, String>> = None;
    let mut paths: Vec<String> = Vec::new();
    let mut block = FrontmatterBlock::None;

    for raw_line in yaml_block.lines() {
        let line_trimmed = raw_line.trim();
        if line_trimmed.is_empty() || line_trimmed.starts_with('#') {
            continue;
        }

        let is_indented = raw_line.starts_with("  ") || raw_line.starts_with('\t');
        if is_indented {
            match block {
                FrontmatterBlock::Metadata => {
                    if let Some(pos) = line_trimmed.find(':') {
                        let key = line_trimmed[..pos].trim().to_string();
                        let val = strip_quotes(line_trimmed[pos + 1..].trim()).to_string();
                        extra_metadata
                            .get_or_insert_with(HashMap::new)
                            .insert(key, val);
                        continue;
                    }
                }
                FrontmatterBlock::Paths => {
                    if let Some(item) = line_trimmed.strip_prefix("- ") {
                        let item = strip_quotes(item.trim());
                        if item.is_empty() {
                            return Err("SKILL.md 'paths' entries must not be empty".to_string());
                        }
                        validate_skill_path_pattern(item)?;
                        paths.push(item.to_string());
                        continue;
                    }
                    return Err(
                        "SKILL.md 'paths' entries must be written as an indented '- pattern' list"
                            .to_string(),
                    );
                }
                FrontmatterBlock::None => {}
            }
        }

        block = FrontmatterBlock::None;

        let Some(pos) = line_trimmed.find(':') else {
            continue;
        };
        let key = line_trimmed[..pos].trim();
        let raw_val = line_trimmed[pos + 1..].trim();
        let val = strip_quotes(raw_val).to_string();

        match key {
            "name" => name = Some(val),
            "description" => description = Some(val),
            "when-to-use" => when_to_use = Some(val),
            "license" => license = Some(val),
            "compatibility" => compatibility = Some(val),
            "allowed-tools" => allowed_tools = Some(val),
            "metadata" => block = FrontmatterBlock::Metadata,
            "paths" => {
                if raw_val.is_empty() {
                    block = FrontmatterBlock::Paths;
                } else {
                    paths.extend(parse_paths_value(raw_val)?);
                }
            }
            _ => {
                warn!(key = key, "Unknown SKILL.md frontmatter key, ignoring");
            }
        }
    }

    let name = name.ok_or("SKILL.md missing required 'name' field")?;
    let description = description
        .filter(|d| !d.is_empty())
        .ok_or("SKILL.md missing required 'description' field")?;

    if block == FrontmatterBlock::Paths && paths.is_empty() {
        return Err("SKILL.md 'paths' must contain at least one pattern".to_string());
    }

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
            when_to_use,
            license,
            compatibility,
            allowed_tools,
            metadata: extra_metadata,
            paths,
            source_path,
            source,
        },
        body,
    ))
}

fn parse_paths_value(raw_val: &str) -> Result<Vec<String>, String> {
    let raw_val = raw_val.trim();
    if raw_val.is_empty() {
        return Err("SKILL.md 'paths' must not be empty".to_string());
    }

    let values = if raw_val.starts_with('[') {
        if !raw_val.ends_with(']') {
            return Err("SKILL.md 'paths' inline list must end with ']'".to_string());
        }
        let inner = &raw_val[1..raw_val.len() - 1];
        inner
            .split(',')
            .map(|item| strip_quotes(item.trim()).to_string())
            .collect::<Vec<_>>()
    } else {
        vec![strip_quotes(raw_val).to_string()]
    };

    if values.is_empty() || values.iter().any(|item| item.is_empty()) {
        return Err("SKILL.md 'paths' entries must not be empty".to_string());
    }

    for value in &values {
        validate_skill_path_pattern(value)?;
    }

    Ok(values)
}

fn strip_quotes(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn validate_skill_path_pattern(pattern: &str) -> Result<(), String> {
    compile_globs(pattern)
        .map(|_| ())
        .map_err(|e| format!("SKILL.md invalid 'paths' pattern '{}': {}", pattern, e))
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

        let dir_name = entry.file_name().to_string_lossy().to_string();
        if dir_name.starts_with('.') || dir_name == "node_modules" {
            continue;
        }

        let skill_md_path = path.join("SKILL.md");
        if !skill_md_path.exists() {
            continue;
        }

        match fs::read_to_string(&skill_md_path) {
            Ok(content) => match parse_skill_md(&content, source.clone(), Some(path.clone())) {
                Ok((mut meta, _body)) => {
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
            },
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
    let mut seen: HashSet<String> = HashSet::new();
    let mut all_skills: Vec<SkillMetadata> = Vec::new();

    for (dir, source) in skill_search_dirs(agent_id) {
        for skill in scan_skills_dir(&dir, source) {
            if seen.insert(skill.name.clone()) {
                all_skills.push(skill);
            } else {
                debug!(name = %skill.name, "Skill shadowed by higher-priority source");
            }
        }
    }

    for builtin in BUILTIN_SKILLS {
        if seen.contains(builtin.name) {
            continue;
        }
        if let Ok((meta, _body)) = parse_skill_md(builtin.content, SkillSource::BuiltIn, None) {
            seen.insert(meta.name.clone());
            all_skills.push(meta);
        }
    }

    all_skills.retain(|s| !disabled_skills.contains(&s.name));
    all_skills.sort_by(|a, b| a.name.cmp(&b.name));

    SkillCatalog { skills: all_skills }
}

pub fn catalog_skills_for_session(
    agent_id: &str,
    disabled_skills: &[String],
    discovered_path_skills: &HashSet<String>,
    active_skills: &HashSet<String>,
) -> SkillCatalog {
    let catalog = discover_skills(agent_id, disabled_skills);
    let skills = catalog
        .skills
        .into_iter()
        .filter(|skill| {
            skill.paths.is_empty()
                || discovered_path_skills.contains(&skill.name)
                || active_skills.contains(&skill.name)
        })
        .collect();
    SkillCatalog { skills }
}

// ─── Tier 2: Full instruction loading ───────────────────────────────────────

/// Load the full instructions (body) of a skill by name.
pub fn load_skill_instructions(
    agent_id: &str,
    skill_name: &str,
    disabled_skills: &[String],
) -> Result<String, String> {
    load_skill(agent_id, skill_name, disabled_skills).map(|loaded| loaded.instructions)
}

pub fn load_skill(
    agent_id: &str,
    skill_name: &str,
    disabled_skills: &[String],
) -> Result<LoadedSkill, String> {
    if disabled_skills.contains(&skill_name.to_string()) {
        return Err(format!("Skill '{}' is disabled", skill_name));
    }

    for (dir, source) in skill_search_dirs(agent_id) {
        let skill_dir = dir.join(skill_name);
        let skill_md = skill_dir.join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }

        let content = fs::read_to_string(&skill_md)
            .map_err(|e| format!("Failed to read {}: {}", skill_md.display(), e))?;
        let (metadata, body) = parse_skill_md(&content, source, Some(skill_dir.clone()))?;
        return Ok(LoadedSkill {
            metadata,
            instructions: append_skill_resources(&body, &skill_dir),
        });
    }

    for builtin in BUILTIN_SKILLS {
        if builtin.name == skill_name {
            let (metadata, body) = parse_skill_md(builtin.content, SkillSource::BuiltIn, None)?;
            return Ok(LoadedSkill {
                metadata,
                instructions: body,
            });
        }
    }

    Err(format!("Skill '{}' not found", skill_name))
}

fn append_skill_resources(body: &str, skill_dir: &Path) -> String {
    let resources = list_skill_resources(skill_dir);
    if resources.is_empty() {
        return body.to_string();
    }

    let resource_list = resources
        .iter()
        .map(|r| format!("  <file>{}</file>", r))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "{}\n\nSkill directory: {}\nRelative paths in this skill resolve against the skill directory.\n\n<skill-resources>\n{}\n</skill-resources>",
        body,
        skill_dir.display(),
        resource_list
    )
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

// ─── Catalog / active-skill XML builders ───────────────────────────────────

/// Build the XML catalog block for the system prompt (Tier 1 disclosure).
pub fn build_catalog_xml(catalog: &SkillCatalog, active_skill_names: &HashSet<String>) -> String {
    if catalog.skills.is_empty() {
        return String::new();
    }

    let mut ordered: Vec<&SkillMetadata> = catalog.skills.iter().collect();
    ordered.sort_by_key(|skill| catalog_priority(skill, active_skill_names));

    let mut xml = String::from(CATALOG_HEADER);
    let mut included = 0usize;
    let mut omitted = 0usize;

    for skill in ordered {
        let line = format!(
            "  <skill name=\"{}\">{}</skill>\n",
            skill.name,
            truncated_discovery_text(skill)
        );

        if xml.len() + line.len() + CATALOG_FOOTER.len() > MAX_CATALOG_CHARS {
            omitted += 1;
            continue;
        }

        xml.push_str(&line);
        included += 1;
    }

    if included == 0 {
        return String::new();
    }

    if omitted > 0 {
        let note = format!(
            "  <note>{} additional skills omitted to fit the context budget.</note>\n",
            omitted
        );
        if xml.len() + note.len() + CATALOG_FOOTER.len() <= MAX_CATALOG_CHARS {
            xml.push_str(&note);
        }
    }

    xml.push_str(CATALOG_FOOTER);
    xml
}

pub fn build_active_skills_xml(active_skills: &[ActiveSkillRecord]) -> String {
    if active_skills.is_empty() {
        return String::new();
    }

    let mut xml = String::from(
        "\n\n<active-skills>\nThese skills have already been activated in this session. Treat them as loaded and do not call activate_skill for them again unless needed.\n",
    );

    for skill in active_skills {
        xml.push_str(&format!(
            "<skill-instructions name=\"{}\">\n{}\n</skill-instructions>\n",
            skill.skill_name, skill.instructions
        ));
    }

    xml.push_str("</active-skills>");
    xml
}

fn catalog_priority(skill: &SkillMetadata, active_skill_names: &HashSet<String>) -> (u8, String) {
    let is_active = active_skill_names.contains(&skill.name);
    let is_agent_local = skill.source == SkillSource::AgentLocal;
    let priority = match (is_active, is_agent_local) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    };
    (priority, skill.name.clone())
}

fn discovery_text(skill: &SkillMetadata) -> String {
    match skill.when_to_use.as_deref() {
        Some(when) if !when.trim().is_empty() => {
            format!("{} When to use: {}", skill.description, when)
        }
        _ => skill.description.clone(),
    }
}

fn truncated_discovery_text(skill: &SkillMetadata) -> String {
    let text = discovery_text(skill);
    if text.chars().count() <= MAX_LISTING_DESC_CHARS {
        text
    } else {
        let truncated: String = text
            .chars()
            .take(MAX_LISTING_DESC_CHARS.saturating_sub(3))
            .collect();
        format!("{}...", truncated)
    }
}

// ─── Session skill state ────────────────────────────────────────────────────

pub fn upsert_active_skill(
    db: &DbPool,
    session_id: &str,
    skill_name: &str,
    instructions: &str,
    source_path: Option<&Path>,
) -> Result<(), String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO active_session_skills (session_id, skill_name, instructions, source_path, activated_at, tenant_id)
         VALUES (?1, ?2, ?3, ?4, ?5, COALESCE((SELECT tenant_id FROM chat_sessions WHERE id = ?1), 'local'))
         ON CONFLICT(session_id, skill_name) DO UPDATE SET
           instructions = excluded.instructions,
           source_path = excluded.source_path,
           activated_at = excluded.activated_at,
           tenant_id = excluded.tenant_id",
        params![
            session_id,
            skill_name,
            instructions,
            source_path.map(|path| path.to_string_lossy().to_string()),
            Utc::now().to_rfc3339()
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_active_skills_for_session(
    db: &DbPool,
    session_id: &str,
    agent_id: &str,
    disabled_skills: &[String],
) -> Result<Vec<ActiveSkillRecord>, String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT skill_name, instructions, source_path, activated_at
             FROM active_session_skills
             WHERE session_id = ?1
             ORDER BY activated_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows: Vec<ActiveSkillRecord> = stmt
        .query_map(params![session_id], |row| {
            Ok(ActiveSkillRecord {
                skill_name: row.get(0)?,
                instructions: row.get(1)?,
                source_path: row.get(2)?,
                activated_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|row| row.ok())
        .collect();

    let valid_names: HashSet<String> = discover_skills(agent_id, &[])
        .skills
        .into_iter()
        .map(|skill| skill.name)
        .collect();
    let disabled: HashSet<&str> = disabled_skills.iter().map(String::as_str).collect();

    let mut active = Vec::new();
    let mut invalid_names = Vec::new();

    for row in rows {
        if disabled.contains(row.skill_name.as_str()) || !valid_names.contains(&row.skill_name) {
            invalid_names.push(row.skill_name);
        } else {
            active.push(row);
        }
    }

    if !invalid_names.is_empty() {
        delete_session_skill_names(db, session_id, &invalid_names, true, false)?;
    }

    Ok(active)
}

pub fn load_active_skill_names_for_session(
    db: &DbPool,
    session_id: &str,
    agent_id: &str,
    disabled_skills: &[String],
) -> Result<HashSet<String>, String> {
    Ok(
        load_active_skills_for_session(db, session_id, agent_id, disabled_skills)?
            .into_iter()
            .map(|skill| skill.skill_name)
            .collect(),
    )
}

pub fn load_active_skill_names_for_agent(
    db: &DbPool,
    agent_id: &str,
    disabled_skills: &[String],
) -> Result<HashSet<String>, String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT aks.skill_name
             FROM active_session_skills aks
             JOIN chat_sessions cs ON cs.id = aks.session_id
             WHERE cs.agent_id = ?1",
        )
        .map_err(|e| e.to_string())?;

    let names: Vec<String> = stmt
        .query_map(params![agent_id], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|row| row.ok())
        .collect();

    let valid_names: HashSet<String> = discover_skills(agent_id, &[])
        .skills
        .into_iter()
        .map(|skill| skill.name)
        .collect();
    let disabled: HashSet<&str> = disabled_skills.iter().map(String::as_str).collect();

    let mut active = HashSet::new();
    for name in names {
        if disabled.contains(name.as_str()) || !valid_names.contains(&name) {
            clear_skill_state_for_agent_sessions(db, agent_id, &name)?;
        } else {
            active.insert(name);
        }
    }

    Ok(active)
}

pub fn load_discovered_skill_names_for_session(
    db: &DbPool,
    session_id: &str,
    agent_id: &str,
    disabled_skills: &[String],
) -> Result<HashSet<String>, String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT skill_name
             FROM discovered_session_skills
             WHERE session_id = ?1",
        )
        .map_err(|e| e.to_string())?;

    let names: Vec<String> = stmt
        .query_map(params![session_id], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|row| row.ok())
        .collect();

    let valid_path_names: HashSet<String> = discover_skills(agent_id, &[])
        .skills
        .into_iter()
        .filter(|skill| !disabled_skills.contains(&skill.name) && !skill.paths.is_empty())
        .map(|skill| skill.name)
        .collect();

    let mut discovered = HashSet::new();
    let mut invalid_names = Vec::new();
    for name in names {
        if valid_path_names.contains(&name) {
            discovered.insert(name);
        } else {
            invalid_names.push(name);
        }
    }

    if !invalid_names.is_empty() {
        delete_session_skill_names(db, session_id, &invalid_names, false, true)?;
    }

    Ok(discovered)
}

pub fn mark_matching_path_skills_discoverable(
    db: &DbPool,
    session_id: &str,
    agent_id: &str,
    disabled_skills: &[String],
    workspace_root: &Path,
    touched_paths: &[PathBuf],
) -> Result<Vec<String>, String> {
    let already_discovered =
        load_discovered_skill_names_for_session(db, session_id, agent_id, disabled_skills)?;
    let active_names =
        load_active_skill_names_for_session(db, session_id, agent_id, disabled_skills)?;
    let catalog = discover_skills(agent_id, disabled_skills);

    let matched_names: Vec<String> = catalog
        .skills
        .iter()
        .filter(|skill| {
            !skill.paths.is_empty()
                && !already_discovered.contains(&skill.name)
                && !active_names.contains(&skill.name)
        })
        .filter(|skill| skill_matches_any_path(skill, workspace_root, touched_paths))
        .map(|skill| skill.name.clone())
        .collect();

    if matched_names.is_empty() {
        return Ok(Vec::new());
    }

    let conn = db.get().map_err(|e| e.to_string())?;
    let now = Utc::now().to_rfc3339();
    for name in &matched_names {
        conn.execute(
            "INSERT OR IGNORE INTO discovered_session_skills (session_id, skill_name, discovered_at)
             VALUES (?1, ?2, ?3)",
            params![session_id, name, now],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(matched_names)
}

pub fn clear_skill_state_for_agent_sessions(
    db: &DbPool,
    agent_id: &str,
    skill_name: &str,
) -> Result<(), String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM active_session_skills
         WHERE session_id IN (SELECT id FROM chat_sessions WHERE agent_id = ?1)
           AND skill_name = ?2",
        params![agent_id, skill_name],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM discovered_session_skills
         WHERE session_id IN (SELECT id FROM chat_sessions WHERE agent_id = ?1)
           AND skill_name = ?2",
        params![agent_id, skill_name],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn clear_disabled_skill_state_for_agent(
    db: &DbPool,
    agent_id: &str,
    disabled_skills: &[String],
) -> Result<(), String> {
    for skill_name in disabled_skills {
        clear_skill_state_for_agent_sessions(db, agent_id, skill_name)?;
    }
    Ok(())
}

fn delete_session_skill_names(
    db: &DbPool,
    session_id: &str,
    names: &[String],
    delete_active: bool,
    delete_discovered: bool,
) -> Result<(), String> {
    if names.is_empty() {
        return Ok(());
    }

    let conn = db.get().map_err(|e| e.to_string())?;
    for name in names {
        if delete_active {
            conn.execute(
                "DELETE FROM active_session_skills WHERE session_id = ?1 AND skill_name = ?2",
                params![session_id, name],
            )
            .map_err(|e| e.to_string())?;
        }
        if delete_discovered {
            conn.execute(
                "DELETE FROM discovered_session_skills WHERE session_id = ?1 AND skill_name = ?2",
                params![session_id, name],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn skill_matches_any_path(
    skill: &SkillMetadata,
    workspace_root: &Path,
    touched_paths: &[PathBuf],
) -> bool {
    if skill.paths.is_empty() {
        return false;
    }

    let compiled: Vec<_> = skill
        .paths
        .iter()
        .filter_map(|pattern| compile_globs(pattern).ok())
        .flatten()
        .collect();

    touched_paths
        .iter()
        .any(|path| matches_globs(path, workspace_root, &compiled))
}

// ─── Skill creation ─────────────────────────────────────────────────────────

/// Create a new skill in the agent's local skills directory.
pub fn create_skill(
    agent_id: &str,
    name: &str,
    description: &str,
    body: &str,
) -> Result<(), String> {
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

fn extract_keywords(text: &str) -> HashSet<String> {
    let stop: HashSet<&str> = STOP_WORDS.iter().copied().collect();

    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3 && !stop.contains(w))
        .map(|w| w.to_string())
        .collect()
}

fn relevance_score(skill: &SkillMetadata, query_keywords: &HashSet<String>) -> f64 {
    if query_keywords.is_empty() {
        return 0.0;
    }

    let name_keywords = extract_keywords(&skill.name);
    let desc_keywords = extract_keywords(&discovery_text(skill));
    let name_hits = query_keywords.intersection(&name_keywords).count() as f64 * 2.0;
    let desc_hits = query_keywords.intersection(&desc_keywords).count() as f64;
    let total_hits = name_hits + desc_hits;
    let max_possible = query_keywords.len() as f64;

    (total_hits / max_possible).min(1.0)
}

const RELEVANCE_THRESHOLD: f64 = 0.15;

/// Filter a catalog to only include skills relevant to the given context text.
/// Agent-local and explicitly-preserved skills are always included.
pub fn filter_relevant_skills(
    catalog: SkillCatalog,
    context_text: Option<&str>,
    preserved_skill_names: &HashSet<String>,
) -> SkillCatalog {
    let context_text = match context_text {
        Some(t) if !t.trim().is_empty() => t,
        _ => return catalog,
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
            if skill.source == SkillSource::AgentLocal
                || preserved_skill_names.contains(&skill.name)
            {
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

fn skill_search_dirs(agent_id: &str) -> Vec<(PathBuf, SkillSource)> {
    vec![
        (
            workspace::agent_dir(agent_id).join("skills"),
            SkillSource::AgentLocal,
        ),
        (global_skills_dir(), SkillSource::OrbitGlobal),
        (standard_skills_dir(), SkillSource::Standard),
    ]
}

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

#[cfg(test)]
mod tests {
    use super::{
        build_catalog_xml, filter_relevant_skills, load_active_skill_names_for_session,
        load_active_skills_for_session, parse_skill_md, skill_matches_any_path,
        upsert_active_skill, SkillCatalog, SkillMetadata, SkillSource,
    };
    use crate::db::connection::init as init_db;

    #[test]
    fn parse_skill_md_supports_when_to_use_and_paths() {
        let content = r#"---
name: lint-fix
description: Fix lint issues
when-to-use: When the user asks for lint cleanup
paths:
  - src/**/*.rs
  - Cargo.toml
---

Use cargo fmt and cargo clippy.
"#;

        let (meta, body) =
            parse_skill_md(content, SkillSource::Standard, None).expect("skill should parse");

        assert_eq!(
            meta.when_to_use.as_deref(),
            Some("When the user asks for lint cleanup")
        );
        assert_eq!(meta.paths, vec!["src/**/*.rs", "Cargo.toml"]);
        assert_eq!(body, "Use cargo fmt and cargo clippy.");
    }

    #[test]
    fn parse_skill_md_rejects_malformed_paths() {
        let content = r#"---
name: broken
description: Broken skill
paths: [src/**/*.rs
---

Bad
"#;

        let err = parse_skill_md(content, SkillSource::Standard, None).unwrap_err();
        assert!(err.contains("paths"));
    }

    #[test]
    fn build_catalog_xml_prioritizes_active_and_agent_local() {
        let mut skills = Vec::new();
        for i in 0..40 {
            skills.push(SkillMetadata {
                name: format!("skill-{:02}", i),
                description: "A".repeat(400),
                when_to_use: None,
                license: None,
                compatibility: None,
                allowed_tools: None,
                metadata: None,
                paths: Vec::new(),
                source_path: None,
                source: if i == 0 {
                    SkillSource::AgentLocal
                } else {
                    SkillSource::Standard
                },
            });
        }
        let catalog = SkillCatalog { skills };
        let active = std::iter::once("skill-10".to_string()).collect();

        let xml = build_catalog_xml(&catalog, &active);

        assert!(xml.contains(r#"<skill name="skill-00">"#));
        assert!(xml.contains(r#"<skill name="skill-10">"#));
        assert!(xml.len() <= 8_200);
    }

    #[test]
    fn filter_relevant_skills_keeps_preserved_even_when_irrelevant() {
        let catalog = SkillCatalog {
            skills: vec![
                SkillMetadata {
                    name: "code-review".to_string(),
                    description: "Review code".to_string(),
                    when_to_use: None,
                    license: None,
                    compatibility: None,
                    allowed_tools: None,
                    metadata: None,
                    paths: Vec::new(),
                    source_path: None,
                    source: SkillSource::Standard,
                },
                SkillMetadata {
                    name: "terraform".to_string(),
                    description: "Manage terraform infra".to_string(),
                    when_to_use: None,
                    license: None,
                    compatibility: None,
                    allowed_tools: None,
                    metadata: None,
                    paths: Vec::new(),
                    source_path: None,
                    source: SkillSource::Standard,
                },
            ],
        };
        let preserved = std::iter::once("code-review".to_string()).collect();

        let filtered =
            filter_relevant_skills(catalog, Some("deploy kubernetes service"), &preserved);

        assert_eq!(filtered.skills.len(), 1);
        assert_eq!(filtered.skills[0].name, "code-review");
    }

    #[test]
    fn active_skills_round_trip_through_db_state() {
        let dir = std::env::temp_dir().join(format!("orbit-skill-test-{}", ulid::Ulid::new()));
        let db = init_db(dir).expect("db should initialize");
        {
            let conn = db.get().expect("db connection");
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO agents (id, name, description, state, max_concurrent_runs, created_at, updated_at, tenant_id)
                 VALUES (?1, ?2, NULL, 'idle', 1, ?3, ?3, 'local')",
                rusqlite::params!["agent-1", "Agent 1", now],
            )
            .expect("agent fixture should persist");
            conn.execute(
                "INSERT INTO chat_sessions (id, agent_id, title, archived, created_at, updated_at, tenant_id)
                 VALUES (?1, ?2, ?3, 0, ?4, ?4, COALESCE((SELECT tenant_id FROM agents WHERE id = ?2), 'local'))",
                rusqlite::params!["session-1", "agent-1", "Session 1", now],
            )
            .expect("session fixture should persist");
        }

        upsert_active_skill(&db, "session-1", "code-review", "Use review mode", None)
            .expect("active skill should persist");

        let names =
            load_active_skill_names_for_session(&db, "session-1", "agent-1", &[]).expect("names");
        let records =
            load_active_skills_for_session(&db, "session-1", "agent-1", &[]).expect("records");

        assert!(names.contains("code-review"));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].instructions, "Use review mode");
    }

    #[test]
    fn path_scoped_skills_match_workspace_relative_globs() {
        let workspace_root =
            std::env::temp_dir().join(format!("orbit-skill-root-{}", ulid::Ulid::new()));
        let touched_path = workspace_root.join("src/main.rs");
        let skill = SkillMetadata {
            name: "lint-fix".to_string(),
            description: "Fix lint issues".to_string(),
            when_to_use: None,
            license: None,
            compatibility: None,
            allowed_tools: None,
            metadata: None,
            paths: vec!["src/**/*.rs".to_string()],
            source_path: None,
            source: SkillSource::Standard,
        };

        assert!(skill_matches_any_path(
            &skill,
            &workspace_root,
            &[touched_path]
        ));
    }
}
