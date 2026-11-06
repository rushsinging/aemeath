use config::wire_config_override_store;

#[tokio::test]
async fn wiring_reads_runtime_override_from_injected_native_store() {
    let project = tempfile::tempdir().expect("create project directory");
    let storage = tempfile::tempdir().expect("create override storage directory");
    let store = wire_config_override_store(
        storage::wire_file_system_blob(storage.path()).expect("create override blob"),
    );

    let first = config::wire_project_config(project.path(), store.clone())
        .await
        .expect("wire config with injected store");
    // #1866 B3：service() 已删——测试经 writer() 行为面写入。
    first
        .writer()
        .update(config::ConfigUpdateData::SetPermissionMode {
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
