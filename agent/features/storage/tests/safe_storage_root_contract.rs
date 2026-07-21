use std::io::{Read, Write};
use std::str::FromStr;

use storage::{
    SafeOpenOptions, SafePathSegment, SafeStorageFileType, SafeStorageRoot, StorageErrorKind,
};

fn segment(value: &str) -> SafePathSegment {
    SafePathSegment::from_str(value).expect("safe path segment")
}

#[test]
fn public_safe_path_segment_rejects_unsafe_components_before_io() {
    for value in ["", ".", "..", ".hidden", "/tmp", "a/b", "a\\b", "a\0b"] {
        assert!(
            SafePathSegment::from_str(value).is_err(),
            "unsafe segment must be rejected: {value:?}"
        );
    }
}

#[test]
fn open_creates_missing_root_and_ensure_dir_is_idempotent() {
    let parent = tempfile::tempdir().expect("parent tempdir");
    let missing = parent.path().join("missing-root");
    let root = SafeStorageRoot::open(&missing).expect("missing root must be created");

    root.ensure_dir(&[segment("usage"), segment("daily")])
        .expect("nested directory creation");
    root.ensure_dir(&[segment("usage"), segment("daily")])
        .expect("existing nested directory must be accepted");

    assert!(missing.join("usage/daily").is_dir());
}

#[test]
fn open_when_parent_is_regular_file_returns_io_without_path_leak() {
    let parent = tempfile::NamedTempFile::new().expect("parent file");
    let rejected_path = parent.path().join("child");
    let error = match SafeStorageRoot::open(&rejected_path) {
        Ok(_) => panic!("root below a regular file must fail"),
        Err(error) => error,
    };

    assert_eq!(error.kind(), &StorageErrorKind::Io);
    assert!(
        !error
            .to_string()
            .contains(&rejected_path.to_string_lossy().to_string()),
        "error must not leak the rejected absolute path: {error}"
    );
}

#[test]
fn create_or_open_and_open_existing_preserve_bytes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = SafeStorageRoot::open(temp.path()).expect("root");
    let dir = root.ensure_dir(&[segment("usage")]).expect("usage dir");
    let file = segment("session.jsonl");

    let mut created = dir
        .create_or_open(
            &file,
            SafeOpenOptions {
                read: true,
                append: true,
            },
        )
        .expect("create regular file");
    created.write_all(b"fact\n").expect("append fact");
    drop(created);

    let mut opened = dir
        .open_existing(
            &file,
            SafeOpenOptions {
                read: true,
                append: false,
            },
        )
        .expect("open existing regular file");
    let mut bytes = Vec::new();
    opened.read_to_end(&mut bytes).expect("read fact");
    assert_eq!(bytes, b"fact\n");
}

#[test]
fn open_existing_when_missing_returns_io_without_creating_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = SafeStorageRoot::open(temp.path()).expect("root");
    let dir = root.ensure_dir(&[segment("usage")]).expect("usage dir");
    let file = segment("missing.jsonl");

    let error = dir
        .open_existing(
            &file,
            SafeOpenOptions {
                read: true,
                append: false,
            },
        )
        .expect_err("missing file must not be created");

    assert_eq!(error.kind(), &StorageErrorKind::Io);
    assert!(!temp.path().join("usage/missing.jsonl").exists());
}

#[test]
fn open_existing_rejects_directory_target() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join("usage/not-a-file")).expect("directory target");
    let root = SafeStorageRoot::open(temp.path()).expect("root");
    let dir = root.ensure_dir(&[segment("usage")]).expect("usage dir");

    let error = dir
        .open_existing(
            &segment("not-a-file"),
            SafeOpenOptions {
                read: true,
                append: false,
            },
        )
        .expect_err("directory must not be opened as a regular file");
    assert_eq!(error.kind(), &StorageErrorKind::InvalidKey);
}

#[test]
fn entries_are_typed_sorted_and_skip_unsafe_names() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join("usage/z-dir")).expect("directory entry");
    std::fs::write(temp.path().join("usage/b.jsonl"), b"b").expect("b entry");
    std::fs::write(temp.path().join("usage/a.jsonl"), b"a").expect("a entry");
    std::fs::write(temp.path().join("usage/.hidden"), b"hidden").expect("unsafe entry");

    let root = SafeStorageRoot::open(temp.path()).expect("root");
    let dir = root.ensure_dir(&[segment("usage")]).expect("usage dir");
    let entries = dir.entries().expect("entries");
    let observed = entries
        .iter()
        .map(|entry| (entry.name().as_str(), entry.file_type()))
        .collect::<Vec<_>>();

    assert_eq!(
        observed,
        vec![
            ("a.jsonl", SafeStorageFileType::RegularFile),
            ("b.jsonl", SafeStorageFileType::RegularFile),
            ("z-dir", SafeStorageFileType::Directory),
        ]
    );
}

#[cfg(unix)]
#[test]
fn ensure_dir_rejects_intermediate_symlink_without_touching_outside() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("outside tempdir");
    symlink(outside.path(), temp.path().join("usage")).expect("directory symlink");
    let root = SafeStorageRoot::open(temp.path()).expect("root");

    let error = match root.ensure_dir(&[segment("usage"), segment("daily")]) {
        Ok(_) => panic!("intermediate symlink must fail closed"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), &StorageErrorKind::InvalidKey);
    assert!(!outside.path().join("daily").exists());
}

#[cfg(unix)]
#[test]
fn regular_file_operations_reject_symlinks_and_entries_skip_them() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::NamedTempFile::new().expect("outside");
    std::fs::create_dir_all(temp.path().join("usage")).expect("usage dir");
    symlink(outside.path(), temp.path().join("usage/session.jsonl")).expect("symlink");

    let root = SafeStorageRoot::open(temp.path()).expect("root");
    let dir = root.ensure_dir(&[segment("usage")]).expect("usage dir");
    let file = segment("session.jsonl");
    let error = dir
        .create_or_open(
            &file,
            SafeOpenOptions {
                read: true,
                append: true,
            },
        )
        .expect_err("symlink must fail closed");

    assert_eq!(error.kind(), &StorageErrorKind::InvalidKey);
    assert!(dir.entries().expect("entries").is_empty());
}
