use context::adapters::decode_session;
use context::domain::session::{
    CanonicalSession, CommittedStep, SessionCodec, SessionCodecError, SnapshotState,
    CURRENT_SESSION_SCHEMA_VERSION,
};
use serde_json::json;
use share::message::Message;
use share::session_types::{PersistedWorkspaceContext, ProjectIdentity, WorkspaceId, WorktreeKind};

#[test]
fn current_envelope_round_trips_canonically() {
    let session = CanonicalSession::fixture("session-1");
    let bytes = SessionCodec::encode(&session).unwrap();
    let decoded = decode_session(&bytes).unwrap();
    assert_eq!(decoded.session, session);
    assert!(!String::from_utf8(bytes).unwrap().contains("\"messages\""));
}

#[test]
fn legacy_messages_upgrade_to_single_normal_chat() {
    let bytes = serde_json::to_vec(&json!({
        "id": "legacy",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-02T00:00:00Z",
        "messages": [Message::user("legacy fact")],
        "metadata": {"title": "old"}
    }))
    .unwrap();
    let decoded = decode_session(&bytes).unwrap();
    assert!(decoded.upgraded_from_legacy);
    assert_eq!(decoded.session.chats.len(), 1);
    assert_eq!(
        decoded.session.chats[0].messages[0].text_content(),
        "legacy fact"
    );
    assert_eq!(decoded.session.metadata.title.as_deref(), Some("old"));
    assert!(matches!(decoded.session.tasks, SnapshotState::Missing));
    assert!(matches!(decoded.session.workspace, SnapshotState::Missing));
}

#[test]
fn explicit_empty_snapshot_is_distinct_from_missing() {
    let mut session = CanonicalSession::fixture("empty");
    session.tasks = SnapshotState::CapturedEmpty;
    session.workspace = SnapshotState::CapturedEmpty;
    let decoded = decode_session(&SessionCodec::encode(&session).unwrap()).unwrap();
    assert!(matches!(
        decoded.session.tasks,
        SnapshotState::CapturedEmpty
    ));
    assert!(matches!(
        decoded.session.workspace,
        SnapshotState::CapturedEmpty
    ));
}

#[test]
fn future_version_is_rejected_without_losing_original_bytes() {
    let bytes = serde_json::to_vec(&json!({
        "schema_version": CURRENT_SESSION_SCHEMA_VERSION + 1,
        "id": "future",
        "unknown": {"must": "survive"}
    }))
    .unwrap();
    assert!(matches!(
        decode_session(&bytes),
        Err(SessionCodecError::UnsupportedFutureVersion { version, original_bytes })
            if version == CURRENT_SESSION_SCHEMA_VERSION + 1 && original_bytes == bytes
    ));
}

#[test]
fn committed_step_ledger_round_trips() {
    let mut session = CanonicalSession::fixture("ledger");
    session
        .committed_steps
        .push(CommittedStep::fixture("run", "step", "fingerprint", 2));
    let decoded = decode_session(&SessionCodec::encode(&session).unwrap()).unwrap();
    assert_eq!(decoded.session.revision, 2);
    assert_eq!(decoded.session.committed_steps, session.committed_steps);
}

// ---- #894: Session legacy workspace/cwd ACL regression coverage ----

/// 生成一个唯一、真实存在的 NonGit 临时目录，返回其 canonical 路径字符串。
fn unique_non_git_dir(tag: &str) -> (std::path::PathBuf, String) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("aemeath_894_codec_{tag}_{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    let canonical = dir.canonicalize().unwrap();
    let text = canonical.to_string_lossy().into_owned();
    (dir, text)
}

/// 初始化一个真实的 primary Git 仓库并返回其 canonical 根路径。
fn init_git_repo(tag: &str) -> (std::path::PathBuf, String) {
    let (dir, _) = unique_non_git_dir(tag);
    let status = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&dir)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .status()
        .expect("spawn git init");
    assert!(status.success(), "git init 应成功");
    let canonical = dir.canonicalize().unwrap();
    let text = canonical.to_string_lossy().into_owned();
    (canonical, text)
}

fn canonical_text(path: &std::path::Path) -> String {
    path.canonicalize().unwrap().to_string_lossy().into_owned()
}

fn captured_workspace(
    state: &SnapshotState<PersistedWorkspaceContext>,
) -> PersistedWorkspaceContext {
    match state {
        SnapshotState::Captured(ctx) => ctx.clone(),
        other => panic!("expected an upgraded Captured workspace, got {other:?}"),
    }
}

/// #894 (1): canonical writer 绝不落盘 top-level `Session.cwd`——
/// cwd 语义只存活于 workspace 的 `project_identity.initial_cwd`。
#[test]
fn canonical_writer_never_emits_top_level_cwd() {
    let mut session = CanonicalSession::fixture("no-cwd");
    session.workspace = SnapshotState::Captured(PersistedWorkspaceContext {
        workspace_id: WorkspaceId::from("ws-cwd-guard"),
        project_identity: ProjectIdentity {
            initial_cwd: "/tmp/project".to_string(),
            git_common_dir: None,
        },
        path_base: "/tmp/project".to_string(),
        workspace_root: "/tmp/project".to_string(),
        worktree_kind: WorktreeKind::NonGit,
        context_stack: vec![],
    });

    let bytes = SessionCodec::encode(&session).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        value.get("cwd").is_none(),
        "canonical envelope 不得含 top-level cwd 字段: {value}"
    );
    // cwd 仍应作为 workspace identity 的一部分被保留（在嵌套结构中）。
    assert!(
        String::from_utf8(bytes).unwrap().contains("initial_cwd"),
        "workspace identity 应保留 initial_cwd"
    );
}

/// #894 (2): legacy `workspace == None` 且携带 `cwd` 时，必须升级为一份完整的
/// canonical NonGit workspace——identity/root/path 三者一致、栈为空、workspace_id
/// 为确定性派生值。
#[test]
fn legacy_cwd_upgrades_to_full_non_git_workspace() {
    let (dir, cwd) = unique_non_git_dir("upgrade");

    let bytes = serde_json::to_vec(&json!({
        "id": "legacy-cwd",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-02T00:00:00Z",
        "cwd": cwd,
        "messages": [Message::user("legacy fact")],
    }))
    .unwrap();

    let decoded = decode_session(&bytes).unwrap();
    assert!(decoded.upgraded_from_legacy);

    let ctx = captured_workspace(&decoded.session.workspace);
    let expected_identity = ProjectIdentity {
        initial_cwd: cwd.clone(),
        git_common_dir: None,
    };
    assert_eq!(
        ctx.project_identity, expected_identity,
        "NonGit 升级 identity 应来自 cwd 且无 git common dir"
    );
    assert_eq!(ctx.workspace_root, cwd, "workspace_root 应与 cwd 一致");
    assert_eq!(ctx.path_base, cwd, "path_base 应与 cwd 一致");
    assert_eq!(ctx.worktree_kind, WorktreeKind::NonGit);
    assert!(
        ctx.context_stack.is_empty(),
        "NonGit 升级 context_stack 必须为空"
    );
    assert_eq!(
        ctx.workspace_id,
        WorkspaceId::derive(&expected_identity, &cwd),
        "workspace_id 必须是确定性派生值"
    );
    assert!(
        !ctx.workspace_id.as_str().is_empty(),
        "升级后的 workspace_id 不得为空"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// #894 (3): legacy workspace 仅有旧字段（path_base/workspace_root）、缺 identity/id/kind
/// 时，codec 必须补全这些字段，而非留下 serde 默认的空值。
#[test]
fn legacy_workspace_backfills_missing_identity_id_and_kind() {
    let fixture_root = std::env::temp_dir().join("aemeath-894-legacy-root");
    let _ = std::fs::remove_dir_all(&fixture_root);
    std::fs::create_dir_all(fixture_root.join("sub")).unwrap();
    let root = fixture_root
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let base = format!("{root}/sub");

    let bytes = serde_json::to_vec(&json!({
        "id": "legacy-partial-ws",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-02T00:00:00Z",
        "cwd": root,
        "workspace": {
            "path_base": base,
            "workspace_root": root,
        },
    }))
    .unwrap();

    let decoded = decode_session(&bytes).unwrap();
    let ctx = captured_workspace(&decoded.session.workspace);

    // 旧字段必须保留。
    assert_eq!(ctx.workspace_root, root);
    assert_eq!(ctx.path_base, base);

    // 缺失的 identity/id/kind 必须被补全，而不是保留空默认值。
    let expected_identity = ProjectIdentity {
        initial_cwd: root.clone(),
        git_common_dir: None,
    };
    assert_eq!(
        ctx.project_identity, expected_identity,
        "缺失的 project_identity 必须被补全"
    );
    assert_eq!(ctx.worktree_kind, WorktreeKind::NonGit);
    assert!(
        !ctx.workspace_id.as_str().is_empty(),
        "缺失的 workspace_id 必须被派生补全，而非空默认值"
    );
    assert_eq!(
        ctx.workspace_id,
        WorkspaceId::derive(&expected_identity, &root),
        "补全的 workspace_id 必须为确定性派生值"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// #894 (4): legacy `cwd` 与一份已经完整（identity 自洽）的 workspace 的
/// `project_identity` 冲突时，必须返回专用 typed decode error，而不是 `Ok`，
/// 也不是笼统的 `InvalidJson`。
#[test]
fn legacy_cwd_conflicting_with_complete_workspace_identity_is_typed_error() {
    let identity = ProjectIdentity {
        initial_cwd: "/repo/one".to_string(),
        git_common_dir: None,
    };
    let root = "/repo/one".to_string();
    let complete = PersistedWorkspaceContext {
        workspace_id: WorkspaceId::derive(&identity, &root),
        project_identity: identity,
        path_base: root.clone(),
        workspace_root: root,
        worktree_kind: WorktreeKind::NonGit,
        context_stack: vec![],
    };

    let bytes = serde_json::to_vec(&json!({
        "id": "legacy-conflict",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-02T00:00:00Z",
        // 与 workspace identity.initial_cwd（/repo/one）冲突。
        "cwd": "/repo/two",
        "workspace": serde_json::to_value(&complete).unwrap(),
    }))
    .unwrap();

    match decode_session(&bytes) {
        Ok(_) => panic!(
            "legacy cwd 与已完整 workspace identity 冲突必须返回 typed decode error，而非 Ok"
        ),
        Err(SessionCodecError::InvalidJson(msg)) => {
            panic!("identity 冲突必须以专用 typed error 暴露，而非笼统 InvalidJson: {msg}")
        }
        Err(_) => {}
    }
}

/// #894 (5): 在 legacy 升级路径下（携带被捕获的 workspace/cwd），缺失的 tasks
/// 语义为「已捕获且为空」——必须记为 `CapturedEmpty` 而非 `Missing`。
#[test]
fn legacy_upgrade_records_missing_tasks_as_captured_empty() {
    let (dir, cwd) = unique_non_git_dir("tasks");

    let bytes = serde_json::to_vec(&json!({
        "id": "legacy-tasks",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-02T00:00:00Z",
        "cwd": cwd,
        "messages": [Message::user("legacy fact")],
        // 无 "tasks" 字段。
    }))
    .unwrap();

    let decoded = decode_session(&bytes).unwrap();
    assert!(decoded.upgraded_from_legacy);
    assert!(
        matches!(decoded.session.tasks, SnapshotState::CapturedEmpty),
        "升级路径下缺失 tasks 必须记为 CapturedEmpty 而非 Missing"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// #894 (7a): 真实 primary Git 仓库的 legacy 升级——即便旧 `workspace_root` 指向仓库
/// 子目录，也必须归一到 probe 出的实际 top-level，并据此派生 identity/id/kind。
#[test]
fn legacy_git_primary_backfills_and_normalizes_root_to_top_level() {
    let (repo, root) = init_git_repo("git_primary");
    let work = repo.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let work_text = canonical_text(&work);
    let common = canonical_text(&repo.join(".git"));

    // 旧 workspace 只有 path_base/workspace_root，且 workspace_root 指向子目录。
    let bytes = serde_json::to_vec(&json!({
        "id": "legacy-git-primary",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-02T00:00:00Z",
        "cwd": work_text,
        "workspace": {
            "path_base": work_text,
            "workspace_root": work_text,
        },
    }))
    .unwrap();

    let decoded = decode_session(&bytes).unwrap();
    let ctx = captured_workspace(&decoded.session.workspace);

    assert_eq!(
        ctx.workspace_root, root,
        "workspace_root 必须归一到 probe 出的 top-level，而非旧的子目录字符串"
    );
    assert_ne!(
        ctx.workspace_root, work_text,
        "归一后不应保留子目录形态的旧 workspace_root"
    );
    assert_eq!(
        ctx.path_base, work_text,
        "path_base 应保留原始 canonical 值"
    );
    assert_eq!(ctx.worktree_kind, WorktreeKind::Primary);

    let expected_identity = ProjectIdentity {
        initial_cwd: work_text.clone(),
        git_common_dir: Some(common),
    };
    assert_eq!(
        ctx.project_identity, expected_identity,
        "identity 应来自真实 probe（git_common_dir 非空）"
    );
    assert_eq!(
        ctx.workspace_id,
        WorkspaceId::derive(&expected_identity, &root),
        "workspace_id 必须据归一后的 top-level root 确定性派生"
    );
    assert!(ctx.context_stack.is_empty());

    let _ = std::fs::remove_dir_all(&repo);
}

/// #894 (7b): NonGit 非法组合——`cwd`/identity 与 `workspace_root` 指向不同的
/// NonGit 目录（initial_cwd != workspace_root）时，必须返回专用 typed error，
/// 而非静默接受一个不相关的目录。
#[test]
fn legacy_non_git_mismatched_cwd_and_root_is_typed_error() {
    let (root_dir, root) = unique_non_git_dir("nongit_root");
    let (other_dir, other) = unique_non_git_dir("nongit_other");

    let bytes = serde_json::to_vec(&json!({
        "id": "legacy-nongit-illegal",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-02T00:00:00Z",
        // cwd 指向与 workspace_root 无关的另一个 NonGit 目录。
        "cwd": other,
        "workspace": {
            "path_base": root,
            "workspace_root": root,
        },
    }))
    .unwrap();

    match decode_session(&bytes) {
        Err(SessionCodecError::LegacyWorkspaceInvalidNonGitLayout { .. }) => {}
        Ok(_) => panic!("NonGit 下 initial_cwd != workspace_root 必须被拒绝，而非 Ok"),
        Err(other) => panic!("必须是专用 NonGit 布局 typed error，实际: {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&root_dir);
    let _ = std::fs::remove_dir_all(&other_dir);
}

/// #894 (7c): fabricated / 篡改的 workspace_id——一份「完整」DTO 若 id 与其
/// identity+root 的确定性派生值不符，必须以专用 typed error 拒绝。
#[test]
fn legacy_complete_workspace_with_fabricated_id_is_rejected() {
    let (dir, root) = unique_non_git_dir("fabricated_id");

    let identity = ProjectIdentity {
        initial_cwd: root.clone(),
        git_common_dir: None,
    };
    // 除 workspace_id 外一切自洽，但 id 是伪造的。
    let tampered = PersistedWorkspaceContext {
        workspace_id: WorkspaceId::from("ws-fabricated-deadbeef"),
        project_identity: identity,
        path_base: root.clone(),
        workspace_root: root.clone(),
        worktree_kind: WorktreeKind::NonGit,
        context_stack: vec![],
    };

    let bytes = serde_json::to_vec(&json!({
        "id": "legacy-fabricated",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-02T00:00:00Z",
        "cwd": root,
        "workspace": serde_json::to_value(&tampered).unwrap(),
    }))
    .unwrap();

    match decode_session(&bytes) {
        Err(SessionCodecError::LegacyWorkspaceIdMismatch) => {}
        Ok(_) => panic!("伪造的 workspace_id 必须被拒绝，而非 Ok"),
        Err(other) => panic!("必须是 workspace_id 不一致的专用 typed error，实际: {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// #894 (7d): 一份 canonical、id 自洽的完整 NonGit DTO 必须原样接受——
/// 确认收紧后的校验没有误伤合法数据。
#[test]
fn legacy_complete_consistent_workspace_is_accepted() {
    let (dir, root) = unique_non_git_dir("complete_ok");

    let identity = ProjectIdentity {
        initial_cwd: root.clone(),
        git_common_dir: None,
    };
    let complete = PersistedWorkspaceContext {
        workspace_id: WorkspaceId::derive(&identity, &root),
        project_identity: identity,
        path_base: root.clone(),
        workspace_root: root.clone(),
        worktree_kind: WorktreeKind::NonGit,
        context_stack: vec![],
    };

    let bytes = serde_json::to_vec(&json!({
        "id": "legacy-complete-ok",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-02T00:00:00Z",
        "cwd": root,
        "workspace": serde_json::to_value(&complete).unwrap(),
    }))
    .unwrap();

    let decoded = decode_session(&bytes).unwrap();
    let ctx = captured_workspace(&decoded.session.workspace);
    assert_eq!(ctx, complete, "自洽的完整 DTO 必须原样保留");

    let _ = std::fs::remove_dir_all(&dir);
}
