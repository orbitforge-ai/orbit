use std::process::Command;
use tracing::{info, warn};

use crate::executor::llm_provider::is_cli_provider;

const SERVICE_NAME: &str = "com.orbit.api-keys";

/// Store an API key in the macOS Keychain.
/// Uses `-U` to update if an entry already exists.
pub fn store_api_key(provider: &str, key: &str) -> Result<(), String> {
    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            SERVICE_NAME,
            "-a",
            provider,
            "-w",
            key,
        ])
        .output()
        .map_err(|e| format!("failed to run security command: {}", e))?;

    if output.status.success() {
        info!(provider = provider, "API key stored in Keychain");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("failed to store API key: {}", stderr.trim()))
    }
}

/// Retrieve an API key from the macOS Keychain.
///
/// CLI-backed providers (claude-cli, codex-cli) do not use API keys — the
/// local CLI handles its own authentication. Return an empty sentinel for
/// those so runtime code can still call `retrieve_api_key` uniformly.
pub fn retrieve_api_key(provider: &str) -> Result<String, String> {
    if is_cli_provider(provider) {
        return Ok(String::new());
    }
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            SERVICE_NAME,
            "-a",
            provider,
            "-w",
        ])
        .output()
        .map_err(|e| format!("failed to run security command: {}", e))?;

    if output.status.success() {
        let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(key)
    } else {
        Err(format!("no API key found for provider: {}", provider))
    }
}

/// Delete an API key from the macOS Keychain.
pub fn delete_api_key(provider: &str) -> Result<(), String> {
    let output = Command::new("security")
        .args([
            "delete-generic-password",
            "-s",
            SERVICE_NAME,
            "-a",
            provider,
        ])
        .output()
        .map_err(|e| format!("failed to run security command: {}", e))?;

    if output.status.success() {
        info!(provider = provider, "API key deleted from Keychain");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(provider = provider, err = %stderr.trim(), "failed to delete API key");
        Err(format!("failed to delete API key: {}", stderr.trim()))
    }
}

/// Check whether an API key exists in the Keychain for a given provider.
///
/// CLI providers do not require a stored key — their readiness is driven by
/// whether the binary is installed. Surface code should use
/// `get_provider_status` for a complete view; this fn is kept for
/// backwards-compat with existing call sites and returns `true` for CLI
/// providers to avoid false "missing key" errors.
pub fn has_api_key(provider: &str) -> bool {
    if is_cli_provider(provider) {
        return true;
    }
    retrieve_api_key(provider).is_ok()
}
