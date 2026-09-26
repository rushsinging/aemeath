use super::*;

#[test]
fn project_config_location_rejects_relative_path_and_empty_identity() {
    assert_eq!(
        ProjectConfigLocationData::try_from_project_identity(PathBuf::from("relative"), b"id"),
        Err(share::error::DomainError::from(
            ProjectConfigLocationError::NotAbsolute
        ))
    );
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    assert_eq!(
        ProjectConfigLocationData::try_from_project_identity(root, b""),
        Err(share::error::DomainError::from(
            ProjectConfigLocationError::EmptyIdentity
        ))
    );
}

#[test]
fn project_config_location_is_stable_for_same_identity() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let first =
        ProjectConfigLocationData::try_from_project_identity(root.clone(), b"project").unwrap();
    let second = ProjectConfigLocationData::try_from_project_identity(root, b"project").unwrap();
    assert_eq!(first, second);
}

#[test]
fn refresh_error_folds_to_domain_error_categories() {
    let io = share::error::DomainError::from(ConfigRefreshError::Io);
    assert_eq!(io.domain(), "config");
    assert_eq!(io.category(), share::error::ErrorCategory::Storage);
    assert_eq!(io.message(), "配置源读取失败");

    let parse = share::error::DomainError::from(ConfigRefreshError::Parse);
    assert_eq!(parse.category(), share::error::ErrorCategory::Invalid);
    assert_eq!(parse.message(), "配置源解析失败");

    let invalid = share::error::DomainError::from(ConfigRefreshError::Invalid);
    assert_eq!(invalid.category(), share::error::ErrorCategory::Invalid);
    assert_eq!(invalid.message(), "配置内容非法");
}

#[test]
fn persist_error_folds_to_domain_error_categories() {
    for error in [ConfigPersistError::Io, ConfigPersistError::PermissionDenied] {
        let folded = share::error::DomainError::from(error.clone());
        assert_eq!(folded.domain(), "config");
        assert_eq!(
            folded.category(),
            share::error::ErrorCategory::Storage,
            "{error:?}"
        );
    }
    for error in [
        ConfigPersistError::Serialization,
        ConfigPersistError::UnsupportedDurability,
        ConfigPersistError::CorruptTransaction,
    ] {
        let folded = share::error::DomainError::from(error.clone());
        assert_eq!(
            folded.category(),
            share::error::ErrorCategory::Invalid,
            "{error:?}"
        );
        // 既有 ConfigUpdateError::Persist 折叠路径与直接折叠同归宿，
        // message 只多一层「配置持久化失败：」上下文前缀，不重复细分文案。
        let via_update = share::error::DomainError::from(ConfigUpdateError::Persist(error.clone()));
        assert_eq!(via_update.category(), folded.category(), "{error:?}");
        assert!(
            via_update.message().ends_with(folded.message()),
            "{:?} vs {:?}",
            via_update.message(),
            folded.message()
        );
    }
}
