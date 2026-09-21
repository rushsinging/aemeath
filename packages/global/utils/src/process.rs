use std::io;
use std::process::Command;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

/// 将同步命令配置为严格非交互 child。
///
/// Unix child 会在 exec 前创建独立 session，因而不继承父控制终端。
#[cfg(unix)]
pub fn configure_std_noninteractive(command: &mut Command) -> io::Result<()> {
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(())
}

/// 非 Unix 平台尚无经过验证的控制终端隔离实现。
#[cfg(not(unix))]
pub fn configure_std_noninteractive(_command: &mut Command) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "当前平台不支持非交互外部进程的控制终端隔离",
    ))
}

/// 将 Tokio 命令委托给唯一的同步命令隔离配置。
pub fn configure_tokio_noninteractive(command: &mut tokio::process::Command) -> io::Result<()> {
    configure_std_noninteractive(command.as_std_mut())
}
