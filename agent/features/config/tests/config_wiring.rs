use std::sync::Arc;

use config::{ConfigWriter, NativeConfigStore};

#[tokio::test]
async fn wiring_reads_runtime_override_from_injected_native_store() {
    let project = tempfile::tempdir().expect("create project directory");
    let storage = tempfile::tempdir().expect("create override storage directory");
    let store = NativeConfigStore::new(Arc::new(
        storage::FileSystemBlobAdapter::new(storage.path()).expect("create override blob"),
    ));

    let first = config::wire_project_config(project.path(), store.clone())
        .await
        .expect("wire config with injected store");
    first
        .service()
        .update(config::ConfigUpdate::SetPermissionMode {
            mode: share::config::PermissionModeConfig::AllowAll,
        })
        .await
        .expect("persist override");

    let rebuilt = config::wire_project_config(project.path(), store)
        .await
        .expect("rebuild config with injected store");
    assert_eq!(
        rebuilt.reader().committed_snapshot().permission_mode(),
        share::config::PermissionModeConfig::AllowAll
    );
}
