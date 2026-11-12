use std::fs;

use super::*;

fn workspace() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn touch(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, b"x").expect("write file");
}

#[test]
fn external_url_is_forwarded_verbatim() {
    let target = resolve_open_target("https://example.com/a.md", None, None, None, None)
        .expect("external url");
    assert_eq!(
        target,
        OpenTarget::Url("https://example.com/a.md".to_string())
    );
}

#[test]
fn plain_word_is_unrecognized() {
    let error = resolve_open_target("hello", None, None, None, None).expect_err("plain word");
    assert_eq!(error, OpenTargetError::Unrecognized("hello".to_string()));
}

#[test]
fn relative_markdown_path_resolves_under_workspace_root() {
    let dir = workspace();
    let root = dir.path().join("repo");
    touch(&root.join("docs/snapshots/plans/2026-11-12-kubernetes-cluster-ready-plan.md"));

    let target = resolve_open_target(
        "docs/snapshots/plans/2026-11-12-kubernetes-cluster-ready-plan.md",
        Some(&root.to_string_lossy()),
        None,
        None,
        None,
    )
    .expect("resolved");

    match target {
        OpenTarget::Local {
            requested,
            resolved,
        } => {
            assert_eq!(
                requested,
                "docs/snapshots/plans/2026-11-12-kubernetes-cluster-ready-plan.md"
            );
            assert_eq!(
                resolved,
                root.join("docs/snapshots/plans/2026-11-12-kubernetes-cluster-ready-plan.md")
            );
        }
        other => panic!("expected local target, got {other:?}"),
    }
}

#[test]
fn workspace_root_wins_over_path_base_and_cwd() {
    let dir = workspace();
    let workspace_root = dir.path().join("worktree");
    let path_base = dir.path().join("path-base");
    let cwd = dir.path().join("cwd");
    touch(&workspace_root.join("docs/a.md"));
    touch(&path_base.join("docs/a.md"));
    touch(&cwd.join("docs/a.md"));

    let target = resolve_open_target(
        "docs/a.md",
        Some(&workspace_root.to_string_lossy()),
        Some(&path_base.to_string_lossy()),
        Some(&cwd.to_string_lossy()),
        None,
    )
    .expect("resolved");

    assert_eq!(
        target.spawn_arg(),
        workspace_root.join("docs/a.md").to_string_lossy()
    );
}

#[test]
fn falls_back_to_path_base_then_cwd_then_process_cwd() {
    let dir = workspace();
    let missing = dir.path().join("missing");
    let path_base = dir.path().join("path-base");
    let cwd = dir.path().join("cwd");
    let process_cwd = dir.path().join("process-cwd");
    touch(&cwd.join("docs/b.md"));
    touch(&process_cwd.join("docs/c.md"));

    let from_cwd = resolve_open_target(
        "docs/b.md",
        Some(&missing.to_string_lossy()),
        Some(&path_base.to_string_lossy()),
        Some(&cwd.to_string_lossy()),
        Some(process_cwd.as_path()),
    )
    .expect("cwd fallback");
    assert_eq!(
        from_cwd.spawn_arg(),
        cwd.join("docs/b.md").to_string_lossy()
    );

    let from_process_cwd = resolve_open_target(
        "docs/c.md",
        Some(&missing.to_string_lossy()),
        None,
        None,
        Some(process_cwd.as_path()),
    )
    .expect("process cwd fallback");
    assert_eq!(
        from_process_cwd.spawn_arg(),
        process_cwd.join("docs/c.md").to_string_lossy()
    );
}

#[test]
fn missing_file_reports_every_candidate_instead_of_succeeding() {
    let dir = workspace();
    let workspace_root = dir.path().join("worktree");
    let cwd = dir.path().join("cwd");

    let error = resolve_open_target(
        "docs/missing.md",
        Some(&workspace_root.to_string_lossy()),
        None,
        Some(&cwd.to_string_lossy()),
        None,
    )
    .expect_err("must not resolve");

    match error {
        OpenTargetError::NotFound {
            requested,
            candidates,
        } => {
            assert_eq!(requested, "docs/missing.md");
            assert_eq!(
                candidates,
                vec![
                    workspace_root.join("docs/missing.md"),
                    cwd.join("docs/missing.md"),
                ]
            );
        }
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn absolute_path_is_checked_directly() {
    let dir = workspace();
    let file = dir.path().join("docs/absolute.md");
    touch(&file);

    let target = resolve_open_target(&file.to_string_lossy(), None, None, None, None)
        .expect("absolute path exists");
    assert_eq!(target.spawn_arg(), file.to_string_lossy());

    let missing = dir.path().join("docs/nope.md");
    let error = resolve_open_target(&missing.to_string_lossy(), None, None, None, None)
        .expect_err("absolute missing");
    assert!(
        matches!(error, OpenTargetError::NotFound { candidates, .. } if candidates == vec![missing])
    );
}

#[test]
fn directories_are_openable_targets() {
    let dir = workspace();
    let folder = dir.path().join("docs/snapshots/plans");
    fs::create_dir_all(&folder).expect("create dir");

    let target = resolve_open_target(
        "docs/snapshots/plans",
        Some(&dir.path().to_string_lossy()),
        None,
        None,
        None,
    )
    .expect("directory resolves");
    assert_eq!(target.spawn_arg(), folder.to_string_lossy());
}

#[test]
fn duplicated_bases_are_deduplicated() {
    let dir = workspace();
    let root = dir.path().join("repo");
    let root_str = root.to_string_lossy().into_owned();

    let candidates = local_path_candidates("docs/a.md", &[Some(&root_str), Some(&root_str)]);
    assert_eq!(candidates, vec![root.join("docs/a.md")]);
}

#[test]
fn empty_bases_are_skipped() {
    let candidates = local_path_candidates("docs/a.md", &[Some(""), Some("   "), None]);
    assert!(candidates.is_empty());
}

#[test]
fn zero_exit_is_success() {
    assert_eq!(interpret_open_output(true, Some(0), b""), Ok(()));
    // 有 stderr 但退出码为 0 时仍按成功处理（部分实现会打印告警）。
    assert_eq!(interpret_open_output(true, Some(0), b"warning"), Ok(()));
}

#[test]
fn non_zero_exit_reports_first_stderr_line_instead_of_success() {
    let stderr = b"The file /tmp/docs/x.md does not exist.\n";
    assert_eq!(
        interpret_open_output(false, Some(1), stderr),
        Err("The file /tmp/docs/x.md does not exist.".to_string())
    );
}

#[test]
fn non_zero_exit_without_stderr_falls_back_to_exit_code() {
    assert_eq!(
        interpret_open_output(false, Some(1), b""),
        Err("退出码 1".to_string())
    );
}

#[test]
fn signaled_process_without_stderr_is_reported_as_failure() {
    assert_eq!(
        interpret_open_output(false, None, b"\n  \n"),
        Err("进程被信号终止".to_string())
    );
}
