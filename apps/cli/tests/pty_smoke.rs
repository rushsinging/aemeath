//! PTY 终端生命周期 smoke 测试（tui-test-rs 驱动真实二进制）：验证
//! alternate screen 进入、双击 Ctrl+C 退出、退出码与终端状态恢复，
//! 以及隔离 HOME 下不产生 legacy 目录。依赖 PTY 与 Unix 信号语义。

#![cfg(unix)]

use std::path::PathBuf;

use tui_test::{Operation, RunOptions, Session, Timeouts};

const PROCESS_TIMEOUT_MS: u64 = 15_000;

#[test]
#[ignore = "L5 slow test: run via scripts/check-slow-test-matrix.sh"]
fn tui_process_enters_and_restores_terminal_on_interrupt() {
    let binary = locate_aemeath_binary().expect(
        "PTY smoke requires a built binary; run `cargo build -p cli --bin aemeath` or set AEMEATH_PTY_BIN",
    );
    let home = tempfile::tempdir().expect("isolated home");
    let agents_dir = home.path().join(".agents");
    std::fs::create_dir_all(&agents_dir).expect("create isolated agents dir");
    std::fs::write(
        agents_dir.join("aemeath.json"),
        r#"{"models":{"default":"local/test","providers":{"local":{"driver":"ollama","baseUrl":"http://127.0.0.1:11434","models":[{"id":"test","name":"PTY Test","contextWindow":4096,"maxTokens":256}]}}}}"#,
    )
    .expect("write isolated config");

    let mut args = vec!["-i".to_string()];
    for (name, value) in [
        (
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string()),
        ),
        ("TERM", "xterm-256color".to_string()),
        ("HOME", home.path().to_string_lossy().into_owned()),
        (
            "AEMEATH_AGENTS_DIR",
            agents_dir.to_string_lossy().into_owned(),
        ),
        ("LLM_API_KEY", "pty-test-key".to_string()),
        ("AEMEATH_VERSION", "0.0.0-test".to_string()),
        ("RUST_LOG", "off".to_string()),
        ("AEMEATH_LOG_LEVEL", "debug".to_string()),
    ] {
        args.push(format!("{name}={value}"));
    }
    args.push(binary.to_string_lossy().into_owned());

    let terminal = Session::new(format!("aemeath-pty-smoke-{}", std::process::id()));
    terminal
        .run(RunOptions {
            backend: tui_test::Backend::default(),
            program: "/usr/bin/env".to_string(),
            args,
            profile: Default::default(),
            cols: 80,
            rows: 24,
            cwd: None,
            env: Vec::new(),
            wait_ready: Some(false),
            restart: true,
            timeouts: Timeouts {
                text: Some(PROCESS_TIMEOUT_MS),
                idle: Some(PROCESS_TIMEOUT_MS),
                command: Some(PROCESS_TIMEOUT_MS),
                exit: Some(PROCESS_TIMEOUT_MS),
                ready: Some(PROCESS_TIMEOUT_MS),
            },
            recording: Default::default(),
        })
        .expect("spawn aemeath in tui-test PTY");

    terminal
        .execute(Operation::ExpectMode {
            mode: "alternate_screen".to_string(),
            enabled: true,
            timeout_ms: Some(PROCESS_TIMEOUT_MS),
        })
        .expect("alternate screen was not entered");

    terminal
        .execute(Operation::Signal {
            name: "INT".to_string(),
        })
        .expect("send first Ctrl+C");
    terminal
        .execute(Operation::Signal {
            name: "INT".to_string(),
        })
        .expect("send second Ctrl+C");
    terminal
        .execute(Operation::WaitExit {
            timeout_ms: Some(PROCESS_TIMEOUT_MS),
        })
        .expect("aemeath did not exit");
    let state = match terminal
        .execute(Operation::State)
        .expect("read final tui-test state")
    {
        tui_test::OperationResult::State(state) => state,
        _ => panic!("tui-test returned an unexpected final state result"),
    };
    assert_eq!(
        state.exited,
        Some(0),
        "aemeath exited unsuccessfully: {state:?}"
    );
    assert_eq!(
        state.exit_signal, None,
        "aemeath exited by signal: {state:?}"
    );
    terminal
        .execute(Operation::ExpectMode {
            mode: "alternate_screen".to_string(),
            enabled: false,
            timeout_ms: Some(PROCESS_TIMEOUT_MS),
        })
        .expect("alternate screen was not restored");
    terminal
        .execute(Operation::ExpectCursor {
            visible: Some(true),
            shape: None,
            x: None,
            y: None,
            timeout_ms: Some(PROCESS_TIMEOUT_MS),
        })
        .expect("cursor was not restored");

    assert!(
        !home.path().join(".aemeath").exists(),
        "legacy user directory was polluted"
    );
    terminal.close().expect("close tui-test session");
}

fn locate_aemeath_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("AEMEATH_PTY_BIN") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    // cargo 为集成测试自动构建 bin 并注入路径，天然跟随 worktree 的
    // target-dir 重定向；AEMEATH_PTY_BIN 仅作显式覆盖入口。
    PathBuf::from(env!("CARGO_BIN_EXE_aemeath"))
        .is_file()
        .then(|| PathBuf::from(env!("CARGO_BIN_EXE_aemeath")))
}
