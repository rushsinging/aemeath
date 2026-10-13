//! L5（PTY + mock LLM）：Memory 工具族 typed 显示端到端验收。
//!
//! 链路：真实 aemeath 二进制 → mock LLM（SSE 脚本化响应）→ runtime
//! 事件链 → TUI 渲染 → PTY 真实终端屏幕文本断言。覆盖：
//! ① MemoryAdd header（category/layer/content）与 action·id 结果行；
//! ② MemoryList typed 摘要行（非 JSON 原文）；
//! ③ MemorySearch typed 命中行（非 JSON 原文）。
//!
//! 运行方式：`cargo test -p cli --test memory_display_pty -- --ignored`
//!（需先 `cargo build -p cli --bin aemeath`；check-slow-test-matrix 已登记）。

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tui_test::{Operation, OperationResult, RunOptions, Session, Timeouts};

const PROCESS_TIMEOUT_MS: u64 = 30_000;
const DISTINCT_CONTENT: &str = "L5 memory typed display marker";

#[test]
#[ignore = "L5 slow test: run via scripts/check-slow-test-matrix.sh"]
fn memory_tools_render_typed_display() {
    let mock_port = spawn_mock_llm();
    let binary = locate_aemeath_binary().expect(
        "PTY test requires a built binary; run `cargo build -p cli --bin aemeath` or set AEMEATH_PTY_BIN",
    );
    let home_path =
        std::env::temp_dir().join(format!("aemeath-l5-memory-home-{}", std::process::id()));
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

    let terminal = Session::new(format!("aemeath-memory-display-{}", std::process::id()));
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

    submit(&terminal, "记住一条验证显示的记忆");
    wait_screen_text(&terminal, "Memory Add", Duration::from_secs(25));
    wait_screen_text(&terminal, "pitfall", Duration::from_secs(10));
    wait_screen_text(&terminal, DISTINCT_CONTENT, Duration::from_secs(10));
    wait_screen_text(&terminal, "added", Duration::from_secs(15));
    wait_screen_text(&terminal, "已写入记忆", Duration::from_secs(15));

    submit(&terminal, "列出记忆");
    wait_screen_text(&terminal, "Memory List", Duration::from_secs(25));
    wait_screen_text(&terminal, DISTINCT_CONTENT, Duration::from_secs(10));
    wait_screen_text(&terminal, "列表已展示", Duration::from_secs(15));
    assert_screen_lacks(&terminal, r#""action":"list""#);

    submit(&terminal, "搜索记忆");
    wait_screen_text(&terminal, "Memory Search", Duration::from_secs(25));
    wait_screen_text(&terminal, "active", Duration::from_secs(10));
    wait_screen_text(&terminal, DISTINCT_CONTENT, Duration::from_secs(10));
    wait_screen_text(&terminal, "搜索已完成", Duration::from_secs(15));
    assert_screen_lacks(&terminal, r#""action":"search""#);

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

fn assert_screen_lacks(terminal: &Session, forbidden: &str) {
    let OperationResult::Text(text) = terminal
        .execute(Operation::Text { full: true })
        .expect("读屏")
    else {
        panic!("读屏未返回 Text");
    };
    assert!(
        !text.contains(forbidden),
        "屏幕不应出现 JSON 原文 {forbidden:?}；当前屏幕：\n{text}"
    );
}

fn spawn_mock_llm() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock llm");
    let port = listener.local_addr().expect("local addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            std::thread::spawn(move || loop {
                match read_http_request(&mut stream) {
                    Some(body) => {
                        let response = scripted_sse_response(&body);
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
            return None;
        }
        raw.extend_from_slice(&buffer[..read]);
        if let Some(pos) = find_subsequence(&raw, b"\r\n\r\n") {
            header_end = pos;
            break;
        }
        if raw.len() > 64 * 1024 {
            return None;
        }
    }
    let headers = String::from_utf8_lossy(&raw[..header_end]).to_string();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    let mut body = raw[header_end + 4..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    Some(String::from_utf8_lossy(&body).to_string())
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn latest_user_prompt(body: &str) -> Option<&'static str> {
    ["记住一条验证显示的记忆", "列出记忆", "搜索记忆"]
        .into_iter()
        .filter_map(|prompt| body.rfind(prompt).map(|offset| (offset, prompt)))
        .max_by_key(|(offset, _)| *offset)
        .map(|(_, prompt)| prompt)
}

fn request_awaits_tool_followup(body: &str) -> bool {
    let last_role_pos = [
        "\"role\":\"tool\"",
        "\"role\": \"tool\"",
        "\"role\":\"user\"",
        "\"role\": \"user\"",
        "\"role\":\"assistant\"",
        "\"role\": \"assistant\"",
    ]
    .into_iter()
    .filter_map(|marker| body.rfind(marker).map(|pos| (pos, marker)))
    .max_by_key(|(pos, _)| *pos);
    matches!(
        last_role_pos.map(|(_, marker)| marker),
        Some("\"role\":\"tool\"") | Some("\"role\": \"tool\"")
    )
}

fn scripted_sse_response(body: &str) -> String {
    let prompt = latest_user_prompt(body);
    if request_awaits_tool_followup(body) {
        return match prompt {
            Some("记住一条验证显示的记忆") => sse_text_response("已写入记忆"),
            Some("列出记忆") => sse_text_response("列表已展示"),
            Some("搜索记忆") => sse_text_response("搜索已完成"),
            _ => sse_text_response("tool续写"),
        };
    }
    match prompt {
        Some("搜索记忆") => sse_tool_call_response(
            "call_mem_search_1",
            "MemorySearch",
            &format!(r#"{{"goal":"搜记忆显示","query":"{DISTINCT_CONTENT}","limit":10}}"#),
        ),
        Some("列出记忆") => {
            sse_tool_call_response("call_mem_list_1", "MemoryList", r#"{"goal":"列记忆显示"}"#)
        }
        Some("记住一条验证显示的记忆") => sse_tool_call_response(
            "call_mem_add_1",
            "MemoryAdd",
            &format!(
                r#"{{"goal":"记记忆显示","content":"{DISTINCT_CONTENT}","layer":"project","category":"pitfall"}}"#
            ),
        ),
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

/// openai chat.completions SSE：纯文本响应。

fn sse_text_response(text: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n\
data: {{\"choices\":[{{\"delta\":{{\"role\":\"assistant\"}}}}]}}\n\n\
data: {{\"choices\":[{{\"delta\":{{\"content\":\"{text}\"}}}}]}}\n\n\
data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\n\
data: [DONE]\n\n"
    )
}

fn locate_aemeath_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("AEMEATH_PTY_BIN") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    // cargo 为集成测试自动构建 bin 并注入路径，天然跟随 worktree 的
    // target-dir 重定向；AEMEATH_PTY_BIN 仅作显式覆盖入口（与 pty_smoke
    // 同构）。
    PathBuf::from(env!("CARGO_BIN_EXE_aemeath"))
        .is_file()
        .then(|| PathBuf::from(env!("CARGO_BIN_EXE_aemeath")))
}
