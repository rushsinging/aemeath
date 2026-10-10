//! `SafeStorageDir::remove_file` 聚焦测试：普通文件可删、符号链接拒收。
use std::os::unix::fs::symlink;
use std::str::FromStr;

use super::*;

fn segment(value: &str) -> SafePathSegmentData {
    SafePathSegmentData::from_str(value).expect("test segment name is safe")
}

#[test]
fn remove_file_deletes_regular_file_and_reports_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = SafeStorageRoot::open(temp.path()).expect("root");
    let dir = root.ensure_dir(&[segment("events")]).expect("dir");
    let name = segment("2026-10-10.jsonl");
    dir.create_or_open(
        &name,
        SafeOpenOptions {
            read: true,
            append: true,
        },
    )
    .expect("create");

    dir.remove_file(&name).expect("remove existing file");

    let missing = dir.open_existing(
        &name,
        SafeOpenOptions {
            read: true,
            append: false,
        },
    );
    assert!(missing.is_err(), "file must be gone after remove_file");
}

#[cfg(unix)]
#[test]
fn remove_file_rejects_symlink_and_keeps_target() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = SafeStorageRoot::open(temp.path()).expect("root");
    let dir = root.ensure_dir(&[segment("events")]).expect("dir");
    let target = temp.path().join("target.txt");
    std::fs::write(&target, b"keep me").expect("target file");
    let link = temp.path().join("events").join("link.jsonl");
    symlink(&target, &link).expect("symlink");

    let error = dir
        .remove_file(&segment("link.jsonl"))
        .expect_err("symlink must be rejected");

    assert_eq!(error.kind(), &StorageErrorKind::InvalidKey);
    assert!(target.exists(), "symlink rejection must not touch target");
}
