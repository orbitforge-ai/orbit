use std::process::Command;
use tracing::{info, warn};

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
pub fn retrieve_api_key(provider: &str) -> Result<String, String> {
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
pub fn has_api_key(provider: &str) -> bool {
    retrieve_api_key(provider).is_ok()
}
