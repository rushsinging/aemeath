use super::*;
use crate::domain::{LoggingOutputMode, LoggingSettings, NativeStderrRouting};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use std::path::PathBuf;

const MARKER: &str = "aemeath-native-stderr-probe\n";

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
    let written = unsafe { libc::write(libc::STDERR_FILENO, MARKER.as_ptr().cast(), MARKER.len()) };
    assert_eq!(written, MARKER.len() as isize);
}
