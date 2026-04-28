use glob::{MatchOptions, Pattern};
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

#[derive(Clone)]
pub struct CompiledGlob {
    pattern: Pattern,
    matches_basename: bool,
}

pub fn compile_globs(pattern: &str) -> Result<Vec<CompiledGlob>, String> {
    let expanded = expand_brace_patterns(pattern);
    expanded
        .into_iter()
        .map(|item| {
            let matches_basename = !item.contains('/');
            Pattern::new(&item)
                .map(|pattern| CompiledGlob {
                    pattern,
                    matches_basename,
                })
                .map_err(|e| format!("invalid glob pattern '{}': {}", item, e))
        })
        .collect()
}

pub fn matches_globs(path: &Path, search_root: &Path, patterns: &[CompiledGlob]) -> bool {
    if patterns.is_empty() {
        return true;
    }

    let relative = path.strip_prefix(search_root).unwrap_or(path);
    let relative = relative.to_string_lossy().replace('\\', "/");
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let options = MatchOptions {
        case_sensitive: true,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };

    patterns.iter().any(|compiled| {
        compiled.pattern.matches_with(&relative, options)
            || (compiled.matches_basename && compiled.pattern.matches_with(file_name, options))
    })
}

fn expand_brace_patterns(pattern: &str) -> Vec<String> {
    let Some(start) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let Some(relative_end) = pattern[start + 1..].find('}') else {
        return vec![pattern.to_string()];
    };

    let end = start + 1 + relative_end;
    let prefix = &pattern[..start];
    let suffix = &pattern[end + 1..];
    let choices = &pattern[start + 1..end];

    let mut expanded = Vec::new();
    for choice in choices.split(',') {
        let candidate = format!("{}{}{}", prefix, choice.trim(), suffix);
        expanded.extend(expand_brace_patterns(&candidate));
    }

    expanded
}

#[cfg(test)]
mod tests {
    use super::{compile_globs, matches_globs};
    use std::path::Path;

    #[test]
    fn expands_brace_globs() {
        let patterns = compile_globs("*.{ts,tsx}").expect("glob should compile");
        assert_eq!(patterns.len(), 2);
    }

    #[test]
    fn matches_basename_and_relative_paths() {
        let root = Path::new("/tmp/project");
        let file = Path::new("/tmp/project/src/main.rs");

        let basename = compile_globs("*.rs").expect("glob should compile");
        assert!(matches_globs(file, root, &basename));

        let nested = compile_globs("src/**/*.rs").expect("glob should compile");
        assert!(matches_globs(file, root, &nested));
    }
}
