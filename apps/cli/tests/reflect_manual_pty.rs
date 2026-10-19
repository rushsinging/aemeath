//! L5（PTY + mock LLM）：`/reflect-now` 手动反思端到端验收。
//!
//! 链路：真实 aemeath 二进制 → mock LLM（SSE 脚本化反思响应）→ runtime
//! detached 反思链路 → TUI 渲染 → PTY 真实终端屏幕文本断言。覆盖：
//! ① 空会话（无新增对话）`/reflect-now` 仍派发存量反思，受理回执
//!    「Reflection 已在后台开始」；
//! ② 反思期间状态栏出现 `Reflecting`（ReflectionActiveChanged 驱动）；
//! ③ 两阶段（去噪 + 归纳）请求均发出（mock 计数）；
//! ④ 成功无变更终态回执「Reflection 已完成：没有记忆变更。」；
//! ⑤ NEVER 再出现「无新增对话内容…已跳过」空增量跳过文案。
//!
//! 运行方式：`cargo test -p cli --test reflect_manual_pty -- --ignored`
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
/// 两阶段反思各延迟一秒：既给状态栏 `Reflecting` 留出可断言窗口，也保证
/// 去噪与归纳是两次独立请求（计数断言的基础）。
const MOCK_REFLECT_DELAY_MS: u64 = 1_000;
const EMPTY_REFLECTION_JSON: &str =
    r#"{"deviations":[],"suggested_memories":[],"outdated_memories":[]}"#;

#[test]
#[ignore = "L5 slow test: run via scripts/check-slow-test-matrix.sh"]
fn reflect_now_reviews_existing_memory_without_new_dialog() {
    let reflection_requests = Arc::new(AtomicUsize::new(0));
    let mock_port = spawn_mock_llm(Arc::clone(&reflection_requests));
    let binary = locate_aemeath_binary().expect(
        "PTY test requires a built binary; run `cargo build -p cli --bin aemeath` or set AEMEATH_PTY_BIN",
    );
    let home_path =
        std::env::temp_dir().join(format!("aemeath-l5-reflect-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home_path);
    std::fs::create_dir_all(&home_path).expect("create fixed home");
    let home = home_path.clone();
    let agents_dir = home.join(".agents");
    std::fs::create_dir_all(&agents_dir).expect("create isolated agents dir");
    // memory / reflection 默认启用；空 corpus（无任何记忆条目）正是本用例
    // 要验收的「无新增对话且无存量」极端：反思仍必须走完两阶段。
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

    let terminal = Session::new(format!("aemeath-reflect-manual-{}", std::process::id()));
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

    // 等 TUI 完成初始化（状态栏 Ready）再键入：过早写入会被启动期吞掉。
    wait_screen_text(&terminal, "Ready", Duration::from_secs(15));

    // 空会话直接触发手动反思：无新增对话 MUST 仍派发（存量审查入口）。
    // 斜杠输入会弹命令补全菜单：第一次 Enter 选中菜单项，第二次 Enter 提交。
    submit(&terminal, "/reflect-now");
    std::thread::sleep(Duration::from_millis(300));
    press_enter(&terminal);
    wait_screen_text(
        &terminal,
        "Reflection 已在后台开始",
        Duration::from_secs(15),
    );
    wait_screen_text(&terminal, "Reflecting", Duration::from_secs(15));
    wait_screen_text(
        &terminal,
        "Reflection 已完成：没有记忆变更。",
        Duration::from_secs(25),
    );
    assert_screen_lacks(&terminal, "已跳过");
    assert_screen_lacks(&terminal, "无新增对话内容");

    // 两阶段（去噪 + 归纳）都必须真实发出请求，缺一即「静默跳过」回归。
    let observed = reflection_requests.load(Ordering::SeqCst);
    assert_eq!(
        observed, 2,
        "手动反思必须恰好发出去噪 + 归纳两次 LLM 请求，实际 {observed}"
    );

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
    // 调试期保留现场：断言通过后由下方清理；失败时 panic 已带屏幕。
    if std::env::var_os("AEMEATH_PTY_KEEP_HOME").is_none() {
        let _ = std::fs::remove_dir_all(&home_path);
    }
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
        .expect("补一次回车");
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
        "屏幕不应出现 {forbidden:?}；当前屏幕：\n{text}"
    );
}

fn spawn_mock_llm(reflection_requests: Arc<AtomicUsize>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock llm");
    let port = listener.local_addr().expect("local addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let counter = Arc::clone(&reflection_requests);
            std::thread::spawn(move || loop {
                match read_http_request(&mut stream) {
                    Some(body) => {
                        let response = scripted_sse_response(&body, &counter);
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

/// 反思请求识别：去噪与归纳 prompt 的 JSON schema 都包含
/// `outdated_memories` 字段说明（主对话 system prompt 与 Memory 工具 schema
/// 均不含该词），单特征即可稳定区分（去噪 prompt 不含 `deviations`）。
fn is_reflection_request(body: &str) -> bool {
    body.contains("outdated_memories")
}

fn scripted_sse_response(body: &str, reflection_requests: &AtomicUsize) -> String {
    if is_reflection_request(body) {
        reflection_requests.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(MOCK_REFLECT_DELAY_MS));
        return sse_text_response(EMPTY_REFLECTION_JSON);
    }
    sse_text_response("end")
}

// openai chat.completions SSE：纯文本响应。text 内的引号/反斜杠 MUST 转义，
// 否则 JSON 输出体的引号会破坏 data 帧本身（表现为 provider 侧空响应）。
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
