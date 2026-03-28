use std::path::PathBuf;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tracing::debug;

use crate::events::emitter::emit_log_chunk;
use crate::models::task::HttpRequestConfig;

pub struct ProcessResult {
    pub exit_code: i32,
    pub duration_ms: i64,
}

/// Executes an HTTP request task.
/// Logs request/response details and streams them to the frontend.
pub async fn run_http(
    run_id: &str,
    cfg: &HttpRequestConfig,
    log_path: &PathBuf,
    timeout_secs: u64,
    app: &tauri::AppHandle,
    cancel: tokio::sync::oneshot::Receiver<()>,
) -> Result<ProcessResult, String> {
    if let Some(parent) = log_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }

    let mut log_file = tokio::fs::File::create(log_path)
        .await
        .map_err(|e| e.to_string())?;

    let start = std::time::Instant::now();

    // Log request details
    let req_line = format!(
        "--> {} {}\n",
        cfg.method.to_uppercase(),
        cfg.url
    );
    log_file.write_all(req_line.as_bytes()).await.ok();
    emit_log_chunk(
        app,
        run_id,
        vec![("stdout".to_string(), format!("--> {} {}", cfg.method.to_uppercase(), cfg.url))],
    );

    if let Some(headers) = &cfg.headers {
        for (k, v) in headers {
            let line = format!("    {}: {}\n", k, v);
            log_file.write_all(line.as_bytes()).await.ok();
        }
    }

    // Build reqwest client
    let timeout = Duration::from_secs(cfg.timeout_seconds.unwrap_or(timeout_secs));
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())?;

    let method = reqwest::Method::from_bytes(cfg.method.to_uppercase().as_bytes())
        .map_err(|e| e.to_string())?;

    let mut req_builder = client.request(method, &cfg.url);

    if let Some(headers) = &cfg.headers {
        for (k, v) in headers {
            req_builder = req_builder.header(k, v);
        }
    }

    if let Some(body) = &cfg.body {
        req_builder = req_builder.body(body.clone());
    }

    let request = req_builder.build().map_err(|e| e.to_string())?;

    // Execute with cancellation support
    let response = tokio::select! {
        result = client.execute(request) => result.map_err(|e| e.to_string())?,
        _ = cancel => {
            let msg = "run cancelled";
            log_file.write_all(format!("[cancelled]\n").as_bytes()).await.ok();
            emit_log_chunk(app, run_id, vec![("stdout".to_string(), msg.to_string())]);
            return Err("cancelled".to_string());
        }
    };

    let status = response.status();
    let duration_ms = start.elapsed().as_millis() as i64;

    let status_line = format!("<-- {} {} ({}ms)\n", status.as_u16(), status.canonical_reason().unwrap_or(""), duration_ms);
    log_file.write_all(status_line.as_bytes()).await.ok();
    emit_log_chunk(
        app,
        run_id,
        vec![("stdout".to_string(), format!(
            "<-- {} {} ({}ms)",
            status.as_u16(),
            status.canonical_reason().unwrap_or(""),
            duration_ms
        ))],
    );

    // Read response body
    let body_bytes = response.bytes().await.map_err(|e| e.to_string())?;
    let body_str = String::from_utf8_lossy(&body_bytes);

    // Log up to 10KB of the response body
    let truncated = if body_bytes.len() > 10_240 {
        format!("{}\n... ({} bytes truncated)\n", &body_str[..10_240], body_bytes.len() - 10_240)
    } else {
        format!("{}\n", body_str)
    };
    log_file.write_all(truncated.as_bytes()).await.ok();

    for line in body_str.lines().take(100) {
        emit_log_chunk(app, run_id, vec![("stdout".to_string(), line.to_string())]);
    }

    // Determine success based on expected status codes
    let expected = cfg.expected_status_codes.as_deref().unwrap_or(&[]);
    let exit_code = if expected.is_empty() {
        // Default: 2xx is success
        if status.is_success() { 0 } else { 1 }
    } else {
        if expected.contains(&status.as_u16()) { 0 } else { 1 }
    };

    if exit_code != 0 {
        let err_line = format!("[error] unexpected status: {}\n", status.as_u16());
        log_file.write_all(err_line.as_bytes()).await.ok();
        emit_log_chunk(
            app,
            run_id,
            vec![("stderr".to_string(), format!("unexpected status: {}", status.as_u16()))],
        );
    }

    debug!(run_id = run_id, status = status.as_u16(), duration_ms = duration_ms, "http task finished");

    Ok(ProcessResult { exit_code, duration_ms })
}
