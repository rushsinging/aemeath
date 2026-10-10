//! L5（PTY + mock LLM）：Background Process 工具族 typed 显示与状态行
//! 第三行计数在真实终端的端到端验收。
//!
//! 链路：真实 aemeath 二进制 → mock LLM（SSE 脚本化响应）→ runtime
//! 事件链 → TUI 渲染 → PTY 真实终端屏幕文本断言。覆盖两条核心显示：
//! ① 超阈值 tool call 转后台 + Run 收口后（spinner 消失）status line
//! 第三行仍显示「N Backend Progress」；② BackgroundProcessList 工具
//! 卡片渲染 typed header「后台进程列表」与逐行摘要（非 JSON 原文）。
//!
//! 运行方式：`cargo test -p cli --test background_process_display_pty -- --ignored`
//!（需先 `cargo build -p cli --bin aemeath`；check-slow-test-matrix 已登记）。

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tui_test::{Operation, OperationResult, RunOptions, Session, Timeouts};

const PROCESS_TIMEOUT_MS: u64 = 30_000;

#[test]
#[ignore = "L5 slow test: run via scripts/check-slow-test-matrix.sh"]
fn background_process_tools_render_typed_display_and_status_line_count() {
    let mock_port = spawn_mock_llm();
    let binary = locate_aemeath_binary().expect(
        "PTY test requires a built binary; run `cargo build -p cli --bin aemeath` or set AEMEATH_PTY_BIN",
    );
    let home_path = std::env::temp_dir().join(format!("aemeath-l5-home-{}", std::process::id()));
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
        // 1s 阈值：ping 30s 稳定超阈值转后台。
        ("AEMEATH_TOOL_BACKGROUND_THRESHOLD_SECS", "1".to_string()),
    ] {
        args.push(format!("{name}={value}"));
    }
    args.push(binary.to_string_lossy().into_owned());

    let terminal = Session::new(format!("aemeath-bg-display-{}", std::process::id()));
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

    // ① 派发后台任务：mock 第 1 响应返回 Bash tool call（ping 30s 超
    // 1s 阈值转后台），第 2 响应纯文本收口 Run——spinner 消失后计数
    // 第三行必须仍在。
    submit(&terminal, "派发一个后台任务");
    wait_screen_text(&terminal, "已转后台", Duration::from_secs(25));
    // 位置级断言（#1895）：计数独占 status bar 第三行——与 Ready、
    // context 均不同行。
    wait_screen_standalone_row(&terminal, "Backend Progress", Duration::from_secs(10));

    // ② List typed 渲染：mock 第 3 响应返回 BackgroundProcessList tool
    // call，第 4 响应纯文本收口。断言 typed header 与解析摘要行。
    submit(&terminal, "列出后台进程");
    wait_screen_text(&terminal, "Backend Processes", Duration::from_secs(25));
    wait_screen_text(&terminal, "ping -c 30", Duration::from_secs(10));

    // 退出（双击 Ctrl+C），恢复终端。
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
    terminal.close();
}

fn submit(terminal: &Session, data: &str) {
    // Write 模拟逐字输入进聚焦的聊天输入框，Enter 提交（Submit 表单
    // 语义面向向导控件，聊天输入框实测不触发发送）。
    // CR 会被 PTY termios（ICRNL）翻译成 LF，crossterm 解析为 Ctrl+J；
    // kitty CSI u 编码（crossterm 兼容解析）可靠表达 Enter。
    terminal
        .execute(Operation::Write {
            data: format!("{data}\u{1b}[13u"),
        })
        .expect("键入并回车");
}

/// 轮询直到 needle 独占一行（不含 Ready——计数第三行专属），且存在
/// 独立 Ready 行（证明 status bar 正常渲染、位置正确）。
fn wait_screen_standalone_row(terminal: &Session, needle: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    let mut last_screen = String::new();
    while Instant::now() < deadline {
        if let OperationResult::Text(text) = terminal
            .execute(Operation::Text { full: true })
            .expect("读屏")
        {
            let lines: Vec<&str> = text.lines().collect();
            let has_ready = lines.iter().any(|line| line.contains("Ready"));
            let standalone = lines
                .iter()
                .any(|line| line.contains(needle) && !line.contains("Ready"));
            if has_ready && standalone {
                return;
            }
            last_screen = text;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    panic!("等待 {needle:?} 独立行超时；当前屏幕：\n{last_screen}");
}

/// 轮询全屏文本直到包含 needle（超时 panic 时附当前屏幕快照辅助定位）。
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

/// mock LLM（openai chat.completions SSE）：按请求序返回脚本化响应。
///
/// 脚本：① Bash tool call（ping 30s）→ ② 文本「已转后台」→
/// ③ BackgroundProcessList tool call → ④ 文本「列表已展示」→ 后续兜底
/// 文本「end」。线程为守护语义，测试进程退出即终结。
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
                    Some(body) => {
                        let seq = counter.fetch_add(1, Ordering::SeqCst);
                        let response = scripted_sse_response(seq, &body);
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

/// 读取一个 HTTP 请求（头 + Content-Length body）；连接关闭返回 None。
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

fn scripted_sse_response(seq: usize, _body: &str) -> String {
    match seq {
        0 => sse_tool_call_response(
            "call_bg_1",
            "Bash",
            r#"{"command":"ping -c 30 127.0.0.1","goal":"后台显示验证"}"#,
        ),
        1 => sse_text_response("已转后台"),
        2 => sse_tool_call_response("call_list_1", "BackgroundProcessList", "{}"),
        3 => sse_text_response("列表已展示"),
        _ => sse_text_response("end"),
    }
}

/// openai chat.completions SSE：tool call（arguments 全量放首 chunk）。
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
