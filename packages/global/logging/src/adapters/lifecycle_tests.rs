use super::*;

#[test]
fn rotated_path_and_validation_require_nonempty_numeric_suffix() {
    let active = Path::new("/tmp/aemeath.log");
    assert_eq!(rotated_path(active, 2), PathBuf::from("/tmp/aemeath.log.2"));
    assert!(is_rotated_log_path(Path::new("aemeath.log.2")));
    assert!(!is_rotated_log_path(Path::new("aemeath.log.")));
    assert!(!is_rotated_log_path(Path::new("aemeath.log.old")));
}

#[test]
fn backup_match_is_same_directory_same_basename_and_numeric() {
    let active = Path::new("/logs/aemeath.log");
    assert!(is_backup_of(Path::new("/logs/aemeath.log.12"), active));
    assert!(!is_backup_of(Path::new("/other/aemeath.log.12"), active));
    assert!(!is_backup_of(Path::new("/logs/other.log.12"), active));
    assert!(!is_backup_of(Path::new("/logs/aemeath.log.old"), active));
}
