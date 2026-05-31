//! Path security utilities — shared by file_edit, file_read, file_write, glob, grep.
//!
//! All file-access tools **must** validate that the resolved path stays within
//! the workspace boundary. This module centralises that logic so fixes only
//! need to happen in one place.

use std::path::{Path, PathBuf};

/// Maximum number of path components to prevent abuse.
const MAX_PATH_DEPTH: usize = 64;

/// Normalize and validate a file path against the workspace boundary.
///
/// Returns the normalized absolute path if it resolves inside `workspace_root`,
/// or an error message if it escapes the workspace.
///
/// ## Security guarantees
/// - Uses `Path::starts_with` (path-aware comparison), **not** string prefix.
/// - When `canonicalize` fails and the parent directory also cannot be resolved,
///   the path is **rejected** rather than silently accepted.
/// - Rejects paths with `..` components before normalisation to avoid
///   same-prefix bypass attacks (e.g. `/home/user/app-secrets/../../etc`).
pub fn validate_and_normalize_path(
    file_path: &str,
    workspace_root: &Path,
    allow_outside: bool,
) -> Result<PathBuf, String> {
    validate_and_normalize_path_from_base(file_path, workspace_root, workspace_root, allow_outside)
}

/// Normalize a path against `path_base`, then validate it stays within `workspace_root`.
///
/// This allows tools to resolve relative paths against the active worktree while
/// keeping security checks anchored to the configured workspace boundary.
pub fn validate_and_normalize_path_from_base(
    file_path: &str,
    path_base: &Path,
    workspace_root: &Path,
    allow_outside: bool,
) -> Result<PathBuf, String> {
    // --- Reject obvious traversal attempts early ---
    if !allow_outside && file_path.contains("..") {
        return Err(format!(
            "Path '{}' contains '..' which is not allowed. Only files within the workspace are permitted.",
            file_path
        ));
    }

    // Guard against unreasonably deep paths
    if Path::new(file_path).components().count() > MAX_PATH_DEPTH {
        return Err(format!("Path '{}' exceeds maximum depth limit.", file_path));
    }

    // Convert to absolute path
    let abs_path = if Path::new(file_path).is_absolute() {
        PathBuf::from(file_path)
    } else {
        path_base.join(file_path)
    };

    // Resolve workspace root once
    let workspace_abs = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());

    // Try to canonicalize the path (resolves symlinks, '..', '.')
    let normalized = abs_path
        .canonicalize()
        .or_else(|_| {
            // File doesn't exist yet — resolve parent + filename
            let parent = abs_path.parent().unwrap_or(&abs_path);
            parent
                .canonicalize()
                .map(|p| p.join(abs_path.file_name().unwrap_or_default()))
        })
        .map_err(|_| {
            format!(
                "Path '{}' cannot be resolved. Ensure the parent directory exists and the path is within the workspace.",
                file_path
            )
        })?;

    // Path-aware containment check
    if !allow_outside && !normalized.starts_with(&workspace_abs) {
        return Err(outside_workspace_error("Path", &normalized, &workspace_abs));
    }

    Ok(normalized)
}

/// Validate that a search directory is within the workspace boundary.
///
/// Used by `glob` and `grep` tools where the path is a directory to search.
/// Returns the canonical directory path or an error.
pub fn validate_search_path(path_str: &str, workspace_root: &Path) -> Result<PathBuf, String> {
    validate_search_path_from_base(path_str, workspace_root, workspace_root)
}

pub fn validate_search_path_from_base(
    path_str: &str,
    path_base: &Path,
    workspace_root: &Path,
) -> Result<PathBuf, String> {
    let abs_path = if Path::new(path_str).is_absolute() {
        PathBuf::from(path_str)
    } else {
        path_base.join(path_str)
    };

    let workspace_abs = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());

    let resolved = abs_path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve search path '{}': {}", path_str, e))?;

    if !resolved.starts_with(&workspace_abs) {
        return Err(outside_workspace_error(
            "Search path",
            &resolved,
            &workspace_abs,
        ));
    }

    Ok(resolved)
}

fn outside_workspace_error(kind: &str, path: &Path, workspace_abs: &Path) -> String {
    format!(
        "{kind} '{}' is outside the current workspace '{}'. Prefer relative paths, or use an absolute path under '{}'. Do not retry the same absolute path from another checkout.",
        path.display(),
        workspace_abs.display(),
        workspace_abs.display()
    )
}

/// Validate that a tool_use_id is safe to use as a filename.
///
/// Prevents path traversal via `../` components.
pub fn validate_tool_use_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("tool_use_id must not be empty".to_string());
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(format!(
            "tool_use_id '{}' contains path separators or traversal — rejected for security.",
            id
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_validate_and_normalize_path_relative_uses_current_worktree_base() {
        let workspace = tempdir().unwrap();
        let worktree = workspace.path().join(".worktrees/bug35");
        std::fs::create_dir_all(worktree.join("src")).unwrap();

        let path =
            validate_and_normalize_path_from_base("src/new.rs", &worktree, workspace.path(), false)
                .unwrap();

        assert_eq!(path, worktree.canonicalize().unwrap().join("src/new.rs"));
        assert!(!path.starts_with(workspace.path().join("src")));
    }

    #[test]
    fn test_validate_search_path_from_base_rejects_other_checkout_with_recovery_hint() {
        let workspace = tempdir().unwrap();
        let other_checkout = tempdir().unwrap();

        let err = validate_search_path_from_base(
            other_checkout.path().to_str().unwrap(),
            workspace.path(),
            workspace.path(),
        )
        .unwrap_err();

        assert!(err.contains("outside the current workspace"));
        assert!(err.contains("Prefer relative paths"));
        assert!(err.contains(&workspace.path().display().to_string()));
        assert!(err.contains("Do not retry the same absolute path"));
    }
}
