use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tracing::{debug, warn};
use url::Url;

use crate::events::emitter::emit_log_chunk;
use crate::models::task::HttpRequestConfig;

pub struct ProcessResult {
    pub exit_code: i32,
    pub duration_ms: i64,
}

// ─── SSRF protection ───────────────────────────────────────────────────────

/// Returns true if the IP address is private, loopback, link-local, or
/// otherwise should not be reachable from a sandboxed agent.
fn is_dangerous_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()              // 127.0.0.0/8
            || v4.is_private()            // 10/8, 172.16/12, 192.168/16
            || v4.is_link_local()         // 169.254.0.0/16
            || v4.is_broadcast()          // 255.255.255.255
            || v4.is_unspecified()        // 0.0.0.0
            || is_v4_metadata(v4)         // cloud metadata (169.254.169.254)
            || v4.is_documentation()      // 192.0.2/24, 198.51.100/24, 203.0.113/24
            || is_v4_shared(v4)           // 100.64.0.0/10 (CGN)
            || is_v4_benchmarking(v4)     // 198.18.0.0/15
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()              // ::1
            || v6.is_unspecified()        // ::
            || is_v6_unique_local(v6)     // fc00::/7
            || is_v6_link_local(v6)       // fe80::/10
            // IPv4-mapped IPv6 (::ffff:x.x.x.x) — check the inner v4
            || match v6.to_ipv4_mapped() {
                Some(v4) => is_dangerous_ip(IpAddr::V4(v4)),
                None => false,
            }
        }
    }
}

fn is_v4_metadata(ip: Ipv4Addr) -> bool {
    // AWS/GCP/Azure metadata endpoint
    ip == Ipv4Addr::new(169, 254, 169, 254)
}

fn is_v4_shared(ip: Ipv4Addr) -> bool {
    // 100.64.0.0/10 (Carrier-grade NAT)
    let octets = ip.octets();
    octets[0] == 100 && (octets[1] & 0xC0) == 64
}

fn is_v4_benchmarking(ip: Ipv4Addr) -> bool {
    // 198.18.0.0/15
    let octets = ip.octets();
    octets[0] == 198 && (octets[1] & 0xFE) == 18
}

fn is_v6_unique_local(ip: Ipv6Addr) -> bool {
    // fc00::/7
    (ip.segments()[0] & 0xFE00) == 0xFC00
}

fn is_v6_link_local(ip: Ipv6Addr) -> bool {
    // fe80::/10
    (ip.segments()[0] & 0xFFC0) == 0xFE80
}

/// Validate a URL for SSRF safety. Blocks requests to private/internal networks,
/// cloud metadata endpoints, and non-HTTP(S) schemes.
async fn validate_url_for_ssrf(url_str: &str) -> Result<(), String> {
    let url = Url::parse(url_str)
        .map_err(|e| format!("Invalid URL '{}': {}", url_str, e))?;

    // Only allow http and https schemes
    match url.scheme() {
        "http" | "https" => {}
        scheme => return Err(format!("Blocked URL scheme '{}' — only http/https allowed", scheme)),
    }

    // Block URLs without a host
    let host = url.host_str()
        .ok_or_else(|| "Blocked URL with no host".to_string())?;

    // Block common metadata hostnames
    let host_lower = host.to_lowercase();
    if host_lower == "metadata.google.internal"
        || host_lower == "metadata"
        || host_lower.ends_with(".internal")
    {
        return Err(format!("Blocked cloud metadata host '{}'", host));
    }

    // Resolve hostname to IP and check each address
    let addrs = tokio::net::lookup_host(format!("{}:{}", host, url.port_or_known_default().unwrap_or(80)))
        .await
        .map_err(|e| format!("DNS resolution failed for '{}': {}", host, e))?;

    let addrs: Vec<_> = addrs.collect();
    if addrs.is_empty() {
        return Err(format!("DNS resolution returned no addresses for '{}'", host));
    }

    for addr in &addrs {
        if is_dangerous_ip(addr.ip()) {
            return Err(format!(
                "Blocked request to '{}' — resolves to private/internal IP {}",
                host, addr.ip()
            ));
        }
    }

    Ok(())
}

// ─── Header sanitization ───────────────────────────────────────────────────

/// Sanitize a header value by rejecting CRLF characters that could enable
/// header injection or response splitting attacks.
fn validate_header_value(name: &str, value: &str) -> Result<(), String> {
    if value.contains('\r') || value.contains('\n') {
        return Err(format!(
            "Blocked header '{}' — value contains CR/LF characters (potential header injection)",
            name
        ));
    }
    Ok(())
}

/// Sanitize a header name by rejecting CRLF characters.
fn validate_header_name(name: &str) -> Result<(), String> {
    if name.contains('\r') || name.contains('\n') || name.contains(':') {
        return Err(format!(
            "Blocked header name '{}' — contains invalid characters",
            name
        ));
    }
    Ok(())
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

    // ── SSRF protection: validate URL before making any request ──
    if let Err(reason) = validate_url_for_ssrf(&cfg.url).await {
        warn!(run_id = run_id, url = %cfg.url, reason = %reason, "SSRF protection blocked request");
        let msg = format!("[blocked] {}\n", reason);
        log_file.write_all(msg.as_bytes()).await.ok();
        emit_log_chunk(app, run_id, vec![("stderr".to_string(), reason.clone())]);
        return Ok(ProcessResult { exit_code: 1, duration_ms: 0 });
    }

    // ── Header sanitization: reject CRLF injection attempts ──
    if let Some(headers) = &cfg.headers {
        for (k, v) in headers {
            if let Err(reason) = validate_header_name(k) {
                warn!(run_id = run_id, header = %k, "CRLF header injection blocked");
                let msg = format!("[blocked] {}\n", reason);
                log_file.write_all(msg.as_bytes()).await.ok();
                emit_log_chunk(app, run_id, vec![("stderr".to_string(), reason.clone())]);
                return Ok(ProcessResult { exit_code: 1, duration_ms: 0 });
            }
            if let Err(reason) = validate_header_value(k, v) {
                warn!(run_id = run_id, header = %k, "CRLF header injection blocked");
                let msg = format!("[blocked] {}\n", reason);
                log_file.write_all(msg.as_bytes()).await.ok();
                emit_log_chunk(app, run_id, vec![("stderr".to_string(), reason.clone())]);
                return Ok(ProcessResult { exit_code: 1, duration_ms: 0 });
            }
        }
    }

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

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── IP classification tests ────────────────────────────────────────────

    #[test]
    fn blocks_loopback_ipv4() {
        assert!(is_dangerous_ip(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert!(is_dangerous_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2))));
    }

    #[test]
    fn blocks_private_ranges() {
        // 10.0.0.0/8
        assert!(is_dangerous_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        // 172.16.0.0/12
        assert!(is_dangerous_ip(IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
        assert!(is_dangerous_ip(IpAddr::V4(Ipv4Addr::new(172, 31, 255, 255))));
        // 192.168.0.0/16
        assert!(is_dangerous_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
    }

    #[test]
    fn blocks_link_local() {
        assert!(is_dangerous_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1))));
    }

    #[test]
    fn blocks_cloud_metadata() {
        assert!(is_dangerous_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))));
    }

    #[test]
    fn blocks_cgn_range() {
        // 100.64.0.0/10
        assert!(is_dangerous_ip(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
        assert!(is_dangerous_ip(IpAddr::V4(Ipv4Addr::new(100, 127, 255, 255))));
    }

    #[test]
    fn allows_public_ips() {
        assert!(!is_dangerous_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(!is_dangerous_ip(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
        assert!(!is_dangerous_ip(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))));
    }

    #[test]
    fn blocks_ipv6_loopback() {
        assert!(is_dangerous_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
    }

    #[test]
    fn blocks_ipv6_unique_local() {
        // fc00::/7
        assert!(is_dangerous_ip(IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1))));
        assert!(is_dangerous_ip(IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1))));
    }

    #[test]
    fn blocks_ipv4_mapped_ipv6() {
        // ::ffff:127.0.0.1
        let mapped = Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x7f00, 0x0001);
        assert!(is_dangerous_ip(IpAddr::V6(mapped)));
    }

    // ── URL validation tests ───────────────────────────────────────────────

    #[tokio::test]
    async fn blocks_non_http_schemes() {
        assert!(validate_url_for_ssrf("file:///etc/passwd").await.is_err());
        assert!(validate_url_for_ssrf("ftp://internal/file").await.is_err());
        assert!(validate_url_for_ssrf("gopher://evil").await.is_err());
    }

    #[tokio::test]
    async fn blocks_metadata_hostnames() {
        assert!(validate_url_for_ssrf("http://metadata.google.internal/computeMetadata/v1/").await.is_err());
    }

    #[tokio::test]
    async fn blocks_localhost_urls() {
        assert!(validate_url_for_ssrf("http://127.0.0.1/admin").await.is_err());
        assert!(validate_url_for_ssrf("http://[::1]/admin").await.is_err());
    }

    #[tokio::test]
    async fn blocks_private_ip_urls() {
        assert!(validate_url_for_ssrf("http://10.0.0.1/internal").await.is_err());
        assert!(validate_url_for_ssrf("http://192.168.1.1/router").await.is_err());
        assert!(validate_url_for_ssrf("http://172.16.0.1/api").await.is_err());
    }

    #[tokio::test]
    async fn blocks_metadata_ip() {
        assert!(validate_url_for_ssrf("http://169.254.169.254/latest/meta-data/").await.is_err());
    }

    // ── Header sanitization tests ──────────────────────────────────────────

    #[test]
    fn allows_clean_headers() {
        assert!(validate_header_value("Content-Type", "application/json").is_ok());
        assert!(validate_header_name("Authorization").is_ok());
    }

    #[test]
    fn blocks_crlf_in_header_value() {
        assert!(validate_header_value("X-Custom", "value\r\nInjected: header").is_err());
        assert!(validate_header_value("X-Custom", "value\rinjection").is_err());
        assert!(validate_header_value("X-Custom", "value\ninjection").is_err());
    }

    #[test]
    fn blocks_crlf_in_header_name() {
        assert!(validate_header_name("X-Bad\r\nHeader").is_err());
        assert!(validate_header_name("Header:Injection").is_err());
    }
}
