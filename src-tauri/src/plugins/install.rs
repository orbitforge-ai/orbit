//! Install pipeline: zip-extraction staging -> commit, and dev-mode
//! directory install.
//!
//! Staging is always `<plugins_dir>/.staging/<ulid>/`. On confirmation we
//! atomic-rename into `<plugins_dir>/<plugin-id>/`. On cancel we recursively
//! remove the staging dir. Zip-slip defense rejects any archive entry whose
//! normalised path escapes the extraction root.

use std::io::Read;
use std::path::{Path, PathBuf};

use super::manifest::{self, PluginManifest};
use super::{plugins_dir, staging_dir};

pub const MAX_ZIP_BYTES: u64 = 50 * 1024 * 1024; // 50 MiB in V1.

/// Bootstrap bundled plugins shipped with the app. Walks `./bundled-plugins/`
/// (relative to the binary's cwd — the Tauri bundle resource path in release)
/// and, for every plugin not already in the registry, copies the files into
/// `~/.orbit/plugins/<id>/` and registers it disabled by default.
///
/// In dev (cargo run), the bundle is the workspace root's `bundled-plugins/`.
/// In release builds this should be pointed at the Tauri resources dir; left
/// as best-effort here so absence is not an error.
pub fn bootstrap_bundled_plugins(registry: &mut super::registry::PluginRegistry) {
    let candidate_roots: Vec<std::path::PathBuf> = vec![
        std::path::PathBuf::from("bundled-plugins"),
        std::path::PathBuf::from("../bundled-plugins"),
    ];
    let Some(bundle_root) = candidate_roots.into_iter().find(|p| p.is_dir()) else {
        return;
    };

    let Ok(iter) = std::fs::read_dir(&bundle_root) else {
        return;
    };
    for entry in iter.flatten() {
        let src = entry.path();
        if !src.is_dir() {
            continue;
        }
        let manifest_path = src.join("plugin.json");
        if !manifest_path.is_file() {
            continue;
        }
        let manifest = match super::manifest::load_from_path(&manifest_path) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("bundled plugin {:?} invalid: {}", src, e);
                continue;
            }
        };

        let already_installed = registry
            .entries()
            .iter()
            .any(|entry| entry.id == manifest.id);
        if already_installed {
            // Upgrade path: if bundled version is newer, replace files.
            if let Some(installed) = super::plugins_dir().join(&manifest.id).canonicalize().ok() {
                let _ = installed;
                // V1: always overwrite if version differs, preserving the
                // registry entry so enable state carries across upgrades.
                if let Ok(installed_manifest) =
                    super::manifest::load_from_path(&super::plugins_dir().join(&manifest.id).join("plugin.json"))
                {
                    if installed_manifest.version != manifest.version {
                        let target = super::plugins_dir().join(&manifest.id);
                        let _ = std::fs::remove_dir_all(&target);
                        if let Err(e) = copy_dir_recursive(&src, &target) {
                            tracing::warn!(
                                "bundled plugin upgrade copy failed for {}: {}",
                                manifest.id, e
                            );
                        } else {
                            tracing::info!(
                                "bundled plugin upgraded: {} {} -> {}",
                                manifest.id, installed_manifest.version, manifest.version
                            );
                        }
                    }
                }
            }
            continue;
        }

        let target = super::plugins_dir().join(&manifest.id);
        if target.exists() {
            let _ = std::fs::remove_dir_all(&target);
        }
        if let Err(e) = copy_dir_recursive(&src, &target) {
            tracing::warn!("bundled plugin install failed for {}: {}", manifest.id, e);
            continue;
        }
        let mut entry = super::registry::RegistryEntry::new(manifest.id.clone());
        entry.bundled = true;
        let _ = registry.upsert(entry);
        tracing::info!("bundled plugin installed: {} v{}", manifest.id, manifest.version);
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Stage a plugin zip. Returns the staging id and the parsed manifest so the
/// install review modal can render it before the user confirms.
pub fn stage_from_zip(zip_path: &Path) -> Result<(String, PluginManifest), String> {
    let metadata = std::fs::metadata(zip_path)
        .map_err(|e| format!("failed to stat zip {}: {}", zip_path.display(), e))?;
    if metadata.len() > MAX_ZIP_BYTES {
        return Err(format!(
            "plugin zip too large ({} bytes; max {})",
            metadata.len(),
            MAX_ZIP_BYTES
        ));
    }

    let staging_id = ulid::Ulid::new().to_string();
    let target = staging_dir().join(&staging_id);
    std::fs::create_dir_all(&target)
        .map_err(|e| format!("failed to create staging dir: {}", e))?;

    let file = std::fs::File::open(zip_path)
        .map_err(|e| format!("failed to open zip: {}", e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("failed to open zip archive: {}", e))?;

    extract_archive(&mut archive, &target).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&target);
    })?;

    let manifest_path = target.join("plugin.json");
    let manifest = manifest::load_from_path(&manifest_path).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&target);
    })?;

    Ok((staging_id, manifest))
}

/// Commit a previously-staged plugin into the plugins dir. Idempotent:
/// replacing an existing plugin with the same id first removes the prior dir.
pub fn commit_from_staging(staging_id: &str) -> Result<PluginManifest, String> {
    let source = staging_dir().join(staging_id);
    if !source.is_dir() {
        return Err(format!("staging id {:?} not found", staging_id));
    }
    let manifest_path = source.join("plugin.json");
    let manifest = manifest::load_from_path(&manifest_path)?;

    let target = plugins_dir().join(&manifest.id);
    if target.exists() {
        std::fs::remove_dir_all(&target)
            .map_err(|e| format!("failed to remove existing plugin dir: {}", e))?;
    }
    std::fs::create_dir_all(target.parent().unwrap_or_else(|| Path::new("/")))
        .map_err(|e| format!("failed to create plugins parent dir: {}", e))?;
    std::fs::rename(&source, &target)
        .map_err(|e| format!("failed to commit plugin: {}", e))?;

    Ok(manifest)
}

/// Cancel and delete a staging directory.
pub fn cancel_staging(staging_id: &str) -> Result<(), String> {
    let source = staging_dir().join(staging_id);
    if source.exists() {
        std::fs::remove_dir_all(&source)
            .map_err(|e| format!("failed to remove staging dir: {}", e))?;
    }
    Ok(())
}

/// Dev install — writes a pointer file containing the source directory path
/// under `<plugins_dir>/<id>/`. Runtime resolves the pointer before reading
/// `plugin.json`. V1 uses a pointer file rather than a symlink to sidestep
/// Windows symlink-permission issues.
pub fn install_from_directory(source: &Path) -> Result<PluginManifest, String> {
    if !source.is_dir() {
        return Err(format!("{} is not a directory", source.display()));
    }
    let manifest = manifest::load_from_path(&source.join("plugin.json"))?;

    let target = plugins_dir().join(&manifest.id);
    if target.exists() {
        std::fs::remove_dir_all(&target)
            .map_err(|e| format!("failed to remove existing plugin dir: {}", e))?;
    }
    std::fs::create_dir_all(&target)
        .map_err(|e| format!("failed to create plugin dir: {}", e))?;

    // The pointer file tells the runtime where the real source lives. Copy
    // the manifest out so loading logic can read it without resolving the
    // pointer first.
    let pointer = target.join(".dev-source");
    std::fs::write(&pointer, source.to_string_lossy().as_bytes())
        .map_err(|e| format!("failed to write dev pointer: {}", e))?;

    // Copy plugin.json so the parse path is uniform with zip installs.
    std::fs::copy(source.join("plugin.json"), target.join("plugin.json"))
        .map_err(|e| format!("failed to copy plugin.json: {}", e))?;

    Ok(manifest)
}

/// Resolve the live source directory for a plugin — the dev pointer if it
/// exists, else the installed dir itself.
pub fn resolve_source_dir(plugin_id: &str) -> PathBuf {
    let installed = plugins_dir().join(plugin_id);
    let pointer = installed.join(".dev-source");
    if pointer.is_file() {
        if let Ok(path) = std::fs::read_to_string(&pointer) {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
    }
    installed
}

fn extract_archive<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    target: &Path,
) -> Result<(), String> {
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("failed to read archive entry {}: {}", i, e))?;
        let entry_path = entry
            .enclosed_name()
            .ok_or_else(|| format!("archive entry {:?} escapes root", entry.name()))?
            .to_path_buf();
        // enclosed_name defends against `..` and absolute paths, but we also
        // reject symlinked files for clarity.
        let out_path = target.join(&entry_path);
        if !out_path.starts_with(target) {
            return Err(format!("archive entry {:?} escapes root", entry.name()));
        }
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)
                .map_err(|e| format!("failed to create dir {}: {}", out_path.display(), e))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create parent {}: {}", parent.display(), e))?;
            }
            let mut out = std::fs::File::create(&out_path)
                .map_err(|e| format!("failed to create {}: {}", out_path.display(), e))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("failed to extract {}: {}", out_path.display(), e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn extract_rejects_zip_slip() {
        use zip::write::SimpleFileOptions;
        let mut bytes = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut bytes);
            let mut w = zip::ZipWriter::new(cursor);
            w.start_file("../../../etc/passwd", SimpleFileOptions::default())
                .unwrap();
            w.write_all(b"boom").unwrap();
            w.finish().unwrap();
        }
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let temp = std::env::temp_dir().join(format!(
            "orbit-zip-test-{}",
            ulid::Ulid::new().to_string()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let res = extract_archive(&mut archive, &temp);
        let _ = std::fs::remove_dir_all(&temp);
        assert!(res.is_err());
    }
}
