use super::*;
use crate::adapters::AtomicBlobSessionManagement;
use crate::domain::session::{CanonicalSession, SessionCodec, SessionMetadata, SnapshotState};
use crate::ports::{SessionManagementPort, SessionSnapshotStore};
use share::session_types::{PersistedWorkspaceContext, ProjectIdentity};
use std::sync::Arc;
use storage::{file_system_blob, AtomicBlobPort, StorageNamespace};

fn captured_session(id: &str, common_dir: &str) -> CanonicalSession {
    let identity = ProjectIdentity {
        initial_cwd: format!("/repos/work-{id}"),
        git_common_dir: Some(common_dir.to_string()),
    };
    CanonicalSession {
        id: id.to_string(),
        chats: Vec::new(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-02T00:00:00Z".to_string(),
        metadata: SessionMetadata::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Captured(PersistedWorkspaceContext {
            workspace_id: Default::default(),
            project_identity: identity,
            path_base: format!("/repos/work-{id}"),
            workspace_root: format!("/repos/work-{id}"),
            worktree_kind: Default::default(),
            context_stack: Vec::new(),
        }),
        revision: 1,
        compact: None,
        cleared_after: None,
        run_slices: Default::default(),
        committed_steps: Default::default(),
        skill_load_records: Vec::new(),
    }
}

fn temp_blob() -> Arc<dyn AtomicBlobPort> {
    let root = std::env::temp_dir().join(format!(
        "aemeath-flat-session-migration-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let root_clone = root.clone();
    std::thread::spawn(move || {
        // 进程退出前清理由测试末尾完成；此处仅兜底延迟删除。
        std::thread::sleep(std::time::Duration::from_secs(120));
        let _ = std::fs::remove_dir_all(&root_clone);
    });
    file_system_blob(&root).expect("blob adapter init")
}

/// 以旧版 `<id>.json` 平铺形态写入一个 session。
async fn write_flat_json_session(blob: &Arc<dyn AtomicBlobPort>, session: &CanonicalSession) {
    let store = AtomicBlobSessionStore::from_key_segments(
        Arc::clone(blob),
        vec![format!("{}.json", session.id)],
    )
    .unwrap();
    let bytes = SessionCodec::encode(session).unwrap();
    store
        .write(&bytes)
        .await
        .expect("flat json write must succeed");
}

/// 以当前主形态的裸 `<id>` 平铺写入一个 session。
async fn write_flat_bare_session(blob: &Arc<dyn AtomicBlobPort>, session: &CanonicalSession) {
    let store =
        AtomicBlobSessionStore::from_key_segments(Arc::clone(blob), vec![session.id.clone()])
            .unwrap();
    let bytes = SessionCodec::encode(session).unwrap();
    store
        .write(&bytes)
        .await
        .expect("flat bare write must succeed");
}

/// 生产主形态：裸 `<id>` 平铺文件迁入 project 目录后，本项目 list 可见、
/// 跨项目不可见。
#[tokio::test(flavor = "current_thread")]
async fn bare_id_flat_sessions_move_into_project_dirs_and_become_listable() {
    let blob = temp_blob();
    let first = captured_session("sess-plain-a", "/repos/alpha/.git");
    let second = captured_session("sess-plain-b", "/repos/beta/.git");
    write_flat_bare_session(&blob, &first).await;
    write_flat_bare_session(&blob, &second).await;

    let report = migrate_flat_sessions_to_project_dirs(Arc::clone(&blob)).await;
    assert_eq!(
        report,
        FlatSessionMigrationReport {
            migrated: 2,
            skipped: 0
        }
    );

    let entries = blob.list_primary(StorageNamespace::Session).await.unwrap();
    assert!(
        entries
            .iter()
            .all(|entry| entry.key().segments().len() == 2),
        "迁移后不应残留单段平铺 key"
    );

    let alpha_identity = ProjectIdentity {
        initial_cwd: "/repos/work-sess-plain-a".to_string(),
        git_common_dir: Some("/repos/alpha/.git".to_string()),
    };
    let management = AtomicBlobSessionManagement::new(Arc::clone(&blob));
    let listed = management.list_for_project(&alpha_identity).await.unwrap();
    assert_eq!(listed.len(), 1, "本项目只看到本项目 session：{listed:?}");
    assert_eq!(listed[0].id, "sess-plain-a");
}

#[tokio::test(flavor = "current_thread")]
async fn flat_json_sessions_move_into_project_dirs_and_become_listable() {
    let blob = temp_blob();
    let first = captured_session("sess-a", "/repos/alpha/.git");
    let second = captured_session("sess-b", "/repos/beta/.git");
    write_flat_json_session(&blob, &first).await;
    write_flat_json_session(&blob, &second).await;

    let report = migrate_flat_sessions_to_project_dirs(Arc::clone(&blob)).await;
    assert_eq!(
        report,
        FlatSessionMigrationReport {
            migrated: 2,
            skipped: 0
        }
    );

    // 平铺文件消失；本项目目录里能列出对应 session。
    let entries = blob.list_primary(StorageNamespace::Session).await.unwrap();
    assert!(
        entries
            .iter()
            .all(|entry| entry.key().segments().len() == 2),
        "迁移后不应残留单段平铺 key：{:?}",
        entries
            .iter()
            .map(|entry| entry.key().segments().to_vec())
            .collect::<Vec<_>>()
    );

    let alpha_identity = ProjectIdentity {
        initial_cwd: "/repos/work-sess-a".to_string(),
        git_common_dir: Some("/repos/alpha/.git".to_string()),
    };
    let management = AtomicBlobSessionManagement::new(Arc::clone(&blob));
    let listed = management.list_for_project(&alpha_identity).await.unwrap();
    assert_eq!(
        listed.len(),
        1,
        "本项目 list 只能看到本项目 session：{listed:?}"
    );
    assert_eq!(listed[0].id, "sess-a");
}

#[tokio::test(flavor = "current_thread")]
async fn migration_rerun_is_idempotent() {
    let blob = temp_blob();
    let session = captured_session("sess-once", "/repos/alpha/.git");
    write_flat_json_session(&blob, &session).await;

    let first_run = migrate_flat_sessions_to_project_dirs(Arc::clone(&blob)).await;
    assert_eq!(first_run.migrated, 1);
    let second_run = migrate_flat_sessions_to_project_dirs(Arc::clone(&blob)).await;
    assert_eq!(
        second_run,
        FlatSessionMigrationReport::default(),
        "重跑不应重复迁移：{second_run:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn undecodable_flat_file_is_skipped_without_aborting_migration() {
    let blob = temp_blob();
    let good = captured_session("sess-good", "/repos/alpha/.git");
    write_flat_json_session(&blob, &good).await;
    let corrupt_store = AtomicBlobSessionStore::from_key_segments(
        Arc::clone(&blob),
        vec!["sess-bad.json".to_string()],
    )
    .unwrap();
    corrupt_store.write(b"not a session payload").await.unwrap();

    let report = migrate_flat_sessions_to_project_dirs(Arc::clone(&blob)).await;
    assert_eq!(report.migrated, 1, "可解码的必须被迁移");
    assert_eq!(report.skipped, 1, "垃圾文件计入 skipped 且不中断");
}
