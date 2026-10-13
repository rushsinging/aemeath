//! L5（PTY + mock LLM）：AskUser 等待期间已有 queued message 不得 ActiveStep 炸 Run。
//!
//! 复现 #1944：Bash 执行中先排队一条消息，随后模型调用 AskUserQuestion；
//! 修复后问卷可答完且屏幕不出现「已存在活动 Step」。
//!
//! 运行：`cargo test -p cli --test ask_user_queued_pty -- --ignored`
//!（需先 `cargo build -p cli --bin aemeath`；check-slow-test-matrix 已登记）。

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tui_test::{Operation, OperationResult, RunOptions, Session, Timeouts};

const PROCESS_TIMEOUT_MS: u64 = 45_000;
const ASK_PROMPT: &str = "L5 ask while queued?";
const QUEUED_MARKER: &str = "queued-during-ask-marker";
const FINAL_TEXT: &str = "ask-user-queued-ok";

#[test]
#[ignore = "L5 slow test: run via scripts/check-slow-test-matrix.sh"]
fn ask_user_with_queued_message_does_not_hit_active_step_error() {
    let mock_port = spawn_mock_llm();
    let binary = locate_aemeath_binary().expect(
        "PTY test requires a built binary; run `cargo build -p cli --bin aemeath` or set AEMEATH_PTY_BIN",
    );
    let home_path =
        std::env::temp_dir().join(format!("aemeath-l5-ask-queued-home-{}", std::process::id()));
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

    let terminal = Session::new(format!("aemeath-ask-user-queued-{}", std::process::id()));
    terminal
        .run(RunOptions {
            backend: tui_test::Backend::default(),
            program: "/usr/bin/env".to_string(),
            args,
            profile: Default::default(),
            cols: 100,
            rows: 34,
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
        .expect("启动 aemeath");

    submit(&terminal, "start ask-user queued scenario");
    wait_screen_text(&terminal, "Bash", Duration::from_secs(20));

    submit(&terminal, QUEUED_MARKER);
    wait_screen_text(&terminal, QUEUED_MARKER, Duration::from_secs(10));

    wait_screen_text(&terminal, ASK_PROMPT, Duration::from_secs(30));
    assert_screen_lacks(&terminal, "已存在活动 Step");
    assert_screen_lacks(&terminal, "ActiveStepAlreadyExists");

    press_enter(&terminal);
    std::thread::sleep(Duration::from_millis(200));
    press_enter(&terminal);

    wait_screen_text(&terminal, FINAL_TEXT, Duration::from_secs(30));
    assert_screen_lacks(&terminal, "已存在活动 Step");
    assert_screen_lacks(&terminal, "run state error");

    let _ = terminal.execute(Operation::Signal {
        name: "SIGINT".into(),
    });
    let _ = std::fs::remove_dir_all(&home_path);
}

fn submit(terminal: &Session, data: &str) {
    terminal
        .execute(Operation::Write {
            data: format!("{data}\u{1b}[13u"),
        })
        .expect("键入并回车");
}

fn press_enter(terminal: &Session) {
    terminal
        .execute(Operation::Write {
            data: "\u{1b}[13u".to_string(),
        })
        .expect("press enter");
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
    panic!("等待屏幕文本 {needle:?} 超时；当前屏幕：\n{last_screen}");
}

fn assert_screen_lacks(terminal: &Session, forbidden: &str) {
    if let OperationResult::Text(text) = terminal
        .execute(Operation::Text { full: true })
        .expect("读屏")
    {
        assert!(
            !text.contains(forbidden),
            "screen must not contain `{forbidden}`\nSCREEN:\n{text}"
        );
    } else {
        panic!("expected Text screen snapshot");
    }
}

fn spawn_mock_llm() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock llm");
    let port = listener.local_addr().expect("mock port").port();
    let request_counter = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&request_counter);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let counter = Arc::clone(&counter);
            std::thread::spawn(move || loop {
                match read_http_request(&mut stream) {
                    Some(_body) => {
                        let seq = counter.fetch_add(1, Ordering::SeqCst);
                        let response = scripted_sse_response(seq);
                        if stream.write_all(response.as_bytes()).is_err() {
                            return;
                        }
                    }
                    None => return,
                }
            });
        }
    });
    port
}

fn read_http_request(stream: &mut std::net::TcpStream) -> Option<String> {
    let mut buffer = [0u8; 8192];
    let mut raw = Vec::new();
    let header_end;
    loop {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return if raw.is_empty() {
                None
            } else {
                Some(String::from_utf8_lossy(&raw).into_owned())
            };
        }
        raw.extend_from_slice(&buffer[..read]);
        if let Some(pos) = find_subsequence(&raw, b"\r\n\r\n") {
            header_end = pos + 4;
            break;
        }
        if raw.len() > 1024 * 1024 {
            return None;
        }
    }
    let headers = String::from_utf8_lossy(&raw[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let lower = line.to_ascii_lowercase();
            lower
                .strip_prefix("content-length:")
                .map(|value| value.trim().parse::<usize>().unwrap_or(0))
        })
        .unwrap_or(0);
    while raw.len() < header_end + content_length {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&buffer[..read]);
    }
    Some(String::from_utf8_lossy(&raw).into_owned())
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn scripted_sse_response(seq: usize) -> String {
    match seq {
        0 => sse_tool_call_response(
            "call_bash_1",
            "Bash",
            r#"{"command":"sleep 2","goal":"queue window"}"#,
        ),
        1 => sse_tool_call_response(
            "call_ask_1",
            "AskUserQuestion",
            &format!(
                r#"{{"goal":"提问排队","question":"{ASK_PROMPT}","options":[{{"title":"yes","description":"approve"}},{{"title":"no","description":"decline"}}]}}"#
            ),
        ),
        2 => sse_text_response(FINAL_TEXT),
        _ => sse_text_response("end"),
    }
}

fn sse_tool_call_response(call_id: &str, tool_name: &str, arguments: &str) -> String {
    let escaped = arguments.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n\
data: {{\"choices\":[{{\"delta\":{{\"role\":\"assistant\"}}}}]}}\n\n\
data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"{call_id}\",\"type\":\"function\",\"function\":{{\"name\":\"{tool_name}\",\"arguments\":\"{escaped}\"}}}}]}}}}]}}\n\n\
data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"tool_calls\"}}]}}\n\n\
data: [DONE]\n\n"
    )
}

fn sse_text_response(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n\
data: {{\"choices\":[{{\"delta\":{{\"role\":\"assistant\"}}}}]}}\n\n\
data: {{\"choices\":[{{\"delta\":{{\"content\":\"{escaped}\"}}}}]}}\n\n\
data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\n\
data: [DONE]\n\n"
    )
}

fn locate_aemeath_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("AEMEATH_PTY_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    PathBuf::from(env!("CARGO_BIN_EXE_aemeath"))
        .is_file()
        .then(|| PathBuf::from(env!("CARGO_BIN_EXE_aemeath")))
}
