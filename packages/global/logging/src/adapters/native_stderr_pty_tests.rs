use super::*;
use crate::domain::{LoggingOutputMode, LoggingSettings, NativeStderrRouting};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use std::path::PathBuf;

const MARKER: &str = "aemeath-native-stderr-probe\n";
const ROUTED_MARKER: &str = "aemeath-native-stderr-routed\n";
const STDOUT_MARKER: &str = "aemeath-terminal-stdout-probe\n";

#[test]
fn shared_pty_routes_native_stderr_to_file() {
    if std::env::var_os("AEMEATH_NATIVE_STDERR_PROBE_CHILD").is_some() {
        run_probe_child();
        return;
    }

    let temp = tempfile::tempdir().expect("temp logs");
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open pty");
    let mut command = CommandBuilder::new(std::env::current_exe().expect("test executable"));
    command.args([
        "--exact",
        "adapters::native_stderr::pty_tests::shared_pty_routes_native_stderr_to_file",
        "--nocapture",
    ]);
    command.env("AEMEATH_NATIVE_STDERR_PROBE_CHILD", "1");
    command.env("AEMEATH_NATIVE_STDERR_LOGS_DIR", temp.path());
    let mut child = pair.slave.spawn_command(command).expect("spawn child");
    drop(pair.slave);
    child.wait().expect("wait child");

    let mut output = String::new();
    pair.master
        .try_clone_reader()
        .expect("pty reader")
        .read_to_string(&mut output)
        .expect("read pty");
    assert!(!output.contains(MARKER), "PTY leaked marker: {output:?}");
    let native = std::fs::read_to_string(temp.path().join("native-stderr.log"))
        .expect("read native stderr log");
    assert!(
        native.contains(MARKER),
        "native log missing marker: {native:?}"
    );
}

fn run_probe_child() {
    let logs_dir =
        PathBuf::from(std::env::var_os("AEMEATH_NATIVE_STDERR_LOGS_DIR").expect("probe logs dir"));
    let settings = LoggingSettings::new(
        "off".to_string(),
        LoggingOutputMode::File,
        NativeStderrRouting::AppendToFile,
        logs_dir,
        1024,
        0,
        0,
    );
    route_native_stderr(&settings).expect("route native stderr");
    // SAFETY: MARKER points to valid bytes for the duration of the write and FD 2
    // remains process-owned after routing.
    let written = unsafe { libc::write(STDERR_FD, MARKER.as_ptr().cast(), MARKER.len()) };
    assert_eq!(written, MARKER.len() as isize);
}

#[test]
fn shared_pty_restores_native_stderr_for_fatal_error() {
    if std::env::var_os("AEMEATH_NATIVE_STDERR_RESTORE_PROBE_CHILD").is_some() {
        run_restore_probe_child();
        return;
    }

    let temp = tempfile::tempdir().expect("temp logs");
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open pty");
    let mut command = CommandBuilder::new(std::env::current_exe().expect("test executable"));
    command.args([
        "--exact",
        "adapters::native_stderr::pty_tests::shared_pty_restores_native_stderr_for_fatal_error",
        "--nocapture",
    ]);
    command.env("AEMEATH_NATIVE_STDERR_RESTORE_PROBE_CHILD", "1");
    command.env("AEMEATH_NATIVE_STDERR_LOGS_DIR", temp.path());
    let mut child = pair.slave.spawn_command(command).expect("spawn child");
    // 必须与子进程并发读取：全部 slave 关闭后该 pty 的排队输出不再可读。
    let mut reader = pair.master.try_clone_reader().expect("pty reader");
    let reader_handle = std::thread::spawn(move || {
        let mut output = String::new();
        reader.read_to_string(&mut output).expect("read pty");
        output
    });
    drop(pair.slave);
    child.wait().expect("wait child");
    let output = reader_handle.join().expect("join pty reader");
    let native = std::fs::read_to_string(temp.path().join("native-stderr.log")).unwrap_or_default();
    assert!(
        native.contains(ROUTED_MARKER),
        "路由中的写入必须进 native-stderr.log: pty={output:?} native={native:?}"
    );
    assert!(
        output.contains(STDOUT_MARKER.trim_end()),
        "pty 读取通路必须正常: pty={output:?} native={native:?}"
    );
    // pty 会把换行转成 CRLF，断言时只比对内容部分。
    assert!(
        output.contains(MARKER.trim_end()),
        "恢复后的致命错误必须出现在终端: pty={output:?} native={native:?}"
    );
    assert!(
        !native.contains(MARKER),
        "恢复后的写入不得继续进 native-stderr.log: {native:?}"
    );
}

fn run_restore_probe_child() {
    let logs_dir =
        PathBuf::from(std::env::var_os("AEMEATH_NATIVE_STDERR_LOGS_DIR").expect("probe logs dir"));
    let settings = LoggingSettings::new(
        "off".to_string(),
        LoggingOutputMode::File,
        NativeStderrRouting::AppendToFile,
        logs_dir,
        1024,
        0,
        0,
    );
    route_native_stderr(&settings).expect("route native stderr");
    // SAFETY: every marker points to valid bytes for its write; FD 2 first lands in
    // the routed log, then `restore_native_stderr` returns it to the pty.
    let routed = unsafe {
        libc::write(
            STDERR_FD,
            ROUTED_MARKER.as_ptr().cast(),
            ROUTED_MARKER.len(),
        )
    };
    assert_eq!(routed, ROUTED_MARKER.len() as isize);
    restore_native_stderr().expect("restore native stderr");
    let terminal = unsafe {
        libc::write(
            STDOUT_FD,
            STDOUT_MARKER.as_ptr().cast(),
            STDOUT_MARKER.len(),
        )
    };
    assert_eq!(terminal, STDOUT_MARKER.len() as isize);
    let restored = unsafe { libc::write(STDERR_FD, MARKER.as_ptr().cast(), MARKER.len()) };
    assert_eq!(restored, MARKER.len() as isize);
}
