//! #2160 P2：sub run 存档落盘到 `subruns/` 子空间且不进 session 枚举。

use crate::{CanonicalSession, DatasetCanonicalSessionWriter};
use share::session_types::{PersistedWorkspaceContext, ProjectIdentityData};
use storage::{DatasetKeyData, SafePathSegmentData, StorageNamespaceData};

fn identity() -> ProjectIdentityData {
    ProjectIdentityData {
        initial_cwd: "/repo".to_owned(),
        git_common_dir: None,
    }
}

fn workspace() -> PersistedWorkspaceContext {
    PersistedWorkspaceContext {
        workspace_id: share::session_types::WorkspaceId::new("wsp"),
        project_identity: identity(),
        path_base: "/repo".to_owned(),
        workspace_root: "/repo".to_owned(),
        ..Default::default()
    }
}

fn main_session(id: &str) -> CanonicalSession {
    let mut session = CanonicalSession::fixture(id);
    session.workspace = crate::domain::session::SessionSnapshotState::Captured(workspace());
    session
}

fn sub_session(id: &str, parent: &str) -> CanonicalSession {
    let mut session = main_session(id);
    session.metadata.origin = Some(crate::domain::session::SessionOrigin::SubRun);
    session.metadata.parent_session_id = Some(parent.to_owned());
    session.metadata.parent_run_id = Some("run_parent".to_owned());
    session
}

fn key(segments: &[&str]) -> DatasetKeyData {
    DatasetKeyData::new(
        StorageNamespaceData::Session,
        segments
            .iter()
            .map(|segment| {
                segment
                    .to_string()
                    .parse::<SafePathSegmentData>()
                    .expect("safe segment")
            })
            .collect(),
    )
    .expect("valid dataset key")
}

fn project_segment(session: &CanonicalSession) -> String {
    String::from(
        crate::domain::session::session_project_dir(session)
            .expect("project segment")
            .as_str(),
    )
}

#[tokio::test]
async fn subrun_session_writes_into_subruns_subspace() {
    let root = tempfile::tempdir().expect("temp root");
    let dataset = storage::wire_file_system_dataset(root.path()).expect("dataset adapter");
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let session = sub_session("ses_sub1", "ses_main");
    let project = project_segment(&session);

    writer
        .save_initial(&session)
        .await
        .expect("sub session must persist");

    // 子空间 key 可读。
    dataset
        .read_manifest(&key(&[&project, "subruns", "ses_main", "ses_sub1.dataset"]))
        .await
        .expect("subrun manifest must exist under subruns/");

    // 平铺与 project/session 两种旧布局都应为空 manifest（否则会被 session
    // 枚举误纳）。read_manifest 对不存在的 dataset 返回空成员集而非 Err。
    for legacy_key in [
        key(&[&project, "ses_sub1.dataset"]),
        key(&["ses_sub1.dataset"]),
    ] {
        let manifest = dataset
            .read_manifest(&legacy_key)
            .await
            .expect("read legacy key");
        assert!(
            manifest.members().is_empty(),
            "sub run 不应写入旧布局 {legacy_key:?}"
        );
    }
}

#[tokio::test]
async fn main_session_keeps_original_layout() {
    let root = tempfile::tempdir().expect("temp root");
    let dataset = storage::wire_file_system_dataset(root.path()).expect("dataset adapter");
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let session = main_session("ses_main");
    let project = project_segment(&session);

    writer.save_initial(&session).await.expect("main persists");

    dataset
        .read_manifest(&key(&[&project, "ses_main.dataset"]))
        .await
        .expect("main session keeps project/session layout");
}

#[tokio::test]
async fn subrun_session_excluded_from_project_list() {
    use crate::SessionManagementPort;

    let root = tempfile::tempdir().expect("temp root");
    let dataset = storage::wire_file_system_dataset(root.path()).expect("dataset adapter");
    let blob = storage::wire_file_system_blob(root.path()).expect("blob adapter");
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    writer
        .save_initial(&main_session("ses_main"))
        .await
        .expect("main persists");
    writer
        .save_initial(&sub_session("ses_sub", "ses_main"))
        .await
        .expect("sub persists");

    let management = crate::DatasetSessionManagement::new(dataset, blob);
    let listed = management
        .list_for_project(&identity())
        .await
        .expect("list for project");
    assert_eq!(listed.len(), 1, "sub run 不得出现在 session 枚举中");
    assert_eq!(listed[0].id, "ses_main");
}

#[tokio::test]
async fn deleting_parent_session_cascades_to_subruns() {
    use crate::SessionManagementPort;

    let root = tempfile::tempdir().expect("temp root");
    let dataset = storage::wire_file_system_dataset(root.path()).expect("dataset adapter");
    let blob = storage::wire_file_system_blob(root.path()).expect("blob adapter");
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let main = main_session("ses_main");
    let sub = sub_session("ses_sub", "ses_main");
    let project = project_segment(&main);
    writer.save_initial(&main).await.expect("main persists");
    writer.save_initial(&sub).await.expect("sub persists");

    let management = crate::DatasetSessionManagement::new(dataset.clone(), blob);
    management
        .delete_for_project("ses_main", &identity())
        .await
        .expect("delete main session");

    let sub_key = key(&[&project, "subruns", "ses_main", "ses_sub.dataset"]);
    let manifest = dataset.read_manifest(&sub_key).await.expect("read sub key");
    assert!(
        manifest.members().is_empty(),
        "父 session 删除必须级联清理 sub run 存档"
    );
}
