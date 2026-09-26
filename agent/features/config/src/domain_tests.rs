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
