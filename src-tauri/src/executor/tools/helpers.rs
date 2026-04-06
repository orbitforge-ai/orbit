use std::path::{Path, PathBuf};

/// Validate a path stays within the given base directory.
pub fn validate_path(base: &Path, requested: &str) -> Result<PathBuf, String> {
    let resolved = base.join(requested);

    if resolved.exists() {
        let canonical = resolved
            .canonicalize()
            .map_err(|e| format!("failed to resolve path: {}", e))?;
        let base_canonical = base
            .canonicalize()
            .map_err(|e| format!("failed to resolve base: {}", e))?;
        if !canonical.starts_with(&base_canonical) {
            return Err(format!("path escapes workspace: {}", requested));
        }
        return Ok(canonical);
    }

    // For new files, validate the parent
    let parent = resolved.parent().ok_or("invalid path")?;
    if !parent.exists() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create directories: {}", e))?;
    }
    let parent_canonical = parent
        .canonicalize()
        .map_err(|e| format!("failed to resolve parent: {}", e))?;
    let base_canonical = base
        .canonicalize()
        .map_err(|e| format!("failed to resolve base: {}", e))?;
    if !parent_canonical.starts_with(&base_canonical) {
        return Err(format!("path escapes workspace: {}", requested));
    }

    Ok(parent_canonical.join(resolved.file_name().ok_or("no filename")?))
}
