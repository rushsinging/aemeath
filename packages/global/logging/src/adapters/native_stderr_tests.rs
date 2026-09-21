use super::*;
use crate::domain::{LoggingOutputMode, LoggingSettings, NativeStderrRouting};
use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct FakeNativeStderrOps {
    tty: RefCell<Vec<(i32, bool)>>,
    identities: RefCell<Vec<(i32, TerminalIdentity)>>,
    created_dirs: RefCell<Vec<PathBuf>>,
    opened_paths: RefCell<Vec<PathBuf>>,
    replaced: RefCell<Vec<i32>>,
    fail_open: bool,
    fail_replace: bool,
}

impl NativeStderrOps for FakeNativeStderrOps {
    type Target = ();

    fn is_terminal(&self, fd: i32) -> io::Result<bool> {
        Ok(self
            .tty
            .borrow()
            .iter()
            .find_map(|(candidate, value)| (*candidate == fd).then_some(*value))
            .unwrap_or(false))
    }

    fn terminal_identity(&self, fd: i32) -> io::Result<TerminalIdentity> {
        self.identities
            .borrow()
            .iter()
            .find_map(|(candidate, value)| (*candidate == fd).then_some(*value))
            .ok_or_else(|| io::Error::other("missing identity"))
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.created_dirs.borrow_mut().push(path.to_path_buf());
        Ok(())
    }

    fn open_append(&self, path: &Path) -> io::Result<Self::Target> {
        self.opened_paths.borrow_mut().push(path.to_path_buf());
        if self.fail_open {
            Err(io::Error::other("open failed"))
        } else {
            Ok(())
        }
    }

    fn replace_stderr(&self, _target: &Self::Target) -> io::Result<()> {
        self.replaced.borrow_mut().push(STDERR_FD);
        if self.fail_replace {
            Err(io::Error::other("replace failed"))
        } else {
            Ok(())
        }
    }
}

fn settings(routing: NativeStderrRouting) -> LoggingSettings {
    LoggingSettings::new(
        "warn".to_string(),
        LoggingOutputMode::File,
        routing,
        PathBuf::from("/tmp/aemeath-logs"),
        1,
        0,
        0,
    )
}

fn same_terminal_ops() -> FakeNativeStderrOps {
    FakeNativeStderrOps {
        tty: RefCell::new(vec![(STDOUT_FD, true), (STDERR_FD, true)]),
        identities: RefCell::new(vec![
            (
                STDOUT_FD,
                TerminalIdentity {
                    device: 7,
                    inode: 11,
                },
            ),
            (
                STDERR_FD,
                TerminalIdentity {
                    device: 7,
                    inode: 11,
                },
            ),
        ]),
        ..FakeNativeStderrOps::default()
    }
}

#[test]
fn preserve_does_not_inspect_or_replace_stderr() {
    let ops = FakeNativeStderrOps::default();

    route_native_stderr_with(&settings(NativeStderrRouting::Preserve), &ops).unwrap();

    assert!(ops.opened_paths.borrow().is_empty());
    assert!(ops.replaced.borrow().is_empty());
}

#[test]
fn non_terminal_stderr_is_preserved() {
    let ops = same_terminal_ops();
    ops.tty.borrow_mut().retain(|(fd, _)| *fd != STDERR_FD);

    route_native_stderr_with(&settings(NativeStderrRouting::AppendToFile), &ops).unwrap();

    assert!(ops.replaced.borrow().is_empty());
}

#[test]
fn non_terminal_stdout_is_preserved() {
    let ops = same_terminal_ops();
    ops.tty.borrow_mut().retain(|(fd, _)| *fd != STDOUT_FD);

    route_native_stderr_with(&settings(NativeStderrRouting::AppendToFile), &ops).unwrap();

    assert!(ops.replaced.borrow().is_empty());
}

#[test]
fn different_terminals_are_preserved() {
    let ops = same_terminal_ops();
    ops.identities.borrow_mut()[1].1.inode = 12;

    route_native_stderr_with(&settings(NativeStderrRouting::AppendToFile), &ops).unwrap();

    assert!(ops.replaced.borrow().is_empty());
}

#[test]
fn shared_terminal_routes_stderr_to_append_file() {
    let ops = same_terminal_ops();

    route_native_stderr_with(&settings(NativeStderrRouting::AppendToFile), &ops).unwrap();

    assert_eq!(
        ops.created_dirs.borrow().as_slice(),
        [PathBuf::from("/tmp/aemeath-logs")]
    );
    assert_eq!(
        ops.opened_paths.borrow().as_slice(),
        [PathBuf::from("/tmp/aemeath-logs/native-stderr.log")]
    );
    assert_eq!(ops.replaced.borrow().as_slice(), [STDERR_FD]);
}

#[test]
fn open_failure_leaves_stderr_unchanged() {
    let ops = FakeNativeStderrOps {
        fail_open: true,
        ..same_terminal_ops()
    };

    let error =
        route_native_stderr_with(&settings(NativeStderrRouting::AppendToFile), &ops).unwrap_err();

    assert!(error.to_string().contains("open"));
    assert!(ops.replaced.borrow().is_empty());
}

#[test]
fn replace_failure_reports_stage_and_path() {
    let ops = FakeNativeStderrOps {
        fail_replace: true,
        ..same_terminal_ops()
    };

    let error =
        route_native_stderr_with(&settings(NativeStderrRouting::AppendToFile), &ops).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("replace"));
    assert!(message.contains("native-stderr.log"));
}
