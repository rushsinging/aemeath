use super::constants::{STDERR_FD, STDOUT_FD};
use super::state::SAVED_STDERR;
use crate::domain::{LoggingSettings, NativeStderrRouting};
#[cfg(unix)]
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

#[cfg(unix)]
use std::os::fd::AsRawFd;

/// 路由前保存的原生 stderr 描述符副本，供致命错误重新在终端可见。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SavedStderr {
    fd: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalIdentity {
    device: u64,
    inode: u64,
}

trait NativeStderrOps {
    type Target;

    fn is_terminal(&self, fd: i32) -> io::Result<bool>;
    fn terminal_identity(&self, fd: i32) -> io::Result<TerminalIdentity>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn open_append(&self, path: &Path) -> io::Result<Self::Target>;
    fn replace_stderr(&self, target: &Self::Target) -> io::Result<()>;
    /// 复制原生 stderr 描述符，供致命错误恢复终端可见性。
    fn save_stderr(&self) -> io::Result<SavedStderr>;
    /// 把保存的描述符重新指向进程 stderr。
    fn restore_stderr(&self, saved: SavedStderr) -> io::Result<()>;
}

pub(super) fn route_native_stderr(settings: &LoggingSettings) -> io::Result<()> {
    #[cfg(unix)]
    {
        route_native_stderr_with(settings, &UnixNativeStderrOps)
    }
    #[cfg(not(unix))]
    {
        let _ = settings;
        Ok(())
    }
}

fn route_native_stderr_with<Ops>(settings: &LoggingSettings, ops: &Ops) -> io::Result<()>
where
    Ops: NativeStderrOps,
{
    if settings.native_stderr_routing() == NativeStderrRouting::Preserve {
        return Ok(());
    }
    if !ops.is_terminal(STDERR_FD).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("inspect native stderr terminal: {error}"),
        )
    })? {
        return Ok(());
    }
    if !ops.is_terminal(STDOUT_FD).map_err(|error| {
        io::Error::new(error.kind(), format!("inspect stdout terminal: {error}"))
    })? {
        return Ok(());
    }
    let stdout_identity = ops.terminal_identity(STDOUT_FD).map_err(|error| {
        io::Error::new(error.kind(), format!("identify stdout terminal: {error}"))
    })?;
    let stderr_identity = ops.terminal_identity(STDERR_FD).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("identify native stderr terminal: {error}"),
        )
    })?;
    if stdout_identity != stderr_identity {
        return Ok(());
    }

    let path = settings.native_stderr_path();
    ops.create_dir_all(settings.logs_dir()).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "create native stderr directory '{}': {error}",
                settings.logs_dir().display()
            ),
        )
    })?;
    let target = ops.open_append(&path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("open native stderr file '{}': {error}", path.display()),
        )
    })?;
    let saved = ops.save_stderr().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("save native stderr before routing: {error}"),
        )
    })?;
    ops.replace_stderr(&target).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("replace native stderr with '{}': {error}", path.display()),
        )
    })?;
    let _ = SAVED_STDERR.set(saved);
    Ok(())
}

/// 把进程 stderr 恢复到路由前的原生终端：致命错误必须在终端可见，而 TUI 模式下
/// stderr 已被路由到 `native-stderr.log`。未发生路由时为空操作。
pub fn restore_native_stderr() -> io::Result<()> {
    #[cfg(unix)]
    {
        match SAVED_STDERR.get() {
            Some(saved) => UnixNativeStderrOps.restore_stderr(*saved),
            None => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        Ok(())
    }
}

#[cfg(test)]
fn restore_native_stderr_with<Ops>(ops: &Ops, saved: SavedStderr) -> io::Result<()>
where
    Ops: NativeStderrOps,
{
    ops.restore_stderr(saved)
        .map_err(|error| io::Error::new(error.kind(), format!("restore native stderr: {error}")))
}

#[cfg(unix)]
struct UnixNativeStderrOps;

#[cfg(unix)]
impl NativeStderrOps for UnixNativeStderrOps {
    type Target = File;

    fn is_terminal(&self, fd: i32) -> io::Result<bool> {
        // SAFETY: `isatty` only inspects the supplied process file descriptor.
        let result = unsafe { libc::isatty(fd) };
        if result == 1 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOTTY) {
            Ok(false)
        } else {
            Err(error)
        }
    }

    fn terminal_identity(&self, fd: i32) -> io::Result<TerminalIdentity> {
        // SAFETY: zero is a valid initial bit pattern for `libc::stat`; `fstat`
        // initializes it before any field is read when it returns success.
        let mut status: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `status` is valid writable storage and `fd` is inspected only.
        if unsafe { libc::fstat(fd, &mut status) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(TerminalIdentity {
            device: status.st_dev as u64,
            inode: status.st_ino as u64,
        })
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn open_append(&self, path: &Path) -> io::Result<Self::Target> {
        OpenOptions::new().create(true).append(true).open(path)
    }

    fn replace_stderr(&self, target: &Self::Target) -> io::Result<()> {
        // SAFETY: both descriptors are valid for this call; `dup2` atomically
        // replaces FD 2 while `target` remains alive through the call.
        if unsafe { libc::dup2(target.as_raw_fd(), STDERR_FD) } == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn save_stderr(&self) -> io::Result<SavedStderr> {
        // SAFETY: `dup` only duplicates FD 2 and leaves the original untouched.
        let fd = unsafe { libc::dup(STDERR_FD) };
        if fd == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(SavedStderr { fd })
        }
    }

    fn restore_stderr(&self, saved: SavedStderr) -> io::Result<()> {
        // SAFETY: both descriptors are valid for this call; `dup2` atomically
        // replaces FD 2 with the previously saved terminal descriptor.
        if unsafe { libc::dup2(saved.fd, STDERR_FD) } == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(all(test, unix))]
#[path = "native_stderr_pty_tests.rs"]
mod pty_tests;
#[cfg(all(test, unix))]
#[path = "native_stderr_tests.rs"]
mod tests;
