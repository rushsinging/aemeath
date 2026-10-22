//! L5（PTY + mock LLM）：`/think` 档位调节端到端验收（#2032）。
//!
//! 链路：真实 aemeath 二进制 → runtime 事件流（SetThinking →
//! SessionReasoningControl → ThinkingChanged）→ TUI 渲染 → PTY 真实终端
//! 屏幕文本断言。覆盖：
//! ① `/think` 无参数打开 7 档全集对话框（Select Reasoning Effort）；
//! ② 对话框选择 medium → 回执 `[thinking mode: medium]`；
//! ③ `/think max` 带参生效（回执含 requested 档位）；
//! ④ 选择后落盘 `session.metadata.reasoning_effort`；
//! ⑤ `--resume` 重启后档位随会话恢复可见（bootstrap 投影读 metadata）。
//!
//! 运行方式：`cargo test -p cli --test think_display_pty -- --ignored`
//!（需先 `cargo build -p cli --bin aemeath`；check-slow-test-matrix 已登记）。

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tui_test::{Operation, OperationResult, RunOptions, Session, Timeouts};

const PROCESS_TIMEOUT_MS: u64 = 30_000;

#[test]
#[ignore = "L5 slow test: run via scripts/check-slow-test-matrix.sh"]
fn think_command_adjusts_level_persists_and_restores() {
    let mock_port = spawn_mock_llm();
    let binary = locate_aemeath_binary().expect(
        "PTY test requires a built binary; run `cargo build -p cli --bin aemeath` or set AEMEATH_PTY_BIN",
    );
    let home_path =
        std::env::temp_dir().join(format!("aemeath-l5-think-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home_path);
    std::fs::create_dir_all(&home_path).expect("create fixed home");
    let agents_dir = home_path.join(".agents");
    std::fs::create_dir_all(&agents_dir).expect("create isolated agents dir");
    write_isolated_config(&agents_dir, mock_port);

    // ── 阶段 1：新会话——先对话触发初落盘，再对话框、选择、带参 ──
    // 已知边界：空会话（无任何 Run）尚未初落盘，idle 命令的 metadata 更新
    // 会 NotFound（/model 同源既有行为）；主流路径是先对话再调档。
    let terminal = launch_terminal(&binary, &home_path, &agents_dir, Vec::new());
    wait_screen_text(&terminal, "Ready", Duration::from_secs(15));

    submit(&terminal, "触发会话落盘的一句话");
    wait_screen_text(&terminal, "ok", Duration::from_secs(20));
    wait_for_session_manifest(&agents_dir, Duration::from_secs(15));

    // 斜杠输入弹补全菜单：第一次 Enter 选中菜单项，第二次 Enter 提交。
    submit(&terminal, "/think");
    std::thread::sleep(Duration::from_millis(300));
    press_enter(&terminal);
    wait_screen_text(
        &terminal,
        "Select Reasoning Effort",
        Duration::from_secs(10),
    );
    // 7 档全集按最多档位算：off 与 max 两端都必须可见。
    wait_screen_text(&terminal, "minimal", Duration::from_secs(5));
    wait_screen_text(&terminal, "xhigh", Duration::from_secs(5));

    // 默认选中 off，Down×3 移到 medium，Enter 确认（等价提交 /think medium）。
    for _ in 0..3 {
        terminal
            .execute(Operation::Write {
                data: "\u{1b}[B".to_string(),
            })
            .expect("对话框下移");
        std::thread::sleep(Duration::from_millis(150));
    }
    press_enter(&terminal);
    wait_screen_text(
        &terminal,
        "[thinking mode: medium]",
        Duration::from_secs(10),
    );

    // 带参直接生效：mock provider（openai driver）能力阶梯覆盖全档，
    // 回执为直效形态（无 clamp 标注）。
    submit(&terminal, "/think max");
    std::thread::sleep(Duration::from_millis(300));
    press_enter(&terminal);
    wait_screen_text(&terminal, "[thinking mode: max]", Duration::from_secs(10));

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

    // ── 落盘断言：session.metadata.reasoning_effort == "max" ──
    let mut persisted: Option<(String, String)> = None;
    for _ in 0..20 {
        persisted = latest_session_with_reasoning(&agents_dir);
        if persisted.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    let (session_id, session_body) =
        persisted.unwrap_or_else(|| panic!("session 落盘未包含档位字段：{}", agents_dir.display()));
    assert!(
        session_body.contains(r#""reasoning_effort":"max""#),
        "metadata 应落盘 reasoning_effort=max，实际：{}",
        &session_body[..session_body.len().min(600)]
    );

    // ── 阶段 2：--resume 重启——档位随会话恢复可见 ──
    let resume_args = vec!["--resume".to_string(), session_id];
    let resumed = launch_terminal(&binary, &home_path, &agents_dir, resume_args);
    // 模型 entry 未配 reasoning → binding 默认 Off；状态栏出现 max 只能来自
    // resume 会话 metadata 的投影（#2032 bootstrap 恢复）。
    wait_screen_text(&resumed, "max", Duration::from_secs(20));
    resumed
        .execute(Operation::Signal {
            name: "SIGINT".into(),
        })
        .expect("send first Ctrl+C");
    std::thread::sleep(Duration::from_millis(300));
    resumed
        .execute(Operation::Signal {
            name: "SIGINT".into(),
        })
        .expect("send second Ctrl+C");
    let _ = resumed.close();

    if std::env::var_os("AEMEATH_PTY_KEEP_HOME").is_none() {
        let _ = std::fs::remove_dir_all(&home_path);
    }
}

fn write_isolated_config(agents_dir: &Path, mock_port: u16) {
    std::fs::write(
        agents_dir.join("aemeath.json"),
        format!(
            r#"{{"models":{{"default":"mock/test","providers":{{"mock":{{"driver":"openai","baseUrl":"http://127.0.0.1:{mock_port}/v1","apiKey":"test","models":[{{"id":"test","name":"Mock","contextWindow":8192,"maxTokens":1024}}]}}}}}},"permissions":{{"mode":"allow_all"}}}}"#
        ),
    )
    .expect("write isolated config");
}

fn launch_terminal(
    binary: &Path,
    home: &Path,
    agents_dir: &Path,
    extra_args: Vec<String>,
) -> Session {
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
    ] {
        args.push(format!("{name}={value}"));
    }
    args.push(binary.to_string_lossy().into_owned());
    args.extend(extra_args);

    let terminal = Session::new(format!("aemeath-think-display-{}", std::process::id()));
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
    terminal
}

/// 等待任一 session 数据集 manifest 出现（首 Run 后初落盘）。
fn wait_for_session_manifest(agents_dir: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if latest_session_manifest(agents_dir).is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    panic!("等待 session 初落盘超时：{}", agents_dir.display());
}

/// 收集 agents 目录下所有 session 数据集：
/// `session/**/<id>.dataset/`（含项目目录段与顶层布局）。SessionMetadata
/// 位于数据集成员文件（如 metadata.json）而非 dataset 清单本身。
fn session_datasets(agents_dir: &Path) -> Vec<(std::time::SystemTime, String, String)> {
    let session_root = agents_dir.join("session");
    let mut datasets = Vec::new();
    let Ok(entries) = std::fs::read_dir(&session_root) else {
        return datasets;
    };
    for entry in entries.flatten() {
        collect_datasets_under(&entry.path(), &mut datasets);
    }
    datasets.sort_by_key(|(modified, _, _)| *modified);
    datasets
}

fn collect_datasets_under(
    dir: &std::path::Path,
    datasets: &mut Vec<(std::time::SystemTime, String, String)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name.ends_with(".dataset") && path.is_dir() {
            let dataset_id = name.trim_end_matches(".dataset").to_string();
            if dataset_id.is_empty() {
                continue;
            }
            let mut dataset_body = String::new();
            let mut modified: Option<std::time::SystemTime> = None;
            read_dataset_files(&path, &mut dataset_body, &mut modified);
            if let Some(modified) = modified {
                datasets.push((modified, dataset_id, dataset_body));
            }
        } else if path.is_dir() {
            collect_datasets_under(&path, datasets);
        }
    }
}

/// 递归读 dataset 下的小文本成员（json/清单），聚合内容与最新修改时间。
fn read_dataset_files(
    dir: &std::path::Path,
    body: &mut String,
    modified: &mut Option<std::time::SystemTime>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            read_dataset_files(&path, body, modified);
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.len() > 4 * 1024 * 1024 {
            continue;
        }
        if let Ok(file_modified) = meta.modified() {
            *modified = Some(match modified {
                Some(current) if *current >= file_modified => *current,
                _ => file_modified,
            });
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            body.push_str(&content);
        }
    }
}

fn latest_session_manifest(agents_dir: &Path) -> Option<(String, std::time::SystemTime, String)> {
    session_datasets(agents_dir)
        .pop()
        .map(|(modified, session_id, body)| (session_id, modified, body))
}

/// 找到带 reasoning_effort 的最新 session 数据集，返回 (session_id, 聚合内容)。
fn latest_session_with_reasoning(agents_dir: &Path) -> Option<(String, String)> {
    session_datasets(agents_dir)
        .into_iter()
        .rev()
        .find(|(_, _, body)| body.contains("reasoning_effort"))
        .map(|(_, session_id, body)| (session_id, body))
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

/// /think 是纯 runtime 命令，正常路径不触发 LLM；mock 兜底返回最简文本。
fn spawn_mock_llm() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock llm");
    let port = listener.local_addr().expect("local addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            std::thread::spawn(move || loop {
                if read_http_request(&mut stream).is_none() {
                    return;
                }
                let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n\
                            data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
                            data: [DONE]\n\n";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                if stream.write_all(response.as_bytes()).is_err() {
                    return;
                }
            });
        }
    });
    port
}

fn read_http_request(stream: &mut std::net::TcpStream) -> Option<()> {
    let mut buffer = [0u8; 8192];
    let mut raw = Vec::new();
    loop {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        raw.extend_from_slice(&buffer[..read]);
        if let Some(pos) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&raw[..pos]).to_string();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.trim()
                        .eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            let mut body = raw[pos + 4..].to_vec();
            while body.len() < content_length {
                let read = stream.read(&mut buffer).ok()?;
                if read == 0 {
                    return None;
                }
                body.extend_from_slice(&buffer[..read]);
            }
            return Some(());
        }
        if raw.len() > 64 * 1024 {
            return None;
        }
    }
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
