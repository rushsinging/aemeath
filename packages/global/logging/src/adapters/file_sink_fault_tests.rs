use super::*;
use crate::adapters::lifecycle::{
    FileMetadata, FileOps, MonotonicClock, SinkState, SinkWriter, RECOVERY_INTERVAL,
};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Barrier, Condvar};
use std::thread;
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Open,
    Metadata,
    Exists,
    Remove,
    Rename,
    ReadDir,
    Write,
    Flush,
}

#[derive(Default)]
struct Script {
    failures: Mutex<VecDeque<Op>>,
    calls: Mutex<Vec<Op>>,
    metadata: Mutex<HashMap<PathBuf, FileMetadata>>,
    entries: Mutex<Vec<PathBuf>>,
    output: Mutex<Vec<u8>>,
}

impl Script {
    fn fail_next(&self, op: Op) {
        self.failures.lock().unwrap().push_back(op);
    }

    fn call(&self, op: Op) -> io::Result<()> {
        self.calls.lock().unwrap().push(op);
        let mut failures = self.failures.lock().unwrap();
        if failures.front() == Some(&op) {
            failures.pop_front();
            Err(io::Error::other(format!("injected {op:?}")))
        } else {
            Ok(())
        }
    }

    fn calls(&self) -> Vec<Op> {
        self.calls.lock().unwrap().clone()
    }
}

struct ScriptWriter(Arc<Script>);

impl SinkWriter for ScriptWriter {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0.call(Op::Write)?;
        self.0.output.lock().unwrap().extend_from_slice(bytes);
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.call(Op::Flush)
    }
}

impl FileOps for Script {
    fn open(&self, _path: &Path) -> io::Result<Box<dyn SinkWriter>> {
        self.call(Op::Open)?;
        // Tests hold the same script in an Arc and only construct through `fixture`.
        Err(io::Error::other("direct Script::open must be wrapped"))
    }

    fn metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        self.call(Op::Metadata)?;
        self.metadata
            .lock()
            .unwrap()
            .get(path)
            .copied()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing"))
    }

    fn exists(&self, path: &Path) -> io::Result<bool> {
        self.call(Op::Exists)?;
        Ok(self.metadata.lock().unwrap().contains_key(path))
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        self.call(Op::Remove)?;
        self.metadata.lock().unwrap().remove(path);
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.call(Op::Rename)?;
        let value = self.metadata.lock().unwrap().remove(from);
        if let Some(value) = value {
            self.metadata
                .lock()
                .unwrap()
                .insert(to.to_path_buf(), value);
        }
        Ok(())
    }

    fn read_dir(&self, _path: &Path) -> io::Result<Vec<PathBuf>> {
        self.call(Op::ReadDir)?;
        Ok(self.entries.lock().unwrap().clone())
    }
}

/// Arc-aware adapter so `open` can return a writer sharing the script.
struct Files(Arc<Script>);

impl FileOps for Files {
    fn open(&self, path: &Path) -> io::Result<Box<dyn SinkWriter>> {
        self.0.call(Op::Open)?;
        self.0
            .metadata
            .lock()
            .unwrap()
            .entry(path.to_path_buf())
            .or_insert_with(|| regular(0));
        Ok(Box::new(ScriptWriter(self.0.clone())))
    }
    fn metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        FileOps::metadata(&*self.0, path)
    }
    fn exists(&self, path: &Path) -> io::Result<bool> {
        FileOps::exists(&*self.0, path)
    }
    fn remove(&self, path: &Path) -> io::Result<()> {
        FileOps::remove(&*self.0, path)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        FileOps::rename(&*self.0, from, to)
    }
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        FileOps::read_dir(&*self.0, path)
    }
}

#[derive(Default)]
struct Clock(AtomicU64);
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}

#[derive(Default)]
struct Emergency(Mutex<Vec<String>>);
impl EmergencyWriter for Emergency {
    fn write(&self, message: &str) {
        self.0.lock().unwrap().push(message.to_string());
    }
}

fn regular(len: u64) -> FileMetadata {
    FileMetadata {
        len,
        is_file: true,
        is_symlink: false,
        modified: SystemTime::now(),
    }
}

fn fixture(
    script: Arc<Script>,
    clock: Arc<Clock>,
    emergency: Arc<Emergency>,
    max_bytes: u64,
    max_backups: usize,
    retention_days: u64,
) -> FileSinkLifecycle {
    FileSinkLifecycle::start(
        PathBuf::from("/logs/aemeath.log"),
        max_bytes,
        max_backups,
        retention_days,
        Arc::new(Files(script)),
        clock,
        emergency,
    )
}

#[test]
fn startup_open_failure_degrades_and_falls_back() {
    let script = Arc::new(Script::default());
    script.fail_next(Op::Open);
    let emergency = Arc::new(Emergency::default());
    let mut sink = fixture(
        script,
        Arc::new(Clock::default()),
        emergency.clone(),
        100,
        1,
        0,
    );
    assert_eq!(sink.state(), SinkState::Degraded);
    sink.write_line("record");
    assert!(emergency
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|line| line == "record"));
}

#[test]
fn write_and_implicit_flush_failures_degrade() {
    for operation in [Op::Write, Op::Flush] {
        let script = Arc::new(Script::default());
        let emergency = Arc::new(Emergency::default());
        let mut sink = fixture(
            script.clone(),
            Arc::new(Clock::default()),
            emergency.clone(),
            100,
            1,
            0,
        );
        script.fail_next(operation);
        sink.write_line("record");
        assert_eq!(sink.state(), SinkState::Degraded);
        assert!(emergency
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|line| line == "record"));
    }
}

#[test]
fn explicit_flush_failure_is_observable_and_degrades() {
    let script = Arc::new(Script::default());
    let emergency = Arc::new(Emergency::default());
    let mut sink = fixture(
        script.clone(),
        Arc::new(Clock::default()),
        emergency.clone(),
        100,
        1,
        0,
    );
    script.fail_next(Op::Flush);
    sink.flush();
    assert_eq!(sink.state(), SinkState::Degraded);
    assert!(emergency.0.lock().unwrap()[0].contains("explicit flush"));
}

#[test]
fn recovery_is_lazy_fixed_at_five_seconds_and_reopen_can_fail() {
    let script = Arc::new(Script::default());
    script.fail_next(Op::Open);
    let clock = Arc::new(Clock::default());
    let emergency = Arc::new(Emergency::default());
    let mut sink = fixture(script.clone(), clock.clone(), emergency, 100, 1, 0);
    let opens = || script.calls().iter().filter(|op| **op == Op::Open).count();
    sink.write_line("before deadline");
    assert_eq!(opens(), 1);
    clock.0.store(RECOVERY_INTERVAL.as_secs(), Ordering::SeqCst);
    script.fail_next(Op::Open);
    sink.write_line("first retry");
    assert_eq!(sink.state(), SinkState::Degraded);
    assert_eq!(opens(), 2);
    clock.0.store(9, Ordering::SeqCst);
    sink.write_line("still throttled");
    assert_eq!(opens(), 2);
    clock.0.store(10, Ordering::SeqCst);
    sink.write_line("recovered");
    assert_eq!(sink.state(), SinkState::Healthy);
    assert_eq!(opens(), 3);
}

#[test]
fn metadata_failure_stops_rotation_and_falls_back() {
    let script = Arc::new(Script::default());
    let emergency = Arc::new(Emergency::default());
    let mut sink = fixture(
        script.clone(),
        Arc::new(Clock::default()),
        emergency,
        1,
        1,
        0,
    );
    script.fail_next(Op::Metadata);
    sink.write_line("record");
    assert_eq!(sink.state(), SinkState::Degraded);
    assert!(!script.calls().contains(&Op::Rename));
}

#[test]
fn backup_existence_error_is_not_treated_as_absent() {
    let script = Arc::new(Script::default());
    script
        .metadata
        .lock()
        .unwrap()
        .insert(PathBuf::from("/logs/aemeath.log"), regular(2));
    let mut sink = fixture(
        script.clone(),
        Arc::new(Clock::default()),
        Arc::new(Emergency::default()),
        1,
        2,
        0,
    );
    // Startup already rotated; make the active large again.
    script
        .metadata
        .lock()
        .unwrap()
        .insert(PathBuf::from("/logs/aemeath.log"), regular(2));
    script.fail_next(Op::Exists);
    sink.write_line("record");
    assert_eq!(sink.state(), SinkState::Degraded);
    assert_eq!(script.calls().last(), Some(&Op::Exists));
}

#[test]
fn rotation_remove_rename_and_reopen_failures_are_observable() {
    for operation in [Op::Remove, Op::Rename, Op::Open] {
        let script = Arc::new(Script::default());
        let mut sink = fixture(
            script.clone(),
            Arc::new(Clock::default()),
            Arc::new(Emergency::default()),
            1,
            1,
            0,
        );
        let active = PathBuf::from("/logs/aemeath.log");
        script.metadata.lock().unwrap().insert(active, regular(2));
        if operation == Op::Remove {
            script
                .metadata
                .lock()
                .unwrap()
                .insert(PathBuf::from("/logs/aemeath.log.1"), regular(2));
        }
        script.fail_next(operation);
        sink.write_line("record");
        assert_eq!(sink.state(), SinkState::Degraded, "operation {operation:?}");
    }
}

#[test]
fn retention_remove_failure_is_reported_and_only_legal_regular_backup_is_considered() {
    let script = Arc::new(Script::default());
    let legal = PathBuf::from("/logs/aemeath.log.1");
    let other = PathBuf::from("/logs/other.log.1");
    let symlink = PathBuf::from("/logs/aemeath.log.2");
    *script.entries.lock().unwrap() = vec![legal.clone(), other, symlink.clone()];
    let old = SystemTime::now() - Duration::from_secs(2 * 86_400);
    script.metadata.lock().unwrap().insert(
        legal,
        FileMetadata {
            modified: old,
            ..regular(1)
        },
    );
    script.metadata.lock().unwrap().insert(
        symlink,
        FileMetadata {
            is_file: false,
            is_symlink: true,
            modified: old,
            ..regular(1)
        },
    );
    script.fail_next(Op::Remove);
    let emergency = Arc::new(Emergency::default());
    let sink = fixture(
        script.clone(),
        Arc::new(Clock::default()),
        emergency.clone(),
        100,
        1,
        1,
    );
    assert_eq!(sink.state(), SinkState::Healthy);
    assert_eq!(
        script
            .calls()
            .iter()
            .filter(|op| **op == Op::Remove)
            .count(),
        1
    );
    assert!(emergency.0.lock().unwrap()[0].contains("retention remove"));
}

struct BlockingWriter {
    entered: Arc<Barrier>,
    release: Arc<(Mutex<bool>, Condvar)>,
}
impl SinkWriter for BlockingWriter {
    fn write_all(&mut self, _bytes: &[u8]) -> io::Result<()> {
        self.entered.wait();
        let (lock, wake) = &*self.release;
        let mut released = lock.lock().unwrap();
        while !*released {
            released = wake.wait(released).unwrap();
        }
        Err(io::Error::other("blocked sink failed"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct BlockingFiles {
    writer: Mutex<Option<Box<dyn SinkWriter>>>,
}
impl FileOps for BlockingFiles {
    fn open(&self, _path: &Path) -> io::Result<Box<dyn SinkWriter>> {
        self.writer
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| io::Error::other("no writer"))
    }
    fn metadata(&self, _path: &Path) -> io::Result<FileMetadata> {
        Ok(regular(0))
    }
    fn exists(&self, _path: &Path) -> io::Result<bool> {
        Ok(false)
    }
    fn remove(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }
    fn rename(&self, _from: &Path, _to: &Path) -> io::Result<()> {
        Ok(())
    }
    fn read_dir(&self, _path: &Path) -> io::Result<Vec<PathBuf>> {
        Ok(Vec::new())
    }
}

#[test]
fn barrier_proves_one_sink_lock_does_not_block_another_sink() {
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let files_a = Arc::new(BlockingFiles {
        writer: Mutex::new(Some(Box::new(BlockingWriter {
            entered: entered.clone(),
            release: release.clone(),
        }))),
    });
    let script_b = Arc::new(Script::default());
    let common_clock = Arc::new(Clock::default());
    let emergency = Arc::new(Emergency::default());
    let sink_a = Arc::new(Mutex::new(FileSinkLifecycle::start(
        PathBuf::from("/logs/a.log"),
        100,
        1,
        0,
        files_a,
        common_clock.clone(),
        emergency.clone(),
    )));
    let sink_b = Arc::new(Mutex::new(fixture(
        script_b.clone(),
        common_clock,
        emergency,
        100,
        1,
        0,
    )));

    let a = {
        let sink = sink_a.clone();
        thread::spawn(move || sink.lock().unwrap().write_line("a"))
    };
    entered.wait();
    let b = {
        let sink = sink_b.clone();
        thread::spawn(move || sink.lock().unwrap().write_line("b"))
    };
    b.join().unwrap();
    assert!(
        String::from_utf8(script_b.output.lock().unwrap().clone())
            .unwrap()
            .contains('b'),
        "independent sink B must complete while A remains blocked"
    );
    let (lock, wake) = &*release;
    *lock.lock().unwrap() = true;
    wake.notify_one();
    a.join().unwrap();
}
