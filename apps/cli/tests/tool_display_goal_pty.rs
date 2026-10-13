//! L5（PTY + mock LLM）：内置工具 Header 显示 `goal` 端到端矩阵。
//!
//! 覆盖清单（与 `sdk::tool_display_name` 对齐）：
//! Bash/Glob/Grep/Read/ToolSearch/WebSearch/MemoryList/TaskLists。
//! Background* → `background_process_display_pty`；MemoryAdd/Search →
//! `memory_display_pty`（已补 goal）；AskUser/Agent/Worktree 因交互/切目录
//! 不进本矩阵（仍有 L2/L4 与 schema 契约）。
//!
//! 运行：`cargo test -p cli --test tool_display_goal_pty -- --ignored`

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tui_test::{Operation, OperationResult, RunOptions, Session, Timeouts};

const PROCESS_TIMEOUT_MS: u64 = 60_000;

const MATRIX: &[(&str, &str, &str)] = &[
    (
        "Bash",
        r#"{"goal":"GOAL_Bash","command":"echo goal-bash"}"#,
        "GOAL_Bash",
    ),
    (
        "Glob",
        r#"{"goal":"GOAL_Glob","pattern":"*.rs"}"#,
        "GOAL_Glob",
    ),
    (
        "Grep",
        r#"{"goal":"GOAL_Grep","pattern":"fn main"}"#,
        "GOAL_Grep",
    ),
    (
        "Read",
        r#"{"goal":"GOAL_Read","file_path":"Cargo.toml"}"#,
        "GOAL_Read",
    ),
    (
        "ToolSearch",
        r#"{"goal":"GOAL_ToolSearch","query":"memory"}"#,
        "GOAL_ToolSearch",
    ),
    (
        "WebSearch",
        r#"{"goal":"GOAL_WebSearch","query":"rust serde"}"#,
        "GOAL_WebSearch",
    ),
    (
        "MemoryList",
        r#"{"goal":"GOAL_MemoryList"}"#,
        "GOAL_MemoryList",
    ),
    (
        "TaskLists",
        r#"{"goal":"GOAL_TaskLists"}"#,
        "GOAL_TaskLists",
    ),
];

#[test]
#[ignore = "L5 slow test: run via scripts/check-slow-test-matrix.sh"]
fn builtin_tool_headers_render_goal_markers() {
    let mock_port = spawn_mock_llm();
    let binary = locate_aemeath_binary().expect(
        "PTY test requires a built binary; run `cargo build -p cli --bin aemeath` or set AEMEATH_PTY_BIN",
    );
    let home_path =
        std::env::temp_dir().join(format!("aemeath-l5-goal-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home_path);
    std::fs::create_dir_all(&home_path).expect("create fixed home");
    let home = home_path.clone();
    let agents_dir = home.join(".agents");
    std::fs::create_dir_all(&agents_dir).expect("create isolated agents dir");
    std::fs::write(
        agents_dir.join("aemeath.json"),
        format!(
            r#"{{"models":{{"default":"mock/test","providers":{{"mock":{{"driver":"openai","baseUrl":"http://127.0.0.1:{mock_port}/v1","apiKey":"test","models":[{{"id":"test","name":"Mock","contextWindow":8192,"maxTokens":1024}}]}}}}}},"permissions":{{"mode":"allow_all"}}}}"#
        ),
    )
    .expect("write isolated config");

    let workspace = home.join("ws");
    std::fs::create_dir_all(&workspace).expect("workspace");
    std::fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.0.0\"\nedition=\"2021\"\n",
    )
    .expect("cargo toml");

    let mut args = vec!["-i".to_string()];
    for (name, value) in [
        (
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string()),
        ),
        ("TERM", "xterm-256color".to_string()),
        ("HOME", home.to_string_lossy().into_owned()),
        (
            "AEMEATH_AGENTS_DIR",
            agents_dir.to_string_lossy().into_owned(),
        ),
        ("LLM_API_KEY", "pty-test-key".to_string()),
        ("AEMEATH_VERSION", "0.0.0-test".to_string()),
        ("AEMEATH_LOG_LEVEL", "debug".to_string()),
    ] {
        args.push(format!("{name}={value}"));
    }
    args.push(binary.to_string_lossy().into_owned());
    // cwd via env? tool_search uses cwd None; pass as last before binary?
    // Use `env -C` not portable. Instead chdir by writing and using program wrapper.

    let terminal = Session::new(format!("aemeath-goal-display-{}", std::process::id()));
    terminal
        .run(RunOptions {
            backend: tui_test::Backend::default(),
            program: "/usr/bin/env".to_string(),
            args,
            profile: Default::default(),
            cols: 100,
            rows: 34,
            cwd: Some(workspace.to_string_lossy().into_owned()),
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
        .expect("启动 aemeath");

    wait_screen_text(&terminal, "Ready", Duration::from_secs(25));

    for (index, (tool, _args, goal)) in MATRIX.iter().enumerate() {
        submit(&terminal, &format!("run tool {index} {tool}"));
        wait_screen_text(&terminal, goal, Duration::from_secs(45));
        wait_screen_text(&terminal, &format!("done-{index}"), Duration::from_secs(45));
    }

    terminal
        .execute(Operation::Signal {
            name: "SIGINT".into(),
        })
        .expect("send first Ctrl+C");
    std::thread::sleep(Duration::from_millis(300));
    terminal
        .execute(Operation::Signal {
            name: "SIGINT".into(),
        })
        .expect("send second Ctrl+C");
    let _ = terminal.close();
}

fn submit(terminal: &Session, data: &str) {
    terminal
        .execute(Operation::Write {
            data: format!("{data}\u{1b}[13u"),
        })
        .expect("键入并回车");
}

fn wait_screen_text(terminal: &Session, needle: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    let mut last_screen = String::new();
    while Instant::now() < deadline {
        if let OperationResult::Text(text) = terminal
            .execute(Operation::Text { full: true })
            .expect("读屏")
        {
            if text.contains(needle) {
                return;
            }
            last_screen = text;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    panic!("等待 {needle:?} 超时；当前屏幕：\n{last_screen}");
}

fn spawn_mock_llm() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
    let port = listener.local_addr().unwrap().port();
    let seq = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buf = [0u8; 65536];
            let _ = stream.read(&mut buf);
            let n = seq.fetch_add(1, Ordering::SeqCst);
            let body = if n % 2 == 0 {
                let index = n / 2;
                if index < MATRIX.len() {
                    let (tool, args, _) = MATRIX[index];
                    sse_tool_call_response(&format!("call_{index}"), tool, args)
                } else {
                    sse_text_response("extra")
                }
            } else {
                let index = n / 2;
                sse_text_response(&format!("done-{index}"))
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    port
}

fn sse_text_response(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "data: {{\"id\":\"chatcmpl\",\"object\":\"chat.completion.chunk\",\"choices\":[{{\"index\":0,\"delta\":{{\"role\":\"assistant\",\"content\":\"{escaped}\"}},\"finish_reason\":null}}]}}\n\ndata: {{\"id\":\"chatcmpl\",\"object\":\"chat.completion.chunk\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n"
    )
}

fn sse_tool_call_response(call_id: &str, tool_name: &str, arguments: &str) -> String {
    let escaped = arguments.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "data: {{\"id\":\"chatcmpl\",\"object\":\"chat.completion.chunk\",\"choices\":[{{\"index\":0,\"delta\":{{\"role\":\"assistant\",\"tool_calls\":[{{\"index\":0,\"id\":\"{call_id}\",\"type\":\"function\",\"function\":{{\"name\":\"{tool_name}\",\"arguments\":\"{escaped}\"}}}}]}},\"finish_reason\":null}}]}}\n\ndata: {{\"id\":\"chatcmpl\",\"object\":\"chat.completion.chunk\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n"
    )
}

fn locate_aemeath_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("AEMEATH_PTY_BIN") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_aemeath"))
        .is_file()
        .then(|| PathBuf::from(env!("CARGO_BIN_EXE_aemeath")))
}
