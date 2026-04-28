//! Serves the production frontend bundle from inside the Rust binary.
//!
//! At compile time `rust-embed` walks `../../dist/` (relative to
//! `crates/orbit-engine/`, i.e. the workspace-root `dist/`) and bakes every
//! file into the binary. In dev the user runs the Vite dev
//! server on a separate port, so the `dist/` directory is normally absent or
//! stale — the embed simply contributes zero files and these handlers return
//! 404. In release (`pnpm build && cargo build --release`) it serves the
//! whole SPA at `/`, with index.html fallback for client-side routes.

use axum::{
    body::Body,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../dist/"]
struct Asset;

/// Serve a static file. Path is taken from `uri.path()`; empty paths get
/// `index.html`. SPA routes (paths the embed doesn't know about) fall back
/// to `index.html` so React Router can take over.
pub async fn handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let candidate = if path.is_empty() { "index.html" } else { path };

    if let Some(file) = Asset::get(candidate) {
        let mime = file.metadata.mimetype();
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .body(Body::from(file.data.into_owned()))
            .unwrap();
    }

    // SPA fallback. If even index.html is missing, the bundle wasn't built.
    if let Some(index) = Asset::get("index.html") {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(index.data.into_owned()))
            .unwrap();
    }

    (
        StatusCode::NOT_FOUND,
        "frontend bundle not built — run `pnpm build` before release",
    )
        .into_response()
}
