use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use tracing::{info, warn};

use crate::events::emitter::{emit_permission_cancelled, emit_permission_request};
use crate::executor::agent_tools::{self, ToolExecutionContext};
use crate::executor::global_settings;
use crate::executor::workspace::PermissionRule;

// ─── Risk levels ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum RiskLevel {
    AutoAllow,
    Prompt,
    PromptDangerous,
}

// ─── Permission response ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionResponse {
    Allow,
    AlwaysAllow,
    Deny,
}

// ─── Permission registry ────────────────────────────────────────────────────

struct PendingPermission {
    run_id: String,
    response_tx: oneshot::Sender<PermissionResponse>,
}

/// Global registry of pending permission requests.
/// Agents await a oneshot channel while the UI prompts the user.
#[derive(Clone)]
pub struct PermissionRegistry {
    pending: Arc<Mutex<HashMap<String, PendingPermission>>>,
}

impl PermissionRegistry {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a pending request. Returns a receiver the caller awaits.
    pub async fn register(
        &self,
        request_id: &str,
        run_id: &str,
    ) -> oneshot::Receiver<PermissionResponse> {
        let (tx, rx) = oneshot::channel();
        let mut pending = self.pending.lock().await;
        pending.insert(
            request_id.to_string(),
            PendingPermission {
                run_id: run_id.to_string(),
                response_tx: tx,
            },
        );
        rx
    }

    /// Resolve a pending request with the user's decision.
    pub async fn resolve(
        &self,
        request_id: &str,
        response: PermissionResponse,
    ) -> Result<(), String> {
        let mut pending = self.pending.lock().await;
        match pending.remove(request_id) {
            Some(entry) => {
                let _ = entry.response_tx.send(response);
                Ok(())
            }
            None => Err(format!(
                "No pending permission request with id '{}'",
                request_id
            )),
        }
    }

    /// Cancel all pending requests for a given run_id.
    pub async fn cancel_for_run(&self, run_id: &str, app: &tauri::AppHandle) {
        let mut pending = self.pending.lock().await;
        let to_cancel: Vec<String> = pending
            .iter()
            .filter(|(_, v)| v.run_id == run_id)
            .map(|(k, _)| k.clone())
            .collect();
        for request_id in &to_cancel {
            if let Some(entry) = pending.remove(request_id) {
                emit_permission_cancelled(app, request_id, &entry.run_id);
                // Dropping the sender causes the receiver to get Err, which we handle as cancellation
                drop(entry.response_tx);
            }
        }
    }
}

// ─── Shell command classification ───────────────────────────────────────────

/// Commands that are read-only / informational and safe to auto-allow
/// ONLY when they don't reference paths outside the workspace.
const SAFE_COMMANDS: &[&str] = &[
    "echo", "printf", "wc", "sort", "uniq", "pwd", "date", "which", "type", "true", "false",
    "test", "seq", "basename", "dirname", "tr", "cut", "paste", "column", "jq", "yq",
];

/// Commands that read files/dirs — safe only when args stay within workspace.
/// If they reference absolute paths or sensitive locations, they get escalated.
const SAFE_IF_LOCAL_COMMANDS: &[&str] = &[
    "cat",
    "head",
    "tail",
    "less",
    "more",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "ag",
    "find",
    "fd",
    "ls",
    "ll",
    "la",
    "tree",
    "diff",
    "file",
    "stat",
    "du",
    "df",
    "md5",
    "sha256sum",
    "shasum",
    "xxd",
    "hexdump",
    "strings",
    "bat",
    "exa",
    "eza",
    "lsd",
    "readlink",
    "realpath",
];

/// Commands that leak system identity / environment info — always prompt.
/// These have no legitimate use for a sandboxed agent and are commonly
/// used in reconnaissance or social-engineering attacks.
const SYSTEM_INFO_COMMANDS: &[&str] = &["whoami", "id", "hostname", "uname", "env", "printenv"];

/// Git subcommands that are read-only and safe.
const SAFE_GIT_SUBCOMMANDS: &[&str] = &[
    "status",
    "log",
    "diff",
    "show",
    "branch",
    "remote",
    "describe",
    "rev-parse",
    "config",
    "stash list",
    "shortlog",
    "blame",
    "tag",
];

/// Commands that perform writes, builds, or installs — moderate risk.
const MODERATE_COMMANDS: &[&str] = &[
    "mkdir", "cp", "mv", "touch", "tee", "sed", "awk", "perl", "npm", "npx", "pnpm", "yarn", "bun",
    "pip", "pip3", "pipx", "poetry", "uv", "cargo", "rustup", "make", "cmake", "gradle", "mvn",
    "go", "python", "python3", "node", "ruby", "php", "curl", "wget", "http", "httpie",
    "git", // git with non-safe subcommands
    "docker", "podman", "tar", "zip", "unzip", "gzip", "gunzip",
];

/// Commands that are destructive or affect system state — dangerous.
const DANGEROUS_COMMANDS: &[&str] = &[
    "rm",
    "rmdir",
    "shred",
    "chmod",
    "chown",
    "chgrp",
    "sudo",
    "su",
    "doas",
    "kill",
    "killall",
    "pkill",
    "xkill",
    "shutdown",
    "reboot",
    "halt",
    "poweroff",
    "dd",
    "mkfs",
    "fdisk",
    "diskutil",
    "mount",
    "umount",
    "systemctl",
    "launchctl",
    "service",
    "iptables",
    "ufw",
    "firewall-cmd",
    "useradd",
    "userdel",
    "usermod",
    "passwd",
    "crontab",
];

/// Patterns in a command string that indicate dangerous operations.
const DANGEROUS_PATTERNS: &[&str] = &[
    ">/dev/",  // writing to device files
    "| rm",    // piping to rm
    "| sudo",  // piping to sudo
    "--force", // force flags are often dangerous
    "--hard",  // git reset --hard
];

/// Detect shell command injection patterns in a command string.
/// Returns Some(reason) if injection patterns are found.
fn detect_command_injection(command: &str) -> Option<String> {
    let trimmed = command.trim();

    // Check for $(...) command substitution
    // We look for $( not preceded by a backslash and not inside single quotes
    if contains_unquoted_pattern(trimmed, "$(") {
        return Some("Command substitution detected: '$(...)'".to_string());
    }

    // Check for backtick command substitution
    if contains_unquoted_pattern(trimmed, "`") {
        return Some("Backtick command substitution detected".to_string());
    }

    // Check for eval
    let base = trimmed.split_whitespace().next().unwrap_or("");
    if base == "eval" {
        return Some("'eval' can execute arbitrary code".to_string());
    }

    // Check for process substitution <(...) and >(...)
    if contains_unquoted_pattern(trimmed, "<(") || contains_unquoted_pattern(trimmed, ">(") {
        return Some("Process substitution detected".to_string());
    }

    // Check for base64 decode piped to execution (common obfuscation)
    let lower = trimmed.to_lowercase();
    if (lower.contains("base64") && lower.contains("decode"))
        || (lower.contains("base64") && lower.contains("-d"))
    {
        if lower.contains("|") || lower.contains("xargs") || lower.contains("eval") {
            return Some("Base64 decode piped to execution (potential obfuscation)".to_string());
        }
    }

    // Check for hex/octal escape sequences used for obfuscation
    if trimmed.contains("\\x") || trimmed.contains("$'\\x") || trimmed.contains("$'\\0") {
        return Some("Hex/octal escape sequences detected (potential obfuscation)".to_string());
    }

    None
}

/// Check if a pattern appears outside of single-quoted strings.
/// Single-quoted strings in shell prevent all interpretation, so patterns
/// inside them are safe data, not injection vectors.
fn contains_unquoted_pattern(command: &str, pattern: &str) -> bool {
    let mut in_single_quote = false;
    let chars: Vec<char> = command.chars().collect();
    let pattern_chars: Vec<char> = pattern.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\'' && (i == 0 || chars[i - 1] != '\\') {
            in_single_quote = !in_single_quote;
            i += 1;
            continue;
        }

        if !in_single_quote && i + pattern_chars.len() <= chars.len() {
            let slice: String = chars[i..i + pattern_chars.len()].iter().collect();
            if slice == pattern.to_string() {
                return true;
            }
        }

        i += 1;
    }

    false
}

/// Path prefixes that indicate sensitive system locations.
/// Any command referencing these should never auto-allow.
const SENSITIVE_PATH_PREFIXES: &[&str] = &[
    "/etc/",
    "/etc ",
    "/var/",
    "/var ",
    "/proc/",
    "/proc ",
    "/sys/",
    "/sys ",
    "/dev/",
    "/dev ",
    "/private/etc/", // macOS
    "/Library/",     // macOS system
    "/System/",      // macOS system
    "~/.ssh",
    "$HOME/.ssh",
    "~/.gnupg",
    "$HOME/.gnupg",
    "~/.aws",
    "$HOME/.aws",
    "~/.config",
    "$HOME/.config",
    "~/.kube",
    "$HOME/.kube",
    "~/.docker",
    "$HOME/.docker",
    "/root/",
    "/tmp/",
    "/tmp ",
];

/// Specific sensitive filenames that should never auto-allow regardless of path.
const SENSITIVE_FILENAMES: &[&str] = &[
    "passwd",
    "shadow",
    "sudoers",
    "hosts",
    ".env",
    ".env.local",
    ".env.production",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "authorized_keys",
    "known_hosts",
    "credentials",
    "credentials.json",
    "token",
    "token.json",
    ".netrc",
    ".pgpass",
    ".my.cnf",
    "keychain",
    "keyring",
];

/// Check if a command string references paths outside the workspace or sensitive locations.
fn references_sensitive_paths(command: &str) -> Option<String> {
    let lower = command.to_lowercase();

    // Check for sensitive path prefixes
    for prefix in SENSITIVE_PATH_PREFIXES {
        if lower.contains(prefix) {
            return Some(format!("References sensitive path: '{}'", prefix.trim()));
        }
    }

    // Check for sensitive filenames anywhere in the command
    for filename in SENSITIVE_FILENAMES {
        if lower.contains(filename) {
            return Some(format!("References sensitive file: '{}'", filename));
        }
    }

    // Check for absolute paths (starting with /) in arguments
    // Skip the command name itself (first word)
    let args = command.split_whitespace().skip(1);
    for arg in args {
        // Skip flags
        if arg.starts_with('-') {
            continue;
        }
        if arg.starts_with('/') {
            return Some(format!("References absolute path: '{}'", arg));
        }
        if arg.starts_with("~/") || arg.starts_with("$HOME") {
            return Some(format!("References home directory path: '{}'", arg));
        }
    }

    None
}

/// Classify a shell command string into a risk level.
fn classify_shell_command(command: &str) -> (RiskLevel, String) {
    let trimmed = command.trim();

    // ── First pass: check the ENTIRE command for sensitive path references ──
    // This catches cases like `cat /etc/passwd` where `cat` is otherwise "safe".
    if let Some(reason) = references_sensitive_paths(trimmed) {
        return (RiskLevel::PromptDangerous, reason);
    }

    // ── Check for command injection patterns ──
    if let Some(reason) = detect_command_injection(trimmed) {
        return (RiskLevel::PromptDangerous, reason);
    }

    // Check for dangerous patterns
    for pattern in DANGEROUS_PATTERNS {
        if trimmed.contains(pattern) {
            return (
                RiskLevel::PromptDangerous,
                format!("Dangerous pattern detected: '{}'", pattern),
            );
        }
    }

    // Check for output redirection (overwrite)
    if trimmed.contains(" > ") || trimmed.starts_with("> ") {
        // But not >> (append) or 2> (stderr redirect) alone
        let has_overwrite = trimmed
            .split(">>")
            .any(|part| part.contains(" > ") || part.starts_with("> "));
        if has_overwrite {
            return (
                RiskLevel::PromptDangerous,
                "Output redirection (file overwrite) detected".to_string(),
            );
        }
    }

    // Split on command separators to analyze each sub-command
    let separators = [" && ", " || ", " ; ", " | "];
    let mut segments = vec![trimmed.to_string()];
    for sep in &separators {
        let mut new_segments = Vec::new();
        for segment in &segments {
            for part in segment.split(sep) {
                let part = part.trim();
                if !part.is_empty() {
                    new_segments.push(part.to_string());
                }
            }
        }
        segments = new_segments;
    }

    let mut worst_risk = RiskLevel::AutoAllow;
    let mut descriptions = Vec::new();

    for segment in &segments {
        let (risk, desc) = classify_single_command(segment);
        if risk == RiskLevel::PromptDangerous {
            worst_risk = RiskLevel::PromptDangerous;
            descriptions.push(desc);
        } else if risk == RiskLevel::Prompt && worst_risk == RiskLevel::AutoAllow {
            worst_risk = RiskLevel::Prompt;
            descriptions.push(desc);
        }
    }

    if worst_risk == RiskLevel::AutoAllow {
        (RiskLevel::AutoAllow, "Safe read-only command".to_string())
    } else {
        (worst_risk, descriptions.join("; "))
    }
}

/// Classify a single command (no separators).
fn classify_single_command(command: &str) -> (RiskLevel, String) {
    let trimmed = command.trim();

    // Extract the base command (first word, stripping path prefix)
    let base = trimmed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");

    if base.is_empty() {
        return (RiskLevel::AutoAllow, String::new());
    }

    // Check dangerous first
    if DANGEROUS_COMMANDS.contains(&base) {
        return (
            RiskLevel::PromptDangerous,
            format!("Dangerous command: '{}'", base),
        );
    }

    // Special git handling: check subcommand
    if base == "git" {
        let rest = trimmed.strip_prefix("git").unwrap_or("").trim();
        // Check if it's a safe git subcommand
        for safe_sub in SAFE_GIT_SUBCOMMANDS {
            if rest.starts_with(safe_sub) {
                return (
                    RiskLevel::AutoAllow,
                    format!("Safe git subcommand: 'git {}'", safe_sub),
                );
            }
        }
        // Check for dangerous git operations
        if rest.starts_with("push --force") || rest.starts_with("push -f") {
            return (
                RiskLevel::PromptDangerous,
                "Dangerous: 'git push --force'".to_string(),
            );
        }
        if rest.starts_with("reset --hard") {
            return (
                RiskLevel::PromptDangerous,
                "Dangerous: 'git reset --hard'".to_string(),
            );
        }
        if rest.starts_with("clean") {
            return (
                RiskLevel::PromptDangerous,
                "Dangerous: 'git clean'".to_string(),
            );
        }
        // Other git commands are moderate
        return (
            RiskLevel::Prompt,
            format!(
                "Write operation: 'git {}'",
                rest.split_whitespace().next().unwrap_or("")
            ),
        );
    }

    // System info commands always prompt — no legitimate use in a sandboxed workspace
    if SYSTEM_INFO_COMMANDS.contains(&base) {
        return (
            RiskLevel::Prompt,
            format!("System info command: '{}' (leaks host identity)", base),
        );
    }

    // Check safe commands (no file arguments, pure data processing)
    if SAFE_COMMANDS.contains(&base) {
        return (RiskLevel::AutoAllow, String::new());
    }

    // Commands that are safe ONLY when operating on local/relative paths
    if SAFE_IF_LOCAL_COMMANDS.contains(&base) {
        // Already passed the sensitive-path check in classify_shell_command,
        // so if we get here, args are workspace-relative. Auto-allow.
        return (RiskLevel::AutoAllow, String::new());
    }

    // Check moderate commands
    if MODERATE_COMMANDS.contains(&base) {
        return (
            RiskLevel::Prompt,
            format!("Write/exec operation: '{}'", base),
        );
    }

    // Unknown commands default to moderate (prompt)
    (RiskLevel::Prompt, format!("Unknown command: '{}'", base))
}

// ─── Tool call classification ───────────────────────────────────────────────

/// Classify a tool call into a risk level based on the tool name, input, and permission mode.
pub fn classify_tool_call(
    tool_name: &str,
    input: &serde_json::Value,
    permission_mode: &str,
) -> (RiskLevel, String) {
    // In permissive mode, everything auto-allows
    if permission_mode == "permissive" {
        return (RiskLevel::AutoAllow, String::new());
    }

    match tool_name {
        // Always auto-allow: read-only and safe tools
        "read_file" | "list_files" | "grep" | "web_search" | "web_fetch" | "image_analysis"
        | "session_history" | "session_status" | "sessions_list" | "sessions_spawn"
        | "ask_user" | "activate_skill" | "finish" | "spawn_sub_agents" | "yield_turn"
        | "react_to_message" => (RiskLevel::AutoAllow, String::new()),

        "image_generation" => (
            RiskLevel::Prompt,
            "Generate image (uses API credits)".to_string(),
        ),

        "config" => match input["action"].as_str().unwrap_or("list") {
            "get" | "list" | "info" => (RiskLevel::AutoAllow, String::new()),
            "set" => {
                let setting = input["setting"].as_str().unwrap_or("<unknown>");
                (RiskLevel::Prompt, format!("Update config: '{}'", setting))
            }
            action => (RiskLevel::Prompt, format!("Config action: {}", action)),
        },

        "task" => (RiskLevel::AutoAllow, String::new()),

        // work_item writes the local DB and is scoped to the agent's assigned
        // projects via assert_agent_in_project, so there's no confused-deputy
        // risk. Classified safe — no user prompt.
        "work_item" => (RiskLevel::AutoAllow, String::new()),

        "schedule" => match input["action"].as_str().unwrap_or("list") {
            "list" | "preview" | "pulse_get" => (RiskLevel::AutoAllow, String::new()),
            action => (RiskLevel::Prompt, format!("Schedule action: {}", action)),
        },

        "worktree" => match input["action"].as_str().unwrap_or("list") {
            "list" => (RiskLevel::AutoAllow, String::new()),
            "create" => (RiskLevel::Prompt, "Create git worktree".to_string()),
            "exit" => {
                if input["keep_changes"].as_bool().unwrap_or(true) {
                    (RiskLevel::AutoAllow, String::new())
                } else {
                    (
                        RiskLevel::Prompt,
                        "Remove worktree and discard changes".to_string(),
                    )
                }
            }
            action => (RiskLevel::Prompt, format!("Worktree action: {}", action)),
        },

        "shell_command" => {
            if let Some(action) = input["process_action"].as_str() {
                return match action {
                    "list" | "poll" => (RiskLevel::AutoAllow, String::new()),
                    "kill" => (RiskLevel::Prompt, "Kill background process".to_string()),
                    _ => (RiskLevel::Prompt, format!("Process action: {}", action)),
                };
            }

            let command = input["command"].as_str().unwrap_or("");
            if permission_mode == "strict" {
                return (
                    RiskLevel::Prompt,
                    format!(
                        "Strict mode: shell_command '{}'",
                        truncate_for_display(command, 60)
                    ),
                );
            }
            classify_shell_command(command)
        }

        "write_file" => {
            let path = input["path"].as_str().unwrap_or("<unknown>");
            if permission_mode == "strict" {
                return (
                    RiskLevel::Prompt,
                    format!("Strict mode: write_file '{}'", path),
                );
            }
            (RiskLevel::Prompt, format!("File write: '{}'", path))
        }

        "edit_file" => {
            let path = input["path"].as_str().unwrap_or("<unknown>");
            if permission_mode == "strict" {
                return (
                    RiskLevel::Prompt,
                    format!("Strict mode: edit_file '{}'", path),
                );
            }
            (RiskLevel::Prompt, format!("File edit: '{}'", path))
        }

        "send_message" => {
            let target = input["target_agent"].as_str().unwrap_or("<unknown>");
            (RiskLevel::Prompt, format!("Agent message to '{}'", target))
        }

        "message" => {
            let action = input["action"].as_str().unwrap_or("send");
            match action {
                "list" => (RiskLevel::AutoAllow, String::new()),
                "send" => {
                    let channel = input["channel"].as_str().unwrap_or("<unknown>");
                    (
                        RiskLevel::PromptDangerous,
                        format!("Send external message to channel '{}'", channel),
                    )
                }
                other => (RiskLevel::Prompt, format!("Message action: {}", other)),
            }
        }

        "session_send" => {
            let session_id = input["session_id"].as_str().unwrap_or("<unknown>");
            (
                RiskLevel::Prompt,
                format!(
                    "Send to existing session '{}'",
                    truncate_for_display(session_id, 20)
                ),
            )
        }

        "subagents" => match input["action"].as_str().unwrap_or("list") {
            "list" | "steer" => (RiskLevel::AutoAllow, String::new()),
            "kill" => (RiskLevel::Prompt, "Kill sub-agent".to_string()),
            action => (RiskLevel::Prompt, format!("Subagent action: {}", action)),
        },

        "plugin_management" => match input["action"].as_str().unwrap_or("list") {
            "list" | "status" | "logs" | "oauth_status" => (RiskLevel::AutoAllow, String::new()),
            action => (
                RiskLevel::Prompt,
                format!("Plugin management action: {}", action),
            ),
        },

        name if name.contains("__") => {
            // Plugin-contributed tool names are namespaced `<slug>__<name>`.
            // Never auto-allow in V1 — the user sees a permission prompt on
            // every plugin tool call and can grant an "always" rule.
            (RiskLevel::Prompt, format!("Plugin tool: '{}'", tool_name))
        }

        _ => {
            // Unknown tools default to prompt in strict, auto-allow in normal
            if permission_mode == "strict" {
                (
                    RiskLevel::Prompt,
                    format!("Strict mode: unknown tool '{}'", tool_name),
                )
            } else {
                (RiskLevel::AutoAllow, String::new())
            }
        }
    }
}

// ─── Rule matching ──────────────────────────────────────────────────────────

/// Find the first matching permission rule for a tool call.
pub fn find_matching_rule<'a>(
    rules: &'a [PermissionRule],
    tool_name: &str,
    input: &serde_json::Value,
) -> Option<&'a PermissionRule> {
    let match_value = extract_match_value(tool_name, input);
    rules
        .iter()
        .find(|rule| rule.tool == tool_name && glob_match(&rule.pattern, &match_value))
}

/// Extract the value to match against from the tool input.
fn extract_match_value(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "shell_command" => {
            if let Some(action) = input["process_action"].as_str() {
                format!("process_action:{}", action)
            } else {
                input["command"].as_str().unwrap_or("").to_string()
            }
        }
        "write_file" => input["path"].as_str().unwrap_or("").to_string(),
        "edit_file" => input["path"].as_str().unwrap_or("").to_string(),
        "send_message" => input["target_agent"].as_str().unwrap_or("").to_string(),
        "message" => {
            let action = input["action"].as_str().unwrap_or("send");
            let channel = input["channel"].as_str().unwrap_or("");
            format!("{}:{}", action, channel)
        }
        "session_send" => input["session_id"].as_str().unwrap_or("").to_string(),
        "subagents" => {
            let action = input["action"].as_str().unwrap_or("list");
            let session_id = input["session_id"].as_str().unwrap_or("");
            format!("{}:{}", action, session_id)
        }
        _ => "*".to_string(),
    }
}

/// Simple glob matching: supports `*` as wildcard.
/// "echo *" matches "echo hello world"
/// "*.rs" matches "main.rs"
/// "*" matches everything
fn glob_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    // Simple star-matching: split pattern on `*` and check ordered containment
    let parts: Vec<&str> = pattern.split('*').collect();

    if parts.len() == 1 {
        // No wildcards — exact match
        return pattern == value;
    }

    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            // First part must match at the start
            if !value.starts_with(part) {
                return false;
            }
            pos = part.len();
        } else if i == parts.len() - 1 {
            // Last part must match at the end
            if !value[pos..].ends_with(part) {
                return false;
            }
        } else {
            // Middle parts must appear in order
            match value[pos..].find(part) {
                Some(idx) => pos += idx + part.len(),
                None => return false,
            }
        }
    }

    true
}

/// Generate the "Always Allow" pattern for a tool call.
pub fn generate_always_allow_pattern(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "shell_command" => {
            if let Some(action) = input["process_action"].as_str() {
                return format!("process_action:{}", action);
            }
            let command = input["command"].as_str().unwrap_or("");
            // Extract the base command and use "base_command *"
            let base = command
                .trim()
                .split_whitespace()
                .next()
                .unwrap_or("")
                .rsplit('/')
                .next()
                .unwrap_or("");
            if base.is_empty() {
                "*".to_string()
            } else if base == "git" {
                // For git, include the subcommand
                let parts: Vec<&str> = command.trim().split_whitespace().take(2).collect();
                format!("{} *", parts.join(" "))
            } else {
                format!("{} *", base)
            }
        }
        "write_file" => {
            let path = input["path"].as_str().unwrap_or("");
            // Use file extension pattern
            if let Some(ext) = path.rsplit('.').next() {
                if ext != path {
                    return format!("*.{}", ext);
                }
            }
            path.to_string()
        }
        "edit_file" => {
            let path = input["path"].as_str().unwrap_or("");
            if let Some(ext) = path.rsplit('.').next() {
                if ext != path {
                    return format!("*.{}", ext);
                }
            }
            path.to_string()
        }
        "send_message" => input["target_agent"].as_str().unwrap_or("*").to_string(),
        "session_send" => input["session_id"].as_str().unwrap_or("*").to_string(),
        "subagents" => {
            let action = input["action"].as_str().unwrap_or("list");
            let session_id = input["session_id"].as_str().unwrap_or("*");
            format!("{}:{}", action, session_id)
        }
        _ => "*".to_string(),
    }
}

fn truncate_for_display(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

// ─── Permission-gated tool execution ────────────────────────────────────────

/// Wrapper around execute_tool that checks permissions before executing.
/// Pauses execution and prompts the user when the tool call requires approval.
pub async fn execute_tool_with_permissions(
    ctx: &ToolExecutionContext,
    tool_name: &str,
    input: &serde_json::Value,
    app: &tauri::AppHandle,
    run_id: &str,
    registry: &PermissionRegistry,
) -> Result<(String, bool), String> {
    // Load fresh global settings (so mid-run "Always Allow" saves take effect).
    // Permission mode and rules now live in the global settings file, not in
    // per-agent config.
    let global = global_settings::load_global_settings();
    let permission_mode = global.agent_defaults.permission_mode.as_str();
    let permission_rules = &global.agent_defaults.permission_rules;

    // For `message.send`, resolve the target channel to a stable id BEFORE
    // classification so permission rules match the same identity whether the
    // agent sent with an explicit `channel` or via its default_channel_id.
    let mut owned_input: Option<serde_json::Value> = None;
    if tool_name == "message" && input["action"].as_str().unwrap_or("send") == "send" {
        let raw_channel = input["channel"].as_str();
        let resolved_id = match raw_channel {
            Some(raw) if !raw.is_empty() => {
                global_settings::find_channel_in_global(raw).map(|c| c.id)
            }
            _ => {
                // Fall back to the agent's default outbound channel.
                let ws_config = crate::executor::workspace::load_agent_config(&ctx.agent_id)
                    .unwrap_or_default();
                ws_config
                    .default_channel_id
                    .and_then(|id| global_settings::find_channel_by_id(&id).map(|c| c.id))
            }
        };
        if let Some(id) = resolved_id {
            let mut clone = input.clone();
            if let Some(obj) = clone.as_object_mut() {
                obj.insert("channel".to_string(), serde_json::Value::String(id));
            }
            owned_input = Some(clone);
        }
    }
    let input: &serde_json::Value = owned_input.as_ref().unwrap_or(input);

    // 1. Classify the tool call
    let (risk, description) = classify_tool_call(tool_name, input, permission_mode);

    // 2. Auto-allow safe operations
    if risk == RiskLevel::AutoAllow {
        return agent_tools::execute_tool(ctx, tool_name, input, app, run_id).await;
    }

    // 3. Check saved permission rules
    if let Some(rule) = find_matching_rule(permission_rules, tool_name, input) {
        if rule.decision == "allow" {
            info!(
                run_id = run_id,
                tool = tool_name,
                pattern = %rule.pattern,
                "Permission auto-granted by saved rule"
            );
            return agent_tools::execute_tool(ctx, tool_name, input, app, run_id).await;
        } else {
            return Ok((
                format!(
                    "Permission denied by saved rule: {}",
                    rule.description.as_deref().unwrap_or("denied")
                ),
                false,
            ));
        }
    }

    // 4. Must prompt user — emit permission request event
    let request_id = ulid::Ulid::new().to_string();
    let suggested_pattern = generate_always_allow_pattern(tool_name, input);
    let session_id = ctx.current_session_id.clone();

    info!(
        run_id = run_id,
        tool = tool_name,
        request_id = %request_id,
        risk = ?risk,
        "Permission prompt required"
    );

    emit_permission_request(
        app,
        &request_id,
        run_id,
        session_id.as_deref(),
        &ctx.agent_id,
        tool_name,
        input,
        match risk {
            RiskLevel::PromptDangerous => "dangerous",
            _ => "moderate",
        },
        &description,
        &suggested_pattern,
    );

    // 5. Register and await user response
    let rx = registry.register(&request_id, run_id).await;
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(300), // 5 minute timeout
        rx,
    )
    .await
    .map_err(|_| {
        warn!(request_id = %request_id, "Permission request timed out after 5 minutes");
        "Permission request timed out after 5 minutes. The tool call was not executed.".to_string()
    })?
    .map_err(|_| "Permission request cancelled".to_string())?;

    // 6. Execute based on response
    match response {
        PermissionResponse::Allow | PermissionResponse::AlwaysAllow => {
            // For AlwaysAllow, the frontend handles saving the rule via save_permission_rule command
            agent_tools::execute_tool(ctx, tool_name, input, app, run_id).await
        }
        PermissionResponse::Deny => Ok((
            "Permission denied by user. The tool call was not executed.".to_string(),
            false,
        )),
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_commands_auto_allow() {
        let (risk, _) = classify_shell_command("echo hello world");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let (risk, _) = classify_shell_command("ls -la");
        assert_eq!(risk, RiskLevel::AutoAllow);

        // cat with workspace-relative path is safe
        let (risk, _) = classify_shell_command("cat README.md");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let (risk, _) = classify_shell_command("git status");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let (risk, _) = classify_shell_command("git log --oneline");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let (risk, _) = classify_shell_command("grep -r TODO src/");
        assert_eq!(risk, RiskLevel::AutoAllow);
    }

    #[test]
    fn system_info_commands_prompt() {
        // System identity commands should always prompt
        let (risk, _) = classify_shell_command("whoami");
        assert_eq!(risk, RiskLevel::Prompt);

        let (risk, _) = classify_shell_command("id");
        assert_eq!(risk, RiskLevel::Prompt);

        let (risk, _) = classify_shell_command("hostname");
        assert_eq!(risk, RiskLevel::Prompt);

        let (risk, _) = classify_shell_command("env");
        assert_eq!(risk, RiskLevel::Prompt);
    }

    #[test]
    fn sensitive_paths_flagged_dangerous() {
        // cat /etc/passwd — cat is normally safe but /etc/ is sensitive
        let (risk, _) = classify_shell_command("cat /etc/passwd");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        // Reading SSH keys
        let (risk, _) = classify_shell_command("cat ~/.ssh/id_rsa");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        // Listing /var
        let (risk, _) = classify_shell_command("ls /var/log/");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        // Even grep on sensitive paths
        let (risk, _) = classify_shell_command("grep root /etc/passwd");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        // Env files
        let (risk, _) = classify_shell_command("cat .env");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        // Absolute paths outside workspace
        let (risk, _) = classify_shell_command("cat /Users/someone/secrets.txt");
        assert_eq!(risk, RiskLevel::PromptDangerous);
    }

    #[test]
    fn chained_with_sensitive_paths() {
        // The whole command should be flagged because of /etc/passwd
        let (risk, _) = classify_shell_command("whoami && id && cat /etc/passwd");
        assert_eq!(risk, RiskLevel::PromptDangerous);
    }

    #[test]
    fn moderate_commands_prompt() {
        let (risk, _) = classify_shell_command("npm install express");
        assert_eq!(risk, RiskLevel::Prompt);

        let (risk, _) = classify_shell_command("cargo build");
        assert_eq!(risk, RiskLevel::Prompt);

        let (risk, _) = classify_shell_command("python script.py");
        assert_eq!(risk, RiskLevel::Prompt);

        let (risk, _) = classify_shell_command("git add .");
        assert_eq!(risk, RiskLevel::Prompt);

        let (risk, _) = classify_shell_command("curl https://example.com");
        assert_eq!(risk, RiskLevel::Prompt);
    }

    #[test]
    fn dangerous_commands_prompt_dangerous() {
        let (risk, _) = classify_shell_command("rm -rf .");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        let (risk, _) = classify_shell_command("sudo apt install foo");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        let (risk, _) = classify_shell_command("kill -9 1234");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        let (risk, _) = classify_shell_command("chmod 777 /tmp/file");
        assert_eq!(risk, RiskLevel::PromptDangerous);
    }

    #[test]
    fn dangerous_git_commands() {
        let (risk, _) = classify_shell_command("git push --force origin main");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        let (risk, _) = classify_shell_command("git reset --hard HEAD~1");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        let (risk, _) = classify_shell_command("git clean -fd");
        assert_eq!(risk, RiskLevel::PromptDangerous);
    }

    #[test]
    fn chained_commands_worst_case() {
        // Safe && dangerous = dangerous
        let (risk, _) = classify_shell_command("echo hello && rm -rf /tmp/foo");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        // Safe && safe = safe
        let (risk, _) = classify_shell_command("echo hello && ls -la");
        assert_eq!(risk, RiskLevel::AutoAllow);
    }

    #[test]
    fn redirect_detection() {
        let (risk, _) = classify_shell_command("echo hello > output.txt");
        assert_eq!(risk, RiskLevel::PromptDangerous);
    }

    #[test]
    fn glob_matching_works() {
        assert!(glob_match("echo *", "echo hello world"));
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("git commit *", "git commit -m 'fix bug'"));
        assert!(!glob_match("echo *", "ls -la"));
        assert!(!glob_match("*.rs", "main.py"));
        assert!(glob_match("echo", "echo")); // exact match
        assert!(!glob_match("echo", "echo hello")); // no wildcard = exact
    }

    #[test]
    fn pattern_generation() {
        let input = serde_json::json!({"command": "echo hello world"});
        assert_eq!(
            generate_always_allow_pattern("shell_command", &input),
            "echo *"
        );

        let input = serde_json::json!({"command": "git commit -m 'fix'"});
        assert_eq!(
            generate_always_allow_pattern("shell_command", &input),
            "git commit *"
        );

        let input = serde_json::json!({"path": "src/main.rs"});
        assert_eq!(generate_always_allow_pattern("write_file", &input), "*.rs");

        let input = serde_json::json!({"target_agent": "research-agent"});
        assert_eq!(
            generate_always_allow_pattern("send_message", &input),
            "research-agent"
        );
    }

    #[test]
    fn command_injection_detection() {
        // $(...) substitution
        let (risk, _) = classify_shell_command("echo $(cat /etc/passwd)");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        // Backtick substitution
        let (risk, _) = classify_shell_command("echo `whoami`");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        // eval
        let (risk, _) = classify_shell_command("eval 'rm -rf /'");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        // Process substitution
        let (risk, _) = classify_shell_command("diff <(cat file1) <(cat file2)");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        // Base64 obfuscation piped to execution
        let (risk, _) = classify_shell_command("echo cm0gLXJmIC8= | base64 --decode | sh");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        // Hex escape obfuscation
        let (risk, _) = classify_shell_command("printf '\\x72\\x6d' | sh");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        // Safe: $(...) inside single quotes is data, not injection
        let (risk, _) = classify_shell_command("echo '$(not executed)'");
        assert_eq!(risk, RiskLevel::AutoAllow);

        // Safe: backtick inside single quotes
        let (risk, _) = classify_shell_command("echo '`not executed`'");
        assert_eq!(risk, RiskLevel::AutoAllow);
    }

    #[test]
    fn tool_classification() {
        let input = serde_json::json!({});

        // Auto-allow tools
        let (risk, _) = classify_tool_call("read_file", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let (risk, _) = classify_tool_call("list_files", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let (risk, _) = classify_tool_call("grep", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let (risk, _) = classify_tool_call("web_search", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let (risk, _) = classify_tool_call("web_fetch", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let (risk, _) = classify_tool_call("image_analysis", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let (risk, _) = classify_tool_call("image_generation", &input, "normal");
        assert_eq!(risk, RiskLevel::Prompt);

        let (risk, _) = classify_tool_call("session_history", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let (risk, _) = classify_tool_call("session_status", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let input = serde_json::json!({"action": "list"});
        let (risk, _) = classify_tool_call("config", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let input = serde_json::json!({"action": "set", "setting": "temperature", "value": 0.5});
        let (risk, _) = classify_tool_call("config", &input, "normal");
        assert_eq!(risk, RiskLevel::Prompt);

        let input = serde_json::json!({"action": "list"});
        let (risk, _) = classify_tool_call("task", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let input = serde_json::json!({"action": "preview"});
        let (risk, _) = classify_tool_call("schedule", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let input = serde_json::json!({"action": "create"});
        let (risk, _) = classify_tool_call("schedule", &input, "normal");
        assert_eq!(risk, RiskLevel::Prompt);

        let input = serde_json::json!({"action": "list"});
        let (risk, _) = classify_tool_call("worktree", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let input = serde_json::json!({"action": "create"});
        let (risk, _) = classify_tool_call("worktree", &input, "normal");
        assert_eq!(risk, RiskLevel::Prompt);

        let (risk, _) = classify_tool_call("sessions_list", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let (risk, _) = classify_tool_call("finish", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        // Prompt tools
        let input = serde_json::json!({"path": "test.txt"});
        let (risk, _) = classify_tool_call("write_file", &input, "normal");
        assert_eq!(risk, RiskLevel::Prompt);

        let (risk, _) = classify_tool_call("edit_file", &input, "normal");
        assert_eq!(risk, RiskLevel::Prompt);

        let input = serde_json::json!({"target_agent": "other"});
        let (risk, _) = classify_tool_call("send_message", &input, "normal");
        assert_eq!(risk, RiskLevel::Prompt);

        // message tool: list is auto-allow, send is dangerous
        let input = serde_json::json!({"action": "list"});
        let (risk, _) = classify_tool_call("message", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let input = serde_json::json!({"action": "send", "channel": "ops", "text": "hi"});
        let (risk, _) = classify_tool_call("message", &input, "normal");
        assert_eq!(risk, RiskLevel::PromptDangerous);

        let input = serde_json::json!({"process_action": "list"});
        let (risk, _) = classify_tool_call("shell_command", &input, "normal");
        assert_eq!(risk, RiskLevel::AutoAllow);

        let input = serde_json::json!({"process_action": "kill", "process_id": "abc"});
        let (risk, _) = classify_tool_call("shell_command", &input, "normal");
        assert_eq!(risk, RiskLevel::Prompt);

        // Permissive mode auto-allows everything
        let input = serde_json::json!({"command": "rm -rf /"});
        let (risk, _) = classify_tool_call("shell_command", &input, "permissive");
        assert_eq!(risk, RiskLevel::AutoAllow);
    }
}
