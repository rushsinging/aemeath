//! L5（PTY + mock LLM）：TUI 表格按单元格选中与复制的端到端验收。
//!
//! 链路：真实 aemeath 二进制 → mock LLM（SSE 脚本化 markdown 表格）→ 真实渲染链
//! （document_renderer → apply_gutter）→ PTY 发送 SGR 鼠标序列 → 单元格选区 →
//! 系统剪贴板内容断言。
//!
//! 覆盖：①折行单元格点任一条物理行都取到完整单元格；②矩形跨列复制为 Tab 分隔；
//! ③Cmd/Alt/Ctrl+拖拽绕过吸附走字符级（结果含框线字符）。
//!
//! 运行方式：`cargo test -p cli --test table_cell_selection_pty -- --ignored`
//!（需先 `cargo build -p cli --bin aemeath`；check-slow-test-matrix 已登记）。

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tui_test::{Operation, OperationResult, RunOptions, Session, Timeouts};

const PROCESS_TIMEOUT_MS: u64 = 60_000;

/// 折行单元格：宽度受限时渲染成多条物理行。
const CELL_LONG: &str =
    "长文本单元格的换行与截断策略模块，处理单个单元格内容显著超过列宽时的降级展示逻辑与省略号提示";
const CELL_SECOND: &str =
    "输入为经过宽度测量后的单元格内容与最终分配的可用列宽数值，不依赖外部存储或网络";

fn table_text() -> String {
    // 同一消息里放第二个表格：两张表的数据行号都从 0 起因，选区分组必须能区分，
    // 否则选中第一张表会把第二张表同序号的行一起复制出去。
    format!(
        "| 模块名称与职责边界 | 主要输入与依赖来源 |\n| --- | --- |\n| TUI 表格渲染组件 | 上游为 runtime 推流 |\n| {CELL_LONG} | {CELL_SECOND} |\n\n| 第二表A | 第二表B |\n| --- | --- |\n| z1 | z2 |"
    )
}

#[test]
#[ignore = "L5 slow test: run via scripts/check-slow-test-matrix.sh"]
fn table_cell_drag_copies_selection() {
    let mock_port = spawn_mock_llm();
    let binary = locate_aemeath_binary().expect(
        "PTY test requires a built binary; run `cargo build -p cli --bin aemeath` or set AEMEATH_PTY_BIN",
    );
    let home_path =
        std::env::temp_dir().join(format!("aemeath-l5-table-cell-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home_path);
    std::fs::create_dir_all(&home_path).expect("create fixed home");
    let home = home_path.clone();
    let agents_dir = home.join(".agents");
    std::fs::create_dir_all(&agents_dir).expect("create isolated agents dir");
    std::fs::write(
        agents_dir.join("aemeath.json"),
        format!(
            r#"{{"models":{{"default":"mock/test","providers":{{"mock":{{"driver":"openai","baseUrl":"http://127.0.0.1:{mock_port}/v1","apiKey":"test","models":[{{"id":"test","name":"Mock","contextWindow":8192,"maxTokens":2048}}]}}}}}},"permissions":{{"mode":"allow_all"}}}}"#
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

    let terminal = Session::new(format!("aemeath-table-cell-{}", std::process::id()));
    terminal
        .run(RunOptions {
            backend: tui_test::Backend::default(),
            program: "/usr/bin/env".to_string(),
            args,
            profile: Default::default(),
            cols: 110,
            rows: 40,
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

    submit(&terminal, "给我一张表格");
    let screen = wait_screen_text(&terminal, "省略号提示", Duration::from_secs(40));
    assert!(
        screen.contains('│'),
        "表格未渲染为框线形态；当前屏幕：\n{screen}"
    );

    // ① 折行单元格：点最后一条物理行，应取到整格完整内容。
    let (long_row, long_col) = locate_cell(&screen, "省略号提示", 0);
    click(&terminal, long_row, long_col);
    assert_eq!(
        wait_clipboard(CELL_LONG, Duration::from_secs(20)),
        CELL_LONG,
        "折行单元格必须整格复制（含全部物理行），且不含框线/填充"
    );

    // ② 矩形跨列：同一物理行从第一列拖到第二列，Tab 分隔。
    let (_, second_col) = locate_cell(&screen, "列宽数值", 1);
    drag(&terminal, long_row, long_col, long_row, second_col);
    assert_eq!(
        wait_clipboard("长文本单元格", Duration::from_secs(20)),
        format!("{CELL_LONG}\t{CELL_SECOND}"),
        "矩形跨列复制应为 Tab 分隔且两格内容完整"
    );

    // ③ 修饰键拖拽绕过吸附：走字符级，结果含框线字符。
    drag_with_modifier(&terminal, long_row, long_col, long_row, second_col, 8);
    let char_level = wait_clipboard("│", Duration::from_secs(20));
    assert!(
        char_level.contains('│'),
        "Alt+拖拽应走字符级（含框线字符）；实际 {char_level:?}"
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
}

/// 屏幕列 → SGR 鼠标事件（1-based）。
fn press(col: usize, row: usize, modifier: u16) -> String {
    format!("\u{1b}[<{};{};{}M", modifier, col + 1, row + 1)
}

fn drag_event(col: usize, row: usize, modifier: u16) -> String {
    format!("\u{1b}[<{};{};{}M", 32 + modifier, col + 1, row + 1)
}

fn release(col: usize, row: usize, modifier: u16) -> String {
    format!("\u{1b}[<{};{};{}m", modifier, col + 1, row + 1)
}

fn write_mouse(terminal: &Session, data: String) {
    terminal
        .execute(Operation::Write { data })
        .expect("发送鼠标事件");
}

/// 单击某个单元格（按下 + 松开），命中后复制单格。
fn click(terminal: &Session, row: usize, col: usize) {
    write_mouse(terminal, press(col, row, 0));
    std::thread::sleep(Duration::from_millis(150));
    write_mouse(terminal, release(col, row, 0));
}

fn drag(terminal: &Session, row0: usize, col0: usize, row1: usize, col1: usize) {
    drag_with_modifier(terminal, row0, col0, row1, col1, 0);
}

fn drag_with_modifier(
    terminal: &Session,
    row0: usize,
    col0: usize,
    row1: usize,
    col1: usize,
    modifier: u16,
) {
    write_mouse(terminal, press(col0, row0, modifier));
    std::thread::sleep(Duration::from_millis(150));
    write_mouse(terminal, drag_event(col1, row1, modifier));
    std::thread::sleep(Duration::from_millis(150));
    write_mouse(terminal, release(col1, row1, modifier));
}

/// 在含 `needle` 的表格行中定位第 `cell` 列内容起点：(屏幕行, 显示列)，均为 0-based。
fn locate_cell(screen: &str, needle: &str, cell: usize) -> (usize, usize) {
    let (row, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains('│') && line.contains(needle))
        .unwrap_or_else(|| panic!("屏幕表格行中找不到 {needle:?}；当前屏幕：\n{screen}"));
    let sep_width = '│'.len_utf8();
    let start_byte = if cell == 0 {
        0
    } else {
        nth_separator_byte(line, cell - 1).expect("左侧分隔符缺失") + sep_width
    };
    let end_byte = nth_separator_byte(line, cell).unwrap_or(line.len());
    let segment = &line[start_byte..end_byte];
    let offset = segment
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(offset, _)| offset)
        .unwrap_or_else(|| panic!("第 {cell} 列内容为空：{line:?}"));
    (row, display_col(line, start_byte + offset))
}

/// 第 `n` 个 `│` 的字节偏移。
fn nth_separator_byte(line: &str, n: usize) -> Option<usize> {
    line.match_indices('│').nth(n).map(|(offset, _)| offset)
}

/// 字节偏移 → 终端显示列（ASCII 1 列，CJK/全角 2 列）。
fn display_col(line: &str, byte_offset: usize) -> usize {
    line[..byte_offset]
        .chars()
        .map(|c| if c.is_ascii() { 1 } else { 2 })
        .sum()
}

fn submit(terminal: &Session, data: &str) {
    terminal
        .execute(Operation::Write {
            data: format!("{data}\u{1b}[13u"),
        })
        .expect("键入并回车");
}

fn screen_text(terminal: &Session) -> String {
    match terminal.execute(Operation::Text { full: true }) {
        Ok(OperationResult::Text(text)) => text,
        _ => panic!("读屏未返回 Text"),
    }
}

fn wait_screen_text(terminal: &Session, needle: &str, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    let mut last = String::new();
    while Instant::now() < deadline {
        last = screen_text(terminal);
        if last.contains(needle) {
            return last;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    panic!("等待 {needle:?} 超时；当前屏幕：\n{last}");
}

/// 轮询系统剪贴板直到内容命中 `needle`（或超时返回最后一次内容）。
fn wait_clipboard(needle: &str, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    let mut last = String::new();
    while Instant::now() < deadline {
        last = clipboard();
        if last.contains(needle) {
            return last;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    last
}

#[cfg(target_os = "macos")]
fn clipboard() -> String {
    std::process::Command::new("pbpaste")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).to_string())
        .unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
fn clipboard() -> String {
    String::new()
}

fn spawn_mock_llm() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock llm");
    let port = listener.local_addr().expect("local addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            std::thread::spawn(move || loop {
                match read_http_request(&mut stream) {
                    Some(_body) => {
                        if stream.write_all(table_sse_response().as_bytes()).is_err() {
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

/// 纯文本 SSE 响应，正文为折行 markdown 表格（换行转义进 JSON 字符串）。
fn table_sse_response() -> String {
    let escaped = table_text().replace('\\', "\\\\").replace('\n', "\\n");
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
    PathBuf::from(env!("CARGO_BIN_EXE_aemeath"))
        .is_file()
        .then(|| PathBuf::from(env!("CARGO_BIN_EXE_aemeath")))
}
