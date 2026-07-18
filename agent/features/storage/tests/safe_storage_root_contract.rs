use std::str::FromStr;

use storage::{SafeOpenOptions, SafePathSegment, SafeStorageFileType, SafeStorageRoot};

#[test]
fn safe_storage_root_rejects_unsafe_segments_before_io() {
    for value in ["", ".", "..", ".hidden", "a/b", "a\\b"] {
        assert!(SafePathSegment::from_str(value).is_err(), "unsafe: {value}");
    }
}

#[test]
fn safe_storage_root_opens_regular_files_and_lists_typed_entries() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = SafeStorageRoot::open(temp.path()).expect("root");
    let usage = SafePathSegment::from_str("usage").expect("safe namespace");
    let dir = root.ensure_dir(&[usage]).expect("usage dir");
    let file = SafePathSegment::from_str("session.jsonl").expect("safe file");

    dir.create_or_open(
        &file,
        SafeOpenOptions {
            read: true,
            append: true,
        },
    )
    .expect("regular file");

    let entries = dir.entries().expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name(), &file);
    assert_eq!(entries[0].file_type(), SafeStorageFileType::RegularFile);
}

#[cfg(unix)]
#[test]
fn safe_storage_root_rejects_symlink_files() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::NamedTempFile::new().expect("outside");
    std::fs::create_dir_all(temp.path().join("usage")).expect("usage dir");
    symlink(outside.path(), temp.path().join("usage/session.jsonl")).expect("symlink");

    let root = SafeStorageRoot::open(temp.path()).expect("root");
    let usage = SafePathSegment::from_str("usage").expect("safe namespace");
    let dir = root.ensure_dir(&[usage]).expect("usage dir");
    let file = SafePathSegment::from_str("session.jsonl").expect("safe file");
    let error = dir
        .create_or_open(
            &file,
            SafeOpenOptions {
                read: true,
                append: true,
            },
        )
        .expect_err("symlink must fail closed");

    assert_eq!(error.kind(), &storage::StorageErrorKind::InvalidKey);
}
