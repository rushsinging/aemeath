use std::{
    collections::VecDeque,
    fs, io,
    sync::{Arc, Condvar, Mutex},
};

use crate::contract::{
    AtomicBlobPort, ReadOutcome, SafePathSegment, StorageErrorKind, StorageKey, StorageNamespace,
    WriteOptions,
};

use super::{
    commit::{CommitHooks, StageNameSource},
    FileAtomicBlobAdapter,
};

fn key() -> StorageKey {
    StorageKey::new(
        StorageNamespace::Sessions,
        [SafePathSegment::new("session.bin").unwrap()],
    )
    .unwrap()
}

#[tokio::test]
async fn test_write_atomic_first_value_round_trips() {
    for bytes in [b"hello".as_slice(), b"".as_slice(), &[0, 0xff, 42]] {
        let temp = tempfile::tempdir().unwrap();
        let adapter = FileAtomicBlobAdapter::open(temp.path()).unwrap();
        assert_eq!(adapter.read(&key()).await.unwrap(), ReadOutcome::NotFound);

        adapter
            .write_atomic(&key(), bytes, &WriteOptions::default())
            .await
            .unwrap();

        let ReadOutcome::Found(value) = adapter.read(&key()).await.unwrap() else {
            panic!("写入后应存在 blob");
        };
        assert_eq!(value.bytes(), bytes);
    }
}

#[tokio::test]
async fn test_write_atomic_removes_transaction_stage() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = FileAtomicBlobAdapter::open(temp.path()).unwrap();
    adapter
        .write_atomic(&key(), b"value", &WriteOptions::default())
        .await
        .unwrap();

    let session_dir = temp.path().join("sessions");
    let entries = fs::read_dir(session_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(entries, ["session.bin"]);
}

struct SequenceNames(Mutex<VecDeque<String>>);

impl StageNameSource for SequenceNames {
    fn next_name(&self) -> String {
        self.0.lock().unwrap().pop_front().unwrap()
    }
}

struct FailingHook;

impl CommitHooks for FailingHook {
    fn before_replace(&self) -> io::Result<()> {
        Err(io::Error::other("injected pre-replace failure"))
    }
}

struct UnsupportedReplaceHook;

impl CommitHooks for UnsupportedReplaceHook {
    fn before_replace(&self) -> io::Result<()> {
        Ok(())
    }

    fn replace(
        &self,
        _parent: &fs::File,
        _stage: &str,
        _primary: &str,
    ) -> Result<(), rustix::io::Errno> {
        Err(rustix::io::Errno::XDEV)
    }
}

struct AncestorSwapHook {
    namespace: std::path::PathBuf,
    moved: std::path::PathBuf,
    outside: std::path::PathBuf,
}

impl CommitHooks for AncestorSwapHook {
    fn after_open_parent(&self) -> io::Result<()> {
        use std::os::unix::fs::symlink;

        fs::rename(&self.namespace, &self.moved)?;
        symlink(&self.outside, &self.namespace)
    }

    fn before_replace(&self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct CommitBarrier {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl CommitBarrier {
    fn wait_until_reached(&self) {
        let mut state = self.state.lock().unwrap();
        while !state.0 {
            state = self.changed.wait(state).unwrap();
        }
    }

    fn release(&self) {
        let mut state = self.state.lock().unwrap();
        state.1 = true;
        self.changed.notify_all();
    }
}

impl CommitHooks for CommitBarrier {
    fn before_replace(&self) -> io::Result<()> {
        Ok(())
    }

    fn after_replace(&self) -> io::Result<()> {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.changed.notify_all();
        while !state.1 {
            state = self.changed.wait(state).unwrap();
        }
        Ok(())
    }
}

#[tokio::test]
async fn test_stage_collision_never_truncates_existing_file() {
    let temp = tempfile::tempdir().unwrap();
    let session_dir = temp.path().join("sessions");
    fs::create_dir(&session_dir).unwrap();
    fs::write(session_dir.join(".collision"), b"attacker").unwrap();
    let names = Arc::new(SequenceNames(Mutex::new(VecDeque::from([
        ".collision".to_owned(),
        ".collision".to_owned(),
        ".fresh".to_owned(),
    ]))));
    let adapter = FileAtomicBlobAdapter::with_test_seams(
        temp.path(),
        names,
        Arc::new(super::commit::NoopCommitHooks),
    )
    .unwrap();

    adapter
        .write_atomic(&key(), b"value", &WriteOptions::default())
        .await
        .unwrap();

    assert_eq!(
        fs::read(session_dir.join(".collision")).unwrap(),
        b"attacker"
    );
    assert_eq!(fs::read(session_dir.join("session.bin")).unwrap(), b"value");
}

#[tokio::test]
async fn test_failure_before_replace_preserves_primary() {
    let temp = tempfile::tempdir().unwrap();
    let initial = FileAtomicBlobAdapter::open(temp.path()).unwrap();
    initial
        .write_atomic(&key(), b"old", &WriteOptions::default())
        .await
        .unwrap();
    let adapter = FileAtomicBlobAdapter::with_test_seams(
        temp.path(),
        Arc::new(SequenceNames(Mutex::new(VecDeque::from([
            ".failure".to_owned()
        ])))),
        Arc::new(FailingHook),
    )
    .unwrap();

    let error = adapter
        .write_atomic(&key(), b"new", &WriteOptions::default())
        .await
        .unwrap_err();

    assert_eq!(error.kind(), StorageErrorKind::Io);
    assert_eq!(
        fs::read(temp.path().join("sessions/session.bin")).unwrap(),
        b"old"
    );
    assert!(!temp.path().join("sessions/.failure").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_overwrite_exposes_only_complete_old_or_new_value() {
    let temp = tempfile::tempdir().unwrap();
    let initial = FileAtomicBlobAdapter::open(temp.path()).unwrap();
    let old = vec![0x11; 64 * 1024];
    let new = vec![0x22; 96 * 1024];
    initial
        .write_atomic(&key(), &old, &WriteOptions::default())
        .await
        .unwrap();

    let barrier = Arc::new(CommitBarrier::default());
    let writer = FileAtomicBlobAdapter::with_test_seams(
        temp.path(),
        Arc::new(SequenceNames(Mutex::new(VecDeque::from([
            ".overwrite".to_owned()
        ])))),
        barrier.clone(),
    )
    .unwrap();
    let read_adapter = initial.clone();
    let write_key = key();
    let new_for_write = new.clone();
    let write = tokio::spawn(async move {
        writer
            .write_atomic(&write_key, &new_for_write, &WriteOptions::default())
            .await
            .unwrap();
    });

    let wait_barrier = barrier.clone();
    tokio::task::spawn_blocking(move || wait_barrier.wait_until_reached())
        .await
        .unwrap();
    for _ in 0..32 {
        let ReadOutcome::Found(value) = read_adapter.read(&key()).await.unwrap() else {
            panic!("覆盖期间 primary 不得消失");
        };
        assert!(value.bytes() == old.as_slice() || value.bytes() == new.as_slice());
    }
    barrier.release();
    write.await.unwrap();

    let ReadOutcome::Found(value) = read_adapter.read(&key()).await.unwrap() else {
        panic!("覆盖后 primary 应存在");
    };
    assert_eq!(value.bytes(), new.as_slice());
}

#[tokio::test]
async fn test_unsupported_atomic_replace_is_structured_and_preserves_primary() {
    let temp = tempfile::tempdir().unwrap();
    let initial = FileAtomicBlobAdapter::open(temp.path()).unwrap();
    initial
        .write_atomic(&key(), b"old", &WriteOptions::default())
        .await
        .unwrap();
    let adapter = FileAtomicBlobAdapter::with_test_seams(
        temp.path(),
        Arc::new(SequenceNames(Mutex::new(VecDeque::from([
            ".unsupported".to_owned()
        ])))),
        Arc::new(UnsupportedReplaceHook),
    )
    .unwrap();

    let error = adapter
        .write_atomic(&key(), b"new", &WriteOptions::default())
        .await
        .unwrap_err();

    assert_eq!(error.kind(), StorageErrorKind::UnsupportedAtomicReplace);
    assert_eq!(
        fs::read(temp.path().join("sessions/session.bin")).unwrap(),
        b"old"
    );
    assert!(!temp.path().join("sessions/.unsupported").exists());
}

#[tokio::test]
async fn test_nested_storage_key_round_trips() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = FileAtomicBlobAdapter::open(temp.path()).unwrap();
    let nested = StorageKey::new(
        StorageNamespace::Memory,
        [
            SafePathSegment::new("project").unwrap(),
            SafePathSegment::new("memory.bin").unwrap(),
        ],
    )
    .unwrap();

    adapter
        .write_atomic(&nested, b"nested", &WriteOptions::default())
        .await
        .unwrap();

    let ReadOutcome::Found(value) = adapter.read(&nested).await.unwrap() else {
        panic!("嵌套 key 应能读回");
    };
    assert_eq!(value.bytes(), b"nested");
}

#[cfg(unix)]
#[tokio::test]
async fn test_target_symlink_is_never_followed() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(outside.path(), b"outside").unwrap();
    let session_dir = temp.path().join("sessions");
    fs::create_dir(&session_dir).unwrap();
    symlink(outside.path(), session_dir.join("session.bin")).unwrap();
    let adapter = FileAtomicBlobAdapter::open(temp.path()).unwrap();

    let read_error = adapter.read(&key()).await.unwrap_err();
    assert_eq!(read_error.kind(), StorageErrorKind::UnsafeFilesystemEntry);
    let write_error = adapter
        .write_atomic(&key(), b"new", &WriteOptions::default())
        .await
        .unwrap_err();
    assert_eq!(write_error.kind(), StorageErrorKind::UnsafeFilesystemEntry);
    assert_eq!(fs::read(outside.path()).unwrap(), b"outside");
}

#[cfg(unix)]
#[tokio::test]
async fn test_special_primary_is_rejected_without_blocking() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let temp = tempfile::tempdir().unwrap();
    let session_dir = temp.path().join("sessions");
    fs::create_dir(&session_dir).unwrap();
    let fifo = session_dir.join("session.bin");
    let fifo_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    let result = unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) };
    assert_eq!(result, 0, "mkfifo 应成功");
    let adapter = FileAtomicBlobAdapter::open(temp.path()).unwrap();

    let error = tokio::time::timeout(std::time::Duration::from_secs(1), adapter.read(&key()))
        .await
        .expect("读取 FIFO 不得阻塞")
        .unwrap_err();

    assert_eq!(error.kind(), StorageErrorKind::UnsafeFilesystemEntry);
}

#[cfg(unix)]
#[tokio::test]
async fn test_ancestor_swap_cannot_escape_capability_root() {
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("sessions")).unwrap();
    let adapter = FileAtomicBlobAdapter::with_test_seams(
        temp.path(),
        Arc::new(SequenceNames(Mutex::new(VecDeque::from([
            ".ancestor-swap".to_owned(),
        ])))),
        Arc::new(AncestorSwapHook {
            namespace: temp.path().join("sessions"),
            moved: temp.path().join("sessions-old"),
            outside: outside.path().to_owned(),
        }),
    )
    .unwrap();

    adapter
        .write_atomic(&key(), b"new", &WriteOptions::default())
        .await
        .unwrap();

    assert!(!outside.path().join("session.bin").exists());
    assert_eq!(
        fs::read(temp.path().join("sessions-old/session.bin")).unwrap(),
        b"new"
    );
}
