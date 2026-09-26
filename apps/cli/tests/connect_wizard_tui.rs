//! Connect 向导 TUI 端到端测试（tui-test-rs 驱动真实二进制）。
//!
//! 预写有效 Provider 配置使主 TUI 正常启动，`aemeath connect` 经
//! startup_connect 在 TUI 内自动打开向导；`Submit` 自带回车、方向 /
//! 空格 / Tab 用 `Key`，探测端点不可路由快速失败，验证"测试后显示
//! 结果"与"保存完成"契约。

#![cfg(unix)]

use std::path::PathBuf;
use tui_test::{KeyAction, OpenOptions, Operation, Session};

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

fn isolated_env() -> (TempDirGuard, String) {
    let dir = std::env::temp_dir().join(format!("aemeath-tui-connect-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建临时 agents dir");
    // 有效配置（DeepSeek）保证主 TUI 启动；向导里选 Anthropic 新建。
    std::fs::write(
        dir.join("aemeath.json"),
        r#"{"models":{"providers":{"DeepSeek":{"driver":"deepseek","baseUrl":"https://api.deepseek.com","apiKey":"sk-preconfigured","models":[{"id":"deepseek-flash","contextWindow":1000,"maxTokens":100}]}}}}"#,
    )
    .expect("写入预置配置");
    let command = format!(
        "AEMEATH_AGENTS_DIR={} {} connect",
        dir.to_string_lossy(),
        binary_path().to_string_lossy()
    );
    (TempDirGuard(dir), command)
}

#[test]
fn connect_wizard_full_flow_shows_probe_result() {
    let (_guard, command) = isolated_env();
    let mut open = OpenOptions::default();
    open.restart = true;
    let terminal = Session::new(format!("aemeath-connect-wizard-{}", std::process::id()));
    terminal.open(open).expect("打开终端");

    // Submit：数据 + 自动回车；Key：单键无回车；Write：原始输入无回车。
    let submit = |data: &str| {
        terminal
            .execute(Operation::Submit {
                data: Some(data.to_string()),
            })
            .expect("提交");
    };
    let key = |name: &str| {
        terminal
            .execute(Operation::Key {
                keys: vec![name.to_string()],
                action: KeyAction::Press,
            })
            .expect("按键");
    };
    let pause = || std::thread::sleep(std::time::Duration::from_millis(400));
    // 轮询全屏文本（跨 span 拼接）包含 needle。
    let wait_screen = |needle: &str, step: &str, timeout_ms: u64| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            if let Ok(tui_test::OperationResult::Text(text)) =
                terminal.execute(Operation::Text { full: true })
            {
                if text.contains(needle) {
                    return;
                }
            }
            if std::time::Instant::now() > deadline {
                let screen = terminal
                    .execute(Operation::Text { full: true })
                    .map(|result| match result {
                        tui_test::OperationResult::Text(text) => text,
                        _ => String::new(),
                    })
                    .unwrap_or_default();
                panic!("{step}（等待 {needle}）超时；屏幕：\n{screen}");
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    };

    // 0. 主 TUI 启动并自动打开向导
    submit(&command);
    wait_screen("● Anthropic", "向导首页", 30_000);
    // 1. 回车选中首项 Anthropic（新建流程）
    submit("");
    wait_screen("api.anthropic.com", "endpoint 页", 10_000);
    // 2. Ctrl+U 清空预填，输入不可路由端点（Submit 自带回车）
    key("c-u");
    pause();
    submit("http://127.0.0.1:1");
    // 3. API Key
    pause();
    submit("sk-tui-test");
    // 4. UA：Anthropic 有 catalog 官方 UA 预填，直接回车
    wait_screen("claude-cli", "UA 页", 10_000);
    submit("");
    // 5. 模型页：→ 进入 action 区（添加/编辑模型可达），先验证编辑页
    //    入口再勾选提交（全局默认已并入编辑页，无独立 6/8 页）
    wait_screen("claude-fable", "模型页", 10_000);
    key("right");
    pause();
    wait_screen("[添加模型]", "action 区高亮", 5_000);
    submit("");
    wait_screen("Model ID", "进入模型编辑页", 10_000);
    // Esc 返回模型页，勾选首项提交
    key("escape");
    pause();
    wait_screen("claude-fable", "返回模型页", 10_000);
    key("space");
    pause();
    submit("");
    // 6. 测试页：回车触发 primary（测试连接）
    wait_screen("跳过测试", "测试页", 10_000);
    submit("");
    // 8. 断言探测结果：不可路由端点必须显示失败详情
    wait_screen("失败", "探测结果", 30_000);
    // 9. 继续 → Review → 保存
    pause();
    submit("");
    pause();
    key("tab");
    pause();
    submit("");
    wait_screen("配置已保存", "保存完成", 10_000);

    terminal.close().expect("关闭会话");
}
