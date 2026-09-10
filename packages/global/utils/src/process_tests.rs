use std::process::{Command, Stdio};

use super::{configure_std_noninteractive, configure_tokio_noninteractive};

#[cfg(unix)]
#[test]
fn std_child_starts_new_session_without_controlling_terminal() {
    let mut command = session_probe_command();
    configure_std_noninteractive(&mut command).expect("configure std command");

    let output = command.output().expect("run session probe");

    assert!(output.status.success(), "probe failed: {output:?}");
    assert_session_probe(&String::from_utf8_lossy(&output.stdout));
}

#[cfg(unix)]
#[tokio::test]
async fn tokio_child_starts_new_session_without_controlling_terminal() {
    let mut command = tokio::process::Command::new("sh");
    command
        .arg("-c")
        .arg(session_probe_script())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_tokio_noninteractive(&mut command).expect("configure tokio command");

    let output = command.output().await.expect("run async session probe");

    assert!(output.status.success(), "probe failed: {output:?}");
    assert_session_probe(&String::from_utf8_lossy(&output.stdout));
}

#[cfg(unix)]
fn session_probe_command() -> Command {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(session_probe_script())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[cfg(unix)]
fn session_probe_script() -> &'static str {
    "exec python3 - <<'PY'\nimport os\ntry:\n    tty = open('/dev/tty', 'rb')\nexcept OSError:\n    tty_open = 0\nelse:\n    tty.close()\n    tty_open = 1\nprint(f'{os.getpid()} {os.getpgid(0)} {os.getsid(0)} {tty_open}')\nPY"
}

#[cfg(unix)]
fn assert_session_probe(output: &str) {
    let fields: Vec<&str> = output.split_whitespace().collect();
    assert_eq!(
        fields.len(),
        4,
        "unexpected session probe output: {output:?}"
    );
    assert_eq!(fields[0], fields[1], "child must be process-group leader");
    assert_eq!(fields[0], fields[2], "child must be session leader");
    assert_eq!(fields[3], "0", "child must not have a controlling terminal");
}
