//! Path security utilities — centralized path validation and normalization.
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
/// - When the target file does not exist, walks up the ancestor chain to find
///   the first existing directory and canonicalises that, so creating new files
///   under multiple missing parent directories is supported without weakening
///   the workspace containment check.
/// - `.` and `..` components are lexically resolved after joining `path_base`,
///   so in-workspace traversal (e.g. `../sibling/x.rs` after `cd` into a
///   subdirectory) is permitted while out-of-workspace traversal is still
///   rejected by `starts_with`.
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
    // Guard against unreasonably deep paths
    if Path::new(file_path).components().count() > MAX_PATH_DEPTH {
        return Err(format!("Path '{}' exceeds maximum depth limit.", file_path));
    }

    // Convert to absolute path
    let joined = if Path::new(file_path).is_absolute() {
        PathBuf::from(file_path)
    } else {
        path_base.join(file_path)
    };
    // Lexically resolve `.` and `..` before any filesystem probing so the
    // containment check below cannot be fooled by un-normalised traversal.
    // In-workspace `..` (e.g. after `cd` into a subdirectory) is permitted;
    // out-of-workspace traversal is caught by `starts_with`.
    let abs_path = lexical_normalize(&joined);

    // Resolve workspace root once
    let workspace_abs = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());

    // Try to canonicalize the path (resolves symlinks, '..', '.').
    // When the target file does not exist yet (common for Write/Edit creating
    // new files), walk up the ancestor chain to find the first existing
    // directory, canonicalise it, then re-append the non-existent tail
    // components. This supports creating files under multiple missing parent
    // directories (e.g. `i18n/prompt/foo.rs` where `i18n/` is entirely absent)
    // and lets downstream tools (file_write's create_dir_all) handle directory
    // creation instead of being blocked here.
    let normalized = abs_path.canonicalize().unwrap_or_else(|_| {
        let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
        if let Some(name) = abs_path.file_name() {
            tail.push(name);
        }
        let mut current = abs_path.parent();
        while let Some(ancestor) = current {
            match ancestor.canonicalize() {
                Ok(canonical) => {
                    let mut full = canonical;
                    for comp in tail.into_iter().rev() {
                        full = full.join(comp);
                    }
                    return full;
                }
                Err(_) => {
                    if let Some(name) = ancestor.file_name() {
                        tail.push(name);
                    }
                    current = ancestor.parent();
                }
            }
        }
        // No existing ancestor found; fall back to the raw path so the
        // containment check below can reject it if it escapes the workspace.
        abs_path.clone()
    });

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
pub fn validate_search_path(
    path_str: &str,
    workspace_root: &Path,
    allow_outside: bool,
) -> Result<PathBuf, String> {
    validate_search_path_from_base(path_str, workspace_root, workspace_root, allow_outside)
}

pub fn validate_search_path_from_base(
    path_str: &str,
    path_base: &Path,
    workspace_root: &Path,
    allow_outside: bool,
) -> Result<PathBuf, String> {
    let joined = if Path::new(path_str).is_absolute() {
        PathBuf::from(path_str)
    } else {
        path_base.join(path_str)
    };
    // Lexically resolve `.` / `..` before canonicalize so in-workspace
    // traversal (e.g. `../sibling-dir` after `cd`) is not rejected by a
    // naive string check; out-of-workspace traversal is caught by
    // `starts_with` below.
    let abs_path = lexical_normalize(&joined);

    let workspace_abs = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());

    let resolved = abs_path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve search path '{}': {}", path_str, e))?;

    if !allow_outside && !resolved.starts_with(&workspace_abs) {
        return Err(outside_workspace_error(
            "Search path",
            &resolved,
            &workspace_abs,
        ));
    }

    Ok(resolved)
}

/// Lexically normalise a path by resolving `.` and `..` components **without**
/// touching the filesystem (no symlink resolution, no existence check).
///
/// Rules:
/// - `CurDir` (`.`) is dropped.
/// - `ParentDir` (`..`) pops the last normal component, unless the stack is
///   empty. For absolute paths the stack starts at the root prefix, so `..`
///   above the root is clamped at the root (prevents `/etc/../..` from
///   escaping to `/`).
/// - `RootDir`/`Prefix` reset the stack to that prefix.
/// - Other components are pushed.
///
/// Used before `canonicalize` so that `starts_with` workspace checks operate on
/// traversal-free paths even when the target file does not exist yet.
fn lexical_normalize(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut stack: Vec<Component<'_>> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => { /* drop `.` */ }
            Component::ParentDir => {
                // Pop only if the top is a normal component (don't pop root/prefix).
                if matches!(stack.last(), Some(Component::Normal(_))) {
                    stack.pop();
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                // Absolute anchor: reset stack to this prefix.
                stack.clear();
                stack.push(comp);
            }
            Component::Normal(_) => stack.push(comp),
        }
    }
    let mut out = PathBuf::new();
    for comp in stack {
        out.push(comp.as_os_str());
    }
    out
}

fn outside_workspace_error(kind: &str, path: &Path, workspace_abs: &Path) -> String {
    format!(
        "{kind} '{}' is outside the current workspace '{}'. Prefer relative paths, or use an absolute path under '{}'. Do not retry the same absolute path from another checkout.",
        path.display(),
        workspace_abs.display(),
        workspace_abs.display()
    )
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
            false,
        )
        .unwrap_err();

        assert!(err.contains("outside the current workspace"));
        assert!(err.contains("Prefer relative paths"));
        assert!(err.contains(&workspace.path().display().to_string()));
        assert!(err.contains("Do not retry the same absolute path"));
    }

    #[test]
    fn test_validate_search_path_allow_outside_permits_external() {
        let workspace = tempdir().unwrap();
        let external = tempdir().unwrap();

        let result = validate_search_path_from_base(
            external.path().to_str().unwrap(),
            workspace.path(),
            workspace.path(),
            true, // allow_outside
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), external.path().canonicalize().unwrap());
    }

    #[test]
    fn test_validate_and_normalize_path_allow_outside_permits_external() {
        let workspace = tempdir().unwrap();
        let external = tempdir().unwrap();
        let external_file = external.path().join("config.toml");
        std::fs::write(&external_file, "test").unwrap();

        let result = validate_and_normalize_path_from_base(
            external_file.to_str().unwrap(),
            workspace.path(),
            workspace.path(),
            true, // allow_outside
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_search_path_allow_outside_allows_traversal() {
        // With allow_outside, .. is permitted — canonicalize resolves it,
        // and the boundary check is skipped.
        let workspace = tempdir().unwrap();
        let parent = workspace.path().parent().unwrap();
        let result = validate_search_path_from_base(
            "..",
            workspace.path(),
            workspace.path(),
            true, // allow_outside
        );
        // Should resolve to the parent dir of workspace, which is outside.
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), parent.canonicalize().unwrap());
    }

    #[test]
    fn test_validate_and_normalize_path_from_base_allows_multi_level_missing_parents() {
        // 复现 issue 429：新建文件时多层父目录缺失（i18n/ 子树整体不存在），
        // 合法的 workspace 内路径不应被误拒。返回的规范化路径应保留原结构，
        // 由下游工具（如 file_write.rs 的 create_dir_all）负责创建中间目录。
        let workspace = tempdir().unwrap();
        let worktree = workspace.path().join(".worktrees/bug429");
        std::fs::create_dir_all(&worktree).unwrap(); // worktree 根存在，i18n/ 整体缺失

        let path = validate_and_normalize_path_from_base(
            "agent/shared/src/i18n/mod.rs",
            &worktree,
            workspace.path(),
            false,
        )
        .unwrap();

        assert_eq!(
            path,
            worktree
                .canonicalize()
                .unwrap()
                .join("agent/shared/src/i18n/mod.rs")
        );
    }

    #[test]
    fn test_validate_and_normalize_path_from_base_rejects_multi_level_missing_outside_workspace() {
        // 多层父目录缺失且最终祖先在 workspace 之外 → 必须拒绝（workspace 越界）。
        let workspace = tempdir().unwrap();
        let external = tempdir().unwrap();

        let err = validate_and_normalize_path_from_base(
            "deep/nested/missing/new.rs",
            external.path(),
            workspace.path(),
            false,
        )
        .unwrap_err();

        assert!(err.contains("outside the current workspace"));
    }

    #[test]
    fn test_validate_and_normalize_path_from_base_allows_in_workspace_traversal() {
        // `..` 指向 workspace 内的合法位置时应通过。
        // 场景：bash cd 到子目录 a/b 后（path_base=a/b），访问兄弟目录文件。
        let workspace = tempdir().unwrap();
        let worktree = workspace.path().join(".worktrees/trav");
        std::fs::create_dir_all(worktree.join("a/b")).unwrap();
        std::fs::create_dir_all(worktree.join("a/sibling")).unwrap();
        std::fs::write(worktree.join("a/sibling/x.rs"), "// x").unwrap();
        let path_base = worktree.join("a/b");

        let path = validate_and_normalize_path_from_base(
            "../sibling/x.rs",
            &path_base,
            workspace.path(),
            false,
        )
        .unwrap();

        assert_eq!(
            path,
            worktree.canonicalize().unwrap().join("a/sibling/x.rs")
        );
    }

    #[test]
    fn test_validate_and_normalize_path_from_base_allows_multi_dot_dot_in_workspace() {
        // 多层 .. 仍在 workspace 内时通过。
        // path_base = worktree/a/b/c，访问 ../../top.rs → worktree/a/top.rs。
        let workspace = tempdir().unwrap();
        let worktree = workspace.path().join(".worktrees/trav2");
        std::fs::create_dir_all(worktree.join("a/b/c")).unwrap();
        let path_base = worktree.join("a/b/c");

        let path = validate_and_normalize_path_from_base(
            "../../new_top.rs",
            &path_base,
            workspace.path(),
            false,
        )
        .unwrap();

        assert_eq!(path, worktree.canonicalize().unwrap().join("a/new_top.rs"));
    }

    #[test]
    fn test_validate_and_normalize_path_from_base_rejects_traversal_outside_workspace() {
        // `..` 超出 workspace 边界 → 必须拒绝。
        // path_base = workspace 根，访问 ../outside.rs → workspace 的父目录。
        let workspace = tempdir().unwrap();

        let err = validate_and_normalize_path_from_base(
            "../outside.rs",
            workspace.path(),
            workspace.path(),
            false,
        )
        .unwrap_err();

        assert!(err.contains("outside the current workspace"));
    }

    #[test]
    fn test_lexical_normalize_resolves_dot_and_dot_dot() {
        // 纯函数单测：消解 . 和 ..
        use std::path::PathBuf;
        assert_eq!(
            lexical_normalize(&PathBuf::from("/repo/a/b/../x.rs")),
            PathBuf::from("/repo/a/x.rs")
        );
    }

    #[test]
    fn test_lexical_normalize_drops_current_dir() {
        // 纯函数单测：消解 .
        use std::path::PathBuf;
        assert_eq!(
            lexical_normalize(&PathBuf::from("/repo/./a/./b.rs")),
            PathBuf::from("/repo/a/b.rs")
        );
    }

    #[test]
    fn test_lexical_normalize_trailing_parent_below_root_stays_at_root() {
        // 纯函数单测：根之上的 .. 被钳制在根（避免越界漏网）
        use std::path::PathBuf;
        assert_eq!(
            lexical_normalize(&PathBuf::from("/etc/../../etc/passwd")),
            PathBuf::from("/etc/passwd")
        );
    }
}
