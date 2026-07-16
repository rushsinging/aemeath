use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

#[test]
fn tui_process_enters_and_restores_terminal_on_interrupt() {
    let Some(binary) = locate_aemeath_binary() else {
        eprintln!("SKIP: set AEMEATH_PTY_BIN or build target/debug/aemeath first");
        return;
    };
    let home = tempfile::tempdir().expect("isolated home");
    let agents_dir = home.path().join(".agents");
    std::fs::create_dir_all(&agents_dir).expect("create isolated agents dir");
    std::fs::write(agents_dir.join("aemeath.json"), r#"{"models":{"default":"local/test","providers":{"local":{"driver":"ollama","baseUrl":"http://127.0.0.1:11434","models":[{"id":"test","name":"PTY Test","contextWindow":4096,"maxTokens":256}]}}}}"#).expect("write isolated config");
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open pty");
    let mut command = CommandBuilder::new(binary);
    command.env("HOME", home.path());
    command.env("AEMEATH_AGENTS_DIR", &agents_dir);
    command.env("LLM_API_KEY", "pty-test-key");
    command.env("AEMEATH_VERSION", "pty-test");
    command.env("RUST_LOG", "off");
    let mut child = pair
        .slave
        .spawn_command(command)
        .expect("spawn aemeath in pty");
    drop(pair.slave);
    let output = Arc::new(Mutex::new(Vec::new()));
    let output_reader = Arc::clone(&output);
    let mut reader = pair.master.try_clone_reader().expect("clone pty reader");
    let reader_thread = std::thread::spawn(move || {
        let mut chunk = [0; 4096];
        while let Ok(size) = reader.read(&mut chunk) {
            if size == 0 {
                break;
            }
            output_reader
                .lock()
                .unwrap()
                .extend_from_slice(&chunk[..size]);
        }
    });
    let mut writer = pair.master.take_writer().expect("take pty writer");

    assert!(
        wait_for(&output, "\u{1b}[?1049h", Duration::from_secs(10)),
        "alternate screen was not entered: {:?}",
        text(&output)
    );
    writer.write_all(&[3]).expect("send Ctrl+C");
    writer.flush().expect("flush Ctrl+C");
    std::thread::sleep(Duration::from_millis(150));
    writer.write_all(&[3]).expect("send second Ctrl+C");
    writer.flush().expect("flush second Ctrl+C");
    drop(writer);
    assert!(
        wait_for(&output, "\u{1b}[?1049l", Duration::from_secs(10)),
        "alternate screen was not restored: {:?}",
        text(&output)
    );
    let status = child.wait().expect("wait aemeath");
    reader_thread.join().expect("join pty reader");
    assert!(status.success(), "aemeath exited with {status:?}");
    let output = text(&output);
    assert!(
        output.contains("\u{1b}[?25h"),
        "cursor was not restored: {output:?}"
    );
    assert!(
        !home.path().join(".aemeath").exists(),
        "legacy user directory was polluted"
    );
}

fn locate_aemeath_binary() -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("AEMEATH_PTY_BIN") {
        return Some(path.into());
    }
    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/aemeath");
    path.exists().then_some(path)
}
fn text(output: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8_lossy(&output.lock().unwrap()).into_owned()
}
fn wait_for(output: &Arc<Mutex<Vec<u8>>>, needle: &str, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if text(output).contains(needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}
