//! Embedded pseudoterminal sessions used by the chat panel's "Terminal" tab.
//!
//! Each `PtySession` owns one PTY pair plus the child process attached to its
//! slave end. Stdout is read on a dedicated OS thread (portable-pty's reader is
//! blocking) and forwarded to the frontend as base64-encoded `terminal:output_chunk`
//! events. Keystrokes flow back via `write` / the `write_terminal` Tauri command.
//!
//! The session is ephemeral by design — when the user closes the tab or the
//! app shuts down, we kill the child via `ChildKiller` and the spawned reader
//! thread exits when the master fd is dropped.
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use tokio::sync::Mutex as AsyncMutex;
use tracing::{debug, warn};

use crate::events::emitter::{emit_terminal_chunk, emit_terminal_exit};

/// Description of a process to launch attached to a PTY. Built by
/// `cli_launcher::build_pty_spec` for the supported CLIs and "generic shell".
pub struct PtySpawnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub rows: u16,
    pub cols: u16,
}

pub struct PtySession {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    /// Optional paths to clean up when the session ends (temp MCP configs etc.)
    cleanup_paths: Vec<PathBuf>,
}

impl PtySession {
    pub fn spawn(
        terminal_id: String,
        spec: PtySpawnSpec,
        app: tauri::AppHandle,
        cleanup_paths: Vec<PathBuf>,
    ) -> Result<Self, String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: spec.rows,
                cols: spec.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("openpty failed: {}", e))?;

        let mut cmd = CommandBuilder::new(&spec.program);
        for arg in &spec.args {
            cmd.arg(arg);
        }
        if let Some(cwd) = &spec.cwd {
            cmd.cwd(cwd);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawn `{}`: {}", spec.program.display(), e))?;
        // Drop the slave handle so the child becomes the only holder; without
        // this the master never sees EOF when the child exits.
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("take_writer: {}", e))?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("try_clone_reader: {}", e))?;
        let killer = child.clone_killer();

        let writer = Arc::new(Mutex::new(writer));
        let master = Arc::new(Mutex::new(pair.master));
        let killer = Arc::new(Mutex::new(killer));

        // Reader thread: pump bytes from master -> frontend. Blocking read, so
        // it lives on a dedicated OS thread, not a tokio task.
        let app_for_read = app.clone();
        let term_id_for_read = terminal_id.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let encoded = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                        emit_terminal_chunk(&app_for_read, &term_id_for_read, &encoded);
                    }
                    Err(e) => {
                        debug!("pty {} reader exit: {}", term_id_for_read, e);
                        break;
                    }
                }
            }
        });

        // Wait thread: own the child, wait for exit, emit terminal:exit.
        let app_for_wait = app.clone();
        let term_id_for_wait = terminal_id.clone();
        std::thread::spawn(move || {
            let code = child.wait().map(|s| s.exit_code() as i32).unwrap_or(-1);
            emit_terminal_exit(&app_for_wait, &term_id_for_wait, code);
        });

        Ok(PtySession {
            writer,
            master,
            killer,
            cleanup_paths,
        })
    }

    pub fn write(&self, bytes: &[u8]) -> Result<(), String> {
        let mut w = self
            .writer
            .lock()
            .map_err(|_| "pty writer mutex poisoned".to_string())?;
        w.write_all(bytes).map_err(|e| e.to_string())?;
        w.flush().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        let m = self
            .master
            .lock()
            .map_err(|_| "pty master mutex poisoned".to_string())?;
        m.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
    }

    pub fn kill(&self) {
        if let Ok(mut k) = self.killer.lock() {
            let _ = k.kill();
        }
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.kill();
        for path in &self.cleanup_paths {
            if path.is_file() {
                let _ = std::fs::remove_file(path);
            } else if path.is_dir() {
                let _ = std::fs::remove_dir_all(path);
            }
        }
    }
}

/// Tauri-managed registry keyed by terminal id. Tokio mutex because async
/// commands hold the lock across `.await` points (e.g. when emitting events).
#[derive(Clone)]
pub struct PtyRegistry(pub Arc<AsyncMutex<HashMap<String, PtySession>>>);

impl PtyRegistry {
    pub fn new() -> Self {
        Self(Arc::new(AsyncMutex::new(HashMap::new())))
    }

    pub async fn insert(&self, id: String, session: PtySession) {
        self.0.lock().await.insert(id, session);
    }

    pub async fn write(&self, id: &str, bytes: &[u8]) -> Result<(), String> {
        let map = self.0.lock().await;
        let session = map
            .get(id)
            .ok_or_else(|| format!("unknown terminal id: {}", id))?;
        session.write(bytes)
    }

    pub async fn resize(&self, id: &str, rows: u16, cols: u16) -> Result<(), String> {
        let map = self.0.lock().await;
        let session = map
            .get(id)
            .ok_or_else(|| format!("unknown terminal id: {}", id))?;
        session.resize(rows, cols)
    }

    pub async fn close(&self, id: &str) {
        let removed = self.0.lock().await.remove(id);
        if removed.is_none() {
            warn!("close_terminal: no session for id {}", id);
        }
        // `removed` drops here -> Drop kills the child + cleans temp paths.
    }

    pub async fn close_all(&self) {
        let mut map = self.0.lock().await;
        map.clear();
    }
}
