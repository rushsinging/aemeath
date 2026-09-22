use super::*;

#[test]
fn project_config_location_rejects_relative_path_and_empty_identity() {
    assert_eq!(
        ProjectConfigLocation::try_from_project_identity(PathBuf::from("relative"), b"id"),
        Err(ProjectConfigLocationError::NotAbsolute)
    );
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    assert_eq!(
        ProjectConfigLocation::try_from_project_identity(root, b""),
        Err(ProjectConfigLocationError::EmptyIdentity)
    );
}

#[test]
fn project_config_location_is_stable_for_same_identity() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let first = ProjectConfigLocation::try_from_project_identity(root.clone(), b"project").unwrap();
    let second = ProjectConfigLocation::try_from_project_identity(root, b"project").unwrap();
    assert_eq!(first, second);
}
