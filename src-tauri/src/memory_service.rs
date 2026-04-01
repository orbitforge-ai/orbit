use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::executor::memory::MemoryClient;

/// Port the memory service listens on.
const DEFAULT_PORT: u16 = 9473;
/// Maximum time to wait for the service to become healthy.
const HEALTH_TIMEOUT_SECS: u64 = 60;
/// Interval between health check retries during startup.
const HEALTH_RETRY_INTERVAL_MS: u64 = 500;
/// Interval between background health checks.
const HEALTH_CHECK_INTERVAL_SECS: u64 = 30;

/// Managed state holding the memory service process and client.
#[derive(Clone)]
pub struct MemoryServiceState {
    pub client: MemoryClient,
    inner: Arc<RwLock<MemoryServiceInner>>,
    service_dir: PathBuf,
    port: u16,
}

struct MemoryServiceInner {
    process: Option<Child>,
}

impl MemoryServiceState {
    /// Start the memory service sidecar and wait for it to become healthy.
    pub async fn start(data_dir: PathBuf) -> Result<Self, String> {
        let service_dir = data_dir.join("memory-service");
        let port = DEFAULT_PORT;
        let client = MemoryClient::new(&format!("http://127.0.0.1:{}", port));

        let state = Self {
            client,
            inner: Arc::new(RwLock::new(MemoryServiceInner { process: None })),
            service_dir,
            port,
        };

        state.ensure_environment().await?;
        state.spawn_process().await?;
        state.wait_for_healthy().await?;

        Ok(state)
    }

    /// Ensure the Python virtualenv and dependencies are installed via uv.
    async fn ensure_environment(&self) -> Result<(), String> {
        let venv_dir = self.service_dir.join(".venv");

        // Copy bundled service files to the data directory if not present
        let server_dest = self.service_dir.join("server.py");
        if !server_dest.exists() {
            self.copy_bundled_files().await?;
        }

        // If venv already exists and has deps, skip setup
        if venv_dir.exists() {
            info!("Memory service venv already exists at {}", venv_dir.display());
            return Ok(());
        }

        info!("Setting up memory service environment at {}", self.service_dir.display());

        // Create venv with uv
        let output = Command::new("uv")
            .args(["venv", "--python", "3.12"])
            .arg(&venv_dir)
            .current_dir(&self.service_dir)
            .output()
            .await
            .map_err(|e| format!("Failed to run uv venv: {}. Is uv installed?", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("uv venv failed: {}", stderr));
        }

        // Install dependencies
        let output = Command::new("uv")
            .args(["pip", "install", "-r", "requirements.txt"])
            .arg("--python")
            .arg(venv_dir.join("bin").join("python"))
            .current_dir(&self.service_dir)
            .output()
            .await
            .map_err(|e| format!("Failed to install dependencies: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("uv pip install failed: {}", stderr));
        }

        info!("Memory service environment ready");
        Ok(())
    }

    /// Copy bundled service files from the app resources to the data directory.
    async fn copy_bundled_files(&self) -> Result<(), String> {
        tokio::fs::create_dir_all(&self.service_dir)
            .await
            .map_err(|e| format!("Failed to create service dir: {}", e))?;

        // The bundled files are compiled into the binary at build time via
        // include_str!, or copied from the resources directory at runtime.
        // For now, we expect them to be at the resource path resolved by Tauri.
        let bundled_dir = self.resolve_bundled_dir()?;

        for filename in &["server.py", "memory_config.py", "requirements.txt", "pyproject.toml"] {
            let src = bundled_dir.join(filename);
            let dest = self.service_dir.join(filename);
            if src.exists() {
                tokio::fs::copy(&src, &dest)
                    .await
                    .map_err(|e| format!("Failed to copy {}: {}", filename, e))?;
            }
        }

        Ok(())
    }

    /// Resolve the path to bundled memory service files.
    fn resolve_bundled_dir(&self) -> Result<PathBuf, String> {
        // In development, look relative to the Cargo project
        let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("memory-service");
        if dev_path.exists() {
            return Ok(dev_path);
        }

        // In production, look in the Tauri resource directory
        // This will be resolved by the caller passing the app handle
        Err("Could not find bundled memory service files".to_string())
    }

    /// Spawn the uvicorn process.
    async fn spawn_process(&self) -> Result<(), String> {
        let python_bin = self.service_dir.join(".venv").join("bin").join("python");

        let child = Command::new(&python_bin)
            .args(["-m", "uvicorn", "server:app", "--host", "127.0.0.1", "--port"])
            .arg(self.port.to_string())
            .current_dir(&self.service_dir)
            .env("ORBIT_MEMORY_PORT", self.port.to_string())
            .env(
                "ORBIT_MEMORY_DATA_DIR",
                self.service_dir.join("data").to_string_lossy().to_string(),
            )
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to spawn memory service: {}", e))?;

        info!("Memory service process spawned on port {}", self.port);
        let mut inner = self.inner.write().await;
        inner.process = Some(child);
        Ok(())
    }

    /// Wait for the health endpoint to respond.
    async fn wait_for_healthy(&self) -> Result<(), String> {
        let deadline = tokio::time::Instant::now()
            + tokio::time::Duration::from_secs(HEALTH_TIMEOUT_SECS);

        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err(format!(
                    "Memory service did not become healthy within {}s",
                    HEALTH_TIMEOUT_SECS
                ));
            }

            match self.client.health_check().await {
                Ok(true) => {
                    info!("Memory service is healthy");
                    return Ok(());
                }
                _ => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(
                        HEALTH_RETRY_INTERVAL_MS,
                    ))
                    .await;
                }
            }
        }
    }

    /// Stop the memory service gracefully.
    pub async fn stop(&self) {
        let mut inner = self.inner.write().await;
        if let Some(ref mut child) = inner.process {
            info!("Stopping memory service");
            let _ = child.kill().await;
            inner.process = None;
        }
    }

    /// Background health check loop. Call from a spawned task.
    pub async fn health_loop(self) {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(
                HEALTH_CHECK_INTERVAL_SECS,
            ))
            .await;

            match self.client.health_check().await {
                Ok(true) => {}
                _ => {
                    warn!("Memory service health check failed, attempting restart");
                    self.stop().await;
                    if let Err(e) = self.spawn_process().await {
                        error!("Failed to restart memory service: {}", e);
                        continue;
                    }
                    if let Err(e) = self.wait_for_healthy().await {
                        error!("Memory service failed to become healthy after restart: {}", e);
                    }
                }
            }
        }
    }
}
