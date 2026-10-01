use super::*;

#[test]
fn test_display_path_relative_when_under_workspace_root() {
    // 正常路径：能 strip_prefix 成功时返回相对路径（无 ./ 前缀）
    assert_eq!(
        display_path("/repo/src/lib.rs", Some(Path::new("/repo"))),
        "src/lib.rs"
    );
}

#[test]
fn test_display_path_absolute_when_outside_workspace_root() {
    // 外部路径：strip_prefix 失败时原样返回（不破坏展示）
    assert_eq!(
        display_path("/other/src/lib.rs", Some(Path::new("/repo"))),
        "/other/src/lib.rs"
    );
}

#[test]
fn test_display_path_passthrough_when_workspace_root_none() {
    // workspace_root 为 None 时原样返回（回归保护）
    assert_eq!(display_path("/repo/src/lib.rs", None), "/repo/src/lib.rs");
    assert_eq!(display_path("src/lib.rs", None), "src/lib.rs");
}

#[test]
fn test_display_path_cjk_path() {
    // 中文路径正常处理
    assert_eq!(
        display_path("/项目/子目录/文件.rs", Some(Path::new("/项目"))),
        "子目录/文件.rs"
    );
}

#[test]
fn test_display_path_nonexistent_path_no_panic() {
    // 路径不存在不 panic（不 canonicalize）
    assert_eq!(
        display_path("/repo/does/not/exist.rs", Some(Path::new("/repo"))),
        "does/not/exist.rs"
    );
}

#[test]
fn test_display_path_equals_workspace_root_returns_dot() {
    // 路径等于 workspace_root 本身（strip 成功且为空）→ 返回 "."
    assert_eq!(display_path("/repo", Some(Path::new("/repo"))), ".");
}

#[test]
fn test_display_path_empty_raw() {
    // 空字符串原样返回
    assert_eq!(display_path("", Some(Path::new("/repo"))), "");
}

#[test]
fn test_display_path_relative_input_passthrough() {
    // 输入已是相对路径时，strip_prefix 绝对根会失败 → 原样返回
    assert_eq!(
        display_path("src/lib.rs", Some(Path::new("/repo"))),
        "src/lib.rs"
    );
}

#[test]
fn test_display_path_nested_worktree() {
    // worktree 场景：路径在 worktree 根下时相对化
    let root = "/repo/.worktrees/feature";
    assert_eq!(
        display_path(
            "/repo/.worktrees/feature/src/main.rs",
            Some(Path::new(root))
        ),
        "src/main.rs"
    );
}
