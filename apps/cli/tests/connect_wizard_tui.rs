//! Connect 向导 TUI 端到端测试（tui-test-rs 驱动真实二进制）。
//!
//! 预写有效 Provider 配置使主 TUI 正常启动，`aemeath connect` 经
//! startup_connect 在 TUI 内自动打开向导；`Submit` 自带回车、方向 /
//! 空格 / Tab 用 `Key`，探测端点不可路由快速失败，验证"测试后显示
//! 结果"与"保存完成"契约。进程通过 `/usr/bin/env -i` 以允许变量
//! 白名单启动，HOME / cwd / AEMEATH_AGENTS_DIR 均指向临时目录。

#![cfg(unix)]

use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use tui_test::{
    KeyAction, LocatorExpectOptions, Operation, RunOptions, Session, TextSelector, Timeouts,
    WhitespaceMode,
};

fn binary_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aemeath"))
}

/// RAII 清理临时目录。
struct TempDirGuard(PathBuf);
impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 目录名带随机后缀，避免并行测试共享同一 agents 目录串台。
fn unique_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "-{}-{}",
        count,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.subsec_nanos())
            .unwrap_or_default()
    )
}

/// `extra_providers`：追加进预置 providers 的 JSON 片段（如带慢端点的
/// Anthropic），用于确定性覆盖流程测试。返回 (清理守卫, HOME, agents 目录)。
fn isolated_env_with_extra_providers(extra_providers: &str) -> (TempDirGuard, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "aemeath-tui-connect-{}{}",
        std::process::id(),
        unique_suffix()
    ));
    let agents_dir = root.join("agents");
    let home = root.join("home");
    std::fs::create_dir_all(&agents_dir).expect("创建临时 agents dir");
    std::fs::create_dir_all(&home).expect("创建临时 home dir");
    // 有效配置（DeepSeek 含默认模型）保证主 TUI 启动；向导里选 Anthropic 新建。
    std::fs::write(
        agents_dir.join("aemeath.json"),
        format!(
            r#"{{"models":{{"default":"DeepSeek/deepseek-flash","providers":{{"DeepSeek":{{"driver":"deepseek","baseUrl":"https://api.deepseek.com","apiKey":"sk-preconfigured","models":[{{"id":"deepseek-flash","contextWindow":1000,"maxTokens":100}}]}}{extra_providers}}}}}}}"#
        ),
    )
    .expect("写入预置配置");
    (TempDirGuard(root), home, agents_dir)
}

fn isolated_env() -> (TempDirGuard, PathBuf, PathBuf) {
    isolated_env_with_extra_providers("")
}

fn wait_for_text(terminal: &Session, needle: &str, step: &str, timeout_ms: u64) {
    if needle.is_ascii() {
        let mut selector = TextSelector::new(needle);
        selector.whitespace = WhitespaceMode::Normalize;
        if let Err(error) = terminal
            .get_by_text(selector)
            .any()
            .expect_with(LocatorExpectOptions {
                timeout_ms: Some(timeout_ms),
                ..Default::default()
            })
        {
            panic!(
                "{step}（等待 {needle}）失败：{error}；当前屏幕：\n{}",
                diagnostic_text(terminal)
            );
        }
        return;
    }

    // beta.5 的 Alacritty locator 对 CJK wide-cell 文本暂时不可见；在该边界
    // 使用同一 Session 的屏幕读取接口做有上限的条件等待，避免退回固定 sleep。
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if screen_text(terminal).contains(needle) {
            return;
        }
        if Instant::now() >= deadline {
            panic!(
                "{step}（等待 {needle}）超时；当前屏幕：\n{}",
                diagnostic_text(terminal)
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 仅读取当前可视区域（不含滚出屏幕的 scrollback），保证断言语义是
/// “结果已显示在当前屏幕”。
fn screen_text(terminal: &Session) -> String {
    terminal
        .execute(Operation::Text { full: false })
        .ok()
        .and_then(|result| match result {
            tui_test::OperationResult::Text(text) => Some(text),
            _ => None,
        })
        .unwrap_or_else(|| "<screen unavailable>".to_string())
}

fn diagnostic_text(terminal: &Session) -> String {
    let screen = screen_text(terminal);
    let state = match terminal.execute(Operation::State) {
        Ok(tui_test::OperationResult::State(state)) => format!(
            "exited={:?} exit_signal={:?} ready={} last_exit={:?}",
            state.exited, state.exit_signal, state.ready, state.last_exit
        ),
        Ok(_) => "state unavailable".to_string(),
        Err(error) => format!("state error: {error}"),
    };
    format!("{state}; screen:\n{screen}")
}

fn submit(terminal: &Session, data: &str) {
    terminal
        .execute(Operation::Submit {
            data: Some(data.to_string()),
        })
        .expect("提交");
}

fn key(terminal: &Session, name: &str) {
    terminal
        .execute(Operation::Key {
            keys: vec![name.to_string()],
            action: KeyAction::Press,
        })
        .expect("按键");
}

fn start_command(terminal: &Session, home: &Path, agents_dir: &Path) {
    // env -i 建立白名单环境：不继承开发者真实 HOME、API key、代理或
    // 其他 AEMEATH_* 配置，保证测试 hermetic。
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
        ("AEMEATH_VERSION", "0.0.0-test".to_string()),
        ("RUST_LOG", "off".to_string()),
        ("AEMEATH_LOG_LEVEL", "off".to_string()),
    ] {
        args.push(format!("{name}={value}"));
    }
    args.push(binary_path().to_string_lossy().into_owned());
    args.push("connect".to_string());

    terminal
        .run(RunOptions {
            backend: tui_test::Backend::default(),
            program: "/usr/bin/env".to_string(),
            args,
            profile: Default::default(),
            cols: 120,
            rows: 40,
            cwd: Some(home.to_string_lossy().into_owned()),
            env: Vec::new(),
            wait_ready: Some(false),
            restart: true,
            timeouts: Timeouts {
                text: Some(30_000),
                idle: Some(5_000),
                command: Some(30_000),
                exit: Some(30_000),
                ready: Some(30_000),
            },
            recording: Default::default(),
        })
        .expect("启动 CLI");
}

/// 受控慢探测服务：进程内 `TcpListener` 接受一次探测请求后挂起，
/// 等测试确认 busy 状态可见、请求确实到达后再放行 HTTP 500——既不
/// 依赖外部解释器，也不用固定 sleep 换确定性。
struct SlowProbeServer {
    request_rx: mpsc::Receiver<()>,
    response_tx: Option<mpsc::Sender<()>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl SlowProbeServer {
    fn spawn() -> (Self, u16) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定慢探测端口");
        let port = listener.local_addr().expect("读取慢探测端口").port();
        listener.set_nonblocking(true).expect("设置非阻塞监听");

        let (request_tx, request_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();

        let worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut conn, _)) => {
                        if request_tx.send(()).is_err() {
                            return;
                        }
                        // 等待放行；通道关闭（测试结束/清理）则直接断开连接。
                        if response_rx.recv().is_err() {
                            return;
                        }
                        let _ = conn.set_nonblocking(false);
                        let _ = conn.set_write_timeout(Some(Duration::from_secs(5)));
                        let _ =
                            conn.write_all(b"HTTP/1.1 500 Slow Probe\r\nContent-Length: 0\r\n\r\n");
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => return,
                }
            }
        });
        (
            Self {
                request_rx,
                response_tx: Some(response_tx),
                stop,
                worker: Some(worker),
            },
            port,
        )
    }

    /// 等待探测请求到达（有上限，超时即测试失败）。
    fn wait_for_request(&self) {
        self.request_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("探测请求未在期限内到达");
    }

    /// 放行响应：向挂起的探测请求返回 HTTP 500。
    fn release_response(&self) {
        self.response_tx
            .as_ref()
            .expect("慢探测服务已清理")
            .send(())
            .expect("放行慢探测响应");
    }
}

impl Drop for SlowProbeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        drop(self.response_tx.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[test]
#[ignore = "L5 slow test: run via scripts/check-slow-test-matrix.sh"]
fn connect_wizard_probe_shows_busy_then_result_on_long_probe() {
    let (server, port) = SlowProbeServer::spawn();
    // 预置带慢端点的 Anthropic：覆盖流程全程预填（endpoint/key/模型），
    // 消除输入法不确定性，探测确定性命中慢端点。
    let anthropic = format!(
        r#","Anthropic":{{"driver":"anthropic","baseUrl":"http://127.0.0.1:{port}","apiKey":"sk-preconfigured-anthropic","models":[{{"id":"claude-fable-5-1","contextWindow":1000000,"maxTokens":65536}}]}}"#
    );
    let (_guard, home, agents_dir) = isolated_env_with_extra_providers(&anthropic);
    let terminal = Session::new(format!("aemeath-slow-probe-{}", std::process::id()));
    start_command(&terminal, &home, &agents_dir);

    wait_for_text(&terminal, "● Anthropic", "向导首页", 30_000);
    submit(&terminal, "");
    wait_for_text(&terminal, "覆盖", "覆盖确认页", 10_000);
    submit(&terminal, "");
    // 覆盖后各页预填，一路回车：endpoint → key（掩码保留）→ UA → 模型（预选）
    submit(&terminal, "");
    submit(&terminal, "");
    submit(&terminal, "");
    submit(&terminal, "");
    wait_for_text(&terminal, "跳过测试", "测试页", 10_000);
    submit(&terminal, "");
    // 长探测：busy 状态必须先显示、请求确实挂起未返回，再放行 500；
    // 随后“失败”结果出现。
    wait_for_text(&terminal, "正在测试连接", "测试中状态", 5_000);
    server.wait_for_request();
    server.release_response();
    wait_for_text(&terminal, "失败", "探测结果", 30_000);
    terminal.close().expect("关闭会话");
    drop(server);
}

#[test]
#[ignore = "L5 slow test: run via scripts/check-slow-test-matrix.sh"]
fn connect_wizard_full_flow_shows_probe_result() {
    let (_guard, home, agents_dir) = isolated_env();
    let terminal = Session::new(format!("aemeath-connect-wizard-{}", std::process::id()));
    start_command(&terminal, &home, &agents_dir);

    // 0. 主 TUI 启动并自动打开向导
    wait_for_text(&terminal, "● Anthropic", "向导首页", 30_000);
    // 1. 回车选中首项 Anthropic（新建流程）
    submit(&terminal, "");
    wait_for_text(&terminal, "api.anthropic.com", "endpoint 页", 10_000);
    // 2. Ctrl+U 清空预填，输入不可路由端点（Submit 自带回车）
    key(&terminal, "ctrl+u");
    submit(&terminal, "http://127.0.0.1:1");
    // 3. API Key
    submit(&terminal, "sk-tui-test");
    // 4. UA：Anthropic 有 catalog 官方 UA 预填，直接回车
    wait_for_text(&terminal, "claude-cli", "UA 页", 10_000);
    submit(&terminal, "");
    // 5. 模型页：→ 进入 action 区（添加/编辑模型可达），先验证编辑页入口
    wait_for_text(&terminal, "claude-fable", "模型页", 10_000);
    key(&terminal, "right");
    wait_for_text(&terminal, "添加模型", "action 区高亮", 5_000);
    submit(&terminal, "");
    wait_for_text(&terminal, "Model ID", "进入模型编辑页", 10_000);
    // Esc 返回模型页，勾选首项提交
    key(&terminal, "escape");
    wait_for_text(&terminal, "claude-fable", "返回模型页", 10_000);
    key(&terminal, "space");
    submit(&terminal, "");
    // 6. 测试页：回车触发 primary（测试连接）
    wait_for_text(&terminal, "跳过测试", "测试页", 10_000);
    submit(&terminal, "");
    // 8. 断言探测结果：不可路由端点必须显示失败详情
    wait_for_text(&terminal, "失败", "探测结果", 30_000);
    // 9. 继续 → Review → 保存
    submit(&terminal, "");
    key(&terminal, "tab");
    submit(&terminal, "");
    wait_for_text(&terminal, "配置已保存", "保存完成", 10_000);

    terminal.close().expect("关闭会话");
}
