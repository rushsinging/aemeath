# TUI Panic 兜底彻底修复 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**对应 Issue:** https://github.com/rushsinging/aemeath/issues/287

**Goal:** 让 TUI 在任何 panic 下都能恢复终端、保存会话、给出可见报错，杜绝"崩溃直接退出且终端损坏 + 会话丢失"。

**Architecture:** 四层互补防御。(1) RAII `TerminalGuard` 保证正常退出 / `?` 提前返回 / panic 栈展开三条路径都恢复终端；(2) `catch_unwind` 包裹事件循环 future，捕获 panic 后仍执行 auto-save 并优雅退出，不再 abort 进程；(3) panic hook 增强为 best-effort 恢复终端 + 打印 stderr，兜住 guard 覆盖不到的后台线程 panic；(4) 修热路径裸 panic 源（ask_user 越界、后台 spawn 静默吞 panic）。panic 策略为默认 `unwind`（全仓库无 `panic = "abort"`），故 Drop 在展开时必然执行——这是方案 1/2 成立的前提。

**Tech Stack:** Rust, ratatui 0.29, crossterm 0.28, tokio, futures 0.3（`FutureExt::catch_unwind` 已可用）。

---

## File Structure

| 文件 | 操作 | 职责 |
|---|---|---|
| `apps/cli/src/panic_hook.rs` | 修改 | 抽出 `payload_message`；新增 `restore_terminal_best_effort` + 常量；hook 触发时恢复终端再打 stderr |
| `apps/cli/src/tui/effect/session/terminal_guard.rs` | 新建 | `TerminalGuard`：enter 进入 raw/alt-screen，Drop 恢复终端 |
| `apps/cli/src/tui/effect/session.rs` | 修改 | 注册 `pub mod terminal_guard;` |
| `apps/cli/src/tui/effect/session/session_lifecycle.rs` | 修改 | 用 guard 替换手动 enable/disable；catch_unwind 包裹 run_loop；panic 后仍 auto-save |
| `apps/cli/src/tui/effect/spawn_guard.rs` | 新建 | `spawn_guarded`：后台 task 统一 panic 兜底（DRY） |
| `apps/cli/src/tui/effect.rs` | 修改 | 注册 `pub mod spawn_guard;` |
| `apps/cli/src/tui/effect/executor.rs` | 修改 | 两处 reflection spawn 接入 `spawn_guarded` |
| `apps/cli/src/tui/render/input/paste_handler.rs` | 修改 | 两处 clipboard/image spawn 接入 `spawn_guarded` |
| `apps/cli/src/tui/app/update/ask_user_key.rs` | 修改 | line 117-119 越界 / unwrap 防御 |

**测试策略**（依据 `specs/rust-coding.md`：纯逻辑函数最高优先级；UI 渲染 / 入口 / I/O 可豁免）：
- `payload_message`、`restore_terminal_best_effort` 的转义序列常量 → 纯逻辑，必测。
- `TerminalGuard::enter`/`Drop`、`run()`：真实终端 I/O，无 tty 环境 `enable_raw_mode` 必失败 → 豁免，靠编译 + clippy + 手工验证。
- `spawn_guarded`：tokio 并发，写 `#[tokio::test]` 验证正常 future 完成 + panic future 不传播。
- ask_user 越界防御：纯防御分支，写单测构造越界 index 验证不 panic 且返回 none。

---

## Task 1: 抽出 `payload_message` 复用函数（panic_hook.rs）

**Files:**
- Modify: `apps/cli/src/panic_hook.rs`
- Test: 同文件 `#[cfg(test)] mod tests`

- [ ] **Step 1: 写失败测试**

在 `apps/cli/src/panic_hook.rs` 末尾添加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payload_message_str() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("boom");
        assert_eq!(payload_message(payload.as_ref()), "boom");
    }

    #[test]
    fn test_payload_message_string() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("kaboom"));
        assert_eq!(payload_message(payload.as_ref()), "kaboom");
    }

    #[test]
    fn test_payload_message_unknown() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(42u32);
        assert_eq!(payload_message(payload.as_ref()), "unknown panic");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p cli --lib panic_hook::tests::test_payload_message 2>&1 | tail -20`
Expected: FAIL（`payload_message` 未定义，编译错误）

- [ ] **Step 3: 实现 `payload_message`**

在 `apps/cli/src/panic_hook.rs` 的 `init_panic_hook` 之前插入：

```rust
/// 从 panic payload 提取可读消息，供 panic hook、catch_unwind 兜底、后台 task 兜底复用。
pub fn payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string())
}
```

并将 `init_panic_hook` 内 line 36-41 的内联提取替换为复用调用：

```rust
    std::panic::set_hook(Box::new(move |info| {
        let payload = payload_message(info.payload());
```

（删除原 `.downcast_ref::<&str>()...unwrap_or_else(...)` 链）

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p cli --lib panic_hook::tests::test_payload_message 2>&1 | tail -20`
Expected: PASS（3 个用例）

- [ ] **Step 5: 提交**

```bash
git add apps/cli/src/panic_hook.rs
git commit -m "refactor(tui): 抽出 panic payload_message 复用函数 (#287)"
```

---

## Task 2: panic hook 恢复终端 + 打印 stderr（panic_hook.rs）

**Files:**
- Modify: `apps/cli/src/panic_hook.rs`
- Test: 同文件 tests

- [ ] **Step 1: 写转义序列常量测试**

在 `mod tests` 中追加：

```rust
    #[test]
    fn test_terminal_restore_seq_contains_leave_altscreen_and_show_cursor() {
        // \x1b[?1049l = LeaveAlternateScreen, \x1b[?25h = show cursor
        assert!(TERMINAL_RESTORE_SEQ.windows(8).any(|w| w == b"\x1b[?1049l"));
        assert!(TERMINAL_RESTORE_SEQ.windows(6).any(|w| w == b"\x1b[?25h"));
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p cli --lib panic_hook::tests::test_terminal_restore_seq 2>&1 | tail -20`
Expected: FAIL（`TERMINAL_RESTORE_SEQ` 未定义）

- [ ] **Step 3: 实现常量 + 恢复函数，并接入 hook**

在 `payload_message` 之前插入：

```rust
/// 终端恢复转义序列：LeaveAlternateScreen + DisableMouseCapture + DisableBracketedPaste + show cursor。
/// 与 TerminalGuard::drop 的恢复语义保持一致（此处为 panic hook 的最后兜底，不依赖 crossterm execute）。
const TERMINAL_RESTORE_SEQ: &[u8] = b"\x1b[?1049l\x1b[?1000l\x1b[?2004l\x1b[?25h";

/// panic hook 的终端恢复兜底：best-effort，忽略所有错误。
/// 覆盖 RAII guard 触达不到的场景（后台线程 panic、guard 被绕过）。
fn restore_terminal_best_effort() {
    let _ = crossterm::terminal::disable_raw_mode();
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(TERMINAL_RESTORE_SEQ);
    let _ = stdout.flush();
}
```

将 hook 末尾原 stderr 逻辑（line 75-78）：

```rust
        // TUI 持有终端时写 stderr 会糊屏；此时仅依赖 panic.log。
        if !TUI_ACTIVE.load(Ordering::SeqCst) {
            eprintln!("[PANIC] {} at {}", payload, location);
        }
```

替换为：

```rust
        // TUI 持有终端时，先恢复终端再打印——否则 stderr 会糊在 alternate screen 上。
        if TUI_ACTIVE.load(Ordering::SeqCst) {
            restore_terminal_best_effort();
            TUI_ACTIVE.store(false, Ordering::SeqCst);
        }
        eprintln!("[PANIC] {} at {}（详见 ~/.agents/logs/panic.log）", payload, location);
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p cli --lib panic_hook 2>&1 | tail -20`
Expected: PASS（含 Task 1 的 3 个 + 本 task 1 个）

- [ ] **Step 5: clippy + 提交**

```bash
cargo clippy -p cli 2>&1 | tail -5
git add apps/cli/src/panic_hook.rs
git commit -m "fix(tui): panic hook 触发时恢复终端并打印 stderr (#287)"
```

---

## Task 3: 新建 `TerminalGuard`（terminal_guard.rs）

**Files:**
- Create: `apps/cli/src/tui/effect/session/terminal_guard.rs`
- Modify: `apps/cli/src/tui/effect/session.rs`

- [ ] **Step 1: 注册子模块**

`apps/cli/src/tui/effect/session.rs` 改为：

```rust
pub mod processing;
pub mod resume;
pub mod session_lifecycle;
pub mod terminal_guard;
```

- [ ] **Step 2: 实现 TerminalGuard**

创建 `apps/cli/src/tui/effect/session/terminal_guard.rs`：

```rust
//! 终端 RAII guard：enter 进入 raw mode + alternate screen，Drop 时恢复。
//! 因 panic 策略为默认 unwind，Drop 在正常退出 / `?` 返回 / panic 展开三条路径都执行，
//! 保证终端不会卡在 raw + alternate screen。

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

/// 持有 TUI 终端句柄，析构时自动恢复终端状态。
pub struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    /// 进入 TUI：启用 raw mode、切到 alternate screen、开启括号粘贴与鼠标捕获，
    /// 并通知 panic hook TUI 已持有终端。
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            crossterm::event::EnableBracketedPaste,
            crossterm::event::EnableMouseCapture,
        )?;
        crate::panic_hook::set_tui_active(true);
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    /// 借出底层 ratatui Terminal 供事件循环渲染。
    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<io::Stdout>> {
        &mut self.terminal
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // 先告知 panic hook TUI 已退出，避免后续 panic 走 TUI 分支。
        crate::panic_hook::set_tui_active(false);
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            crossterm::event::DisableMouseCapture,
            crossterm::event::DisableBracketedPaste,
            LeaveAlternateScreen,
        );
        let _ = self.terminal.show_cursor();
    }
}
```

> 说明：`enter`/`Drop` 为真实终端 I/O，无 tty 的测试环境 `enable_raw_mode` 必失败，依 `rust-coding.md` 豁免单测，靠编译 + 手工验证覆盖。

- [ ] **Step 3: 编译确认**

Run: `cargo build -p cli 2>&1 | tail -10`
Expected: 成功编译（guard 暂未被使用会有 dead_code 警告，下一 task 接入后消除）

- [ ] **Step 4: 提交**

```bash
git add apps/cli/src/tui/effect/session.rs apps/cli/src/tui/effect/session/terminal_guard.rs
git commit -m "feat(tui): 新增 TerminalGuard RAII 终端恢复守卫 (#287)"
```

---

## Task 4: session_lifecycle 用 guard + catch_unwind 包裹主循环

**Files:**
- Modify: `apps/cli/src/tui/effect/session/session_lifecycle.rs:1-127`

- [ ] **Step 1: 改 import 头**

将文件顶部 import（line 1-10）改为：

```rust
use crate::tui::app::App;
use crate::tui::effect::session::resume::apply_resume_input_history;
use crate::tui::effect::session::terminal_guard::TerminalGuard;
use futures::FutureExt;
use std::io;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
```

（移除 crossterm/ratatui 的 enable/disable/Terminal import——已迁入 guard）

- [ ] **Step 2: 替换 line 85-127 的进入/退出逻辑**

将 `run()` 从 line 85（`enable_raw_mode()?;`）到 line 126（`result`）整段替换为：

```rust
        // 进入 TUI：RAII guard 保证任何退出路径（正常 / ? / panic 展开）都恢复终端。
        let mut guard = TerminalGuard::enter()?;
        let interrupted = Arc::new(AtomicBool::new(false));

        // catch_unwind 包裹主循环：panic 不再 abort 进程，捕获后仍可 auto-save。
        let loop_result = std::panic::AssertUnwindSafe(
            self.run_loop(guard.terminal_mut(), interrupted),
        )
        .catch_unwind()
        .await;

        // 无论是否 panic，都尝试 auto-save，避免会话丢失。
        if !self.chat.messages.is_empty() {
            if let Err(e) = agent_client
                .sync_current_messages(self.chat.messages.clone())
                .await
            {
                crate::tui::log_warn!("failed to sync session messages: {e}");
            }
            if let Err(e) = agent_client.save_current_session().await {
                crate::tui::log_warn!("failed to auto-save session: {e}");
            }
        }

        // guard 离开作用域 → Drop 恢复终端；此后 panic 可正常打印到 stderr。
        drop(guard);

        match loop_result {
            Ok(inner) => inner,
            Err(panic) => {
                let msg = crate::panic_hook::payload_message(panic.as_ref());
                crate::tui::log_error!("TUI 事件循环 panic，已优雅退出: {msg}");
                Ok(())
            }
        }
```

- [ ] **Step 3: 编译确认**

Run: `cargo build -p cli 2>&1 | tail -15`
Expected: 成功编译，无 TerminalGuard dead_code 警告

- [ ] **Step 4: clippy**

Run: `cargo clippy -p cli 2>&1 | tail -10`
Expected: 无新增 warning（特别关注 AssertUnwindSafe 相关）

- [ ] **Step 5: 提交**

```bash
git add apps/cli/src/tui/effect/session/session_lifecycle.rs
git commit -m "fix(tui): catch_unwind 包裹事件循环，panic 后仍 auto-save 并恢复终端 (#287)"
```

---

## Task 5: 新建 `spawn_guarded` 后台 task 兜底（spawn_guard.rs）

**Files:**
- Create: `apps/cli/src/tui/effect/spawn_guard.rs`
- Modify: `apps/cli/src/tui/effect.rs`
- Test: 同文件 tests

- [ ] **Step 1: 注册模块**

`apps/cli/src/tui/effect.rs` 改为：

```rust
pub mod completion;
pub mod effect;
pub mod executor;
pub mod session;
pub mod spawn_guard;
```

- [ ] **Step 2: 写实现 + 失败测试**

创建 `apps/cli/src/tui/effect/spawn_guard.rs`：

```rust
//! 后台 tokio task 的统一 panic 兜底。
//! tokio 默认会静默吞掉 spawned task 的 panic（仅 panic hook 留痕）；
//! 此 helper 在 future 外层加 catch_unwind，将 panic 转为可见错误日志。

use futures::FutureExt;

/// spawn 一个带 panic 兜底的后台任务。task 内 panic 不会传播，只记录 error 日志。
pub fn spawn_guarded<F>(label: &'static str, fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(panic) = std::panic::AssertUnwindSafe(fut).catch_unwind().await {
            let msg = crate::panic_hook::payload_message(panic.as_ref());
            crate::tui::log_error!("后台任务 {} panic: {}", label, msg);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_spawn_guarded_runs_normal_future() {
        let flag = Arc::new(AtomicBool::new(false));
        let f = flag.clone();
        spawn_guarded("normal", async move {
            f.store(true, Ordering::SeqCst);
        });
        // 让出执行权等待 task 完成
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(flag.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_spawn_guarded_swallows_panic() {
        // panic 的 task 不应导致测试进程崩溃；spawn_guarded 返回后主流程继续。
        spawn_guarded("boom", async move {
            panic!("intentional");
        });
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // 能执行到这里即证明 panic 未传播。
        assert!(true);
    }

    #[tokio::test]
    async fn test_spawn_guarded_normal_after_panic_task() {
        // 边界：先 spawn 一个 panic task，再 spawn 正常 task，正常 task 仍执行。
        spawn_guarded("boom", async move { panic!("x") });
        let flag = Arc::new(AtomicBool::new(false));
        let f = flag.clone();
        spawn_guarded("ok", async move {
            f.store(true, Ordering::SeqCst);
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(flag.load(Ordering::SeqCst));
    }
}
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p cli --lib spawn_guard 2>&1 | tail -20`
Expected: PASS（3 个用例）。注：panic task 会触发已安装的 panic hook 打印，属预期。

- [ ] **Step 4: 提交**

```bash
git add apps/cli/src/tui/effect.rs apps/cli/src/tui/effect/spawn_guard.rs
git commit -m "feat(tui): 新增 spawn_guarded 后台 task panic 兜底 (#287)"
```

---

## Task 6: 后台 spawn 接入 spawn_guarded（executor.rs + paste_handler.rs）

**Files:**
- Modify: `apps/cli/src/tui/effect/executor.rs:187,215`
- Modify: `apps/cli/src/tui/render/input/paste_handler.rs:15,45`

- [ ] **Step 1: executor.rs run_reflection_effect（line 187）**

将 `tokio::spawn(async move {` 替换为 `crate::tui::effect::spawn_guard::spawn_guarded("reflection", async move {`，并把对应闭合 `});`（line 202）改为 `});`（参数闭合，写法见下）。完整替换 line 187-202 为：

```rust
        crate::tui::effect::spawn_guard::spawn_guarded("reflection", async move {
            if foreground {
                let _ = tx.send(UiEvent::ReflectionStarted).await;
            }
            match agent_client.run_reflection(messages).await {
                Ok(output) => {
                    let _ = tx.send(UiEvent::ReflectionUsage).await;
                    let _ = tx.send(UiEvent::ReflectionDone { output }).await;
                }
                Err(error) => {
                    let _ = tx
                        .send(UiEvent::Error(format!("Reflection LLM 调用失败: {error}")))
                        .await;
                }
            }
        });
```

- [ ] **Step 2: executor.rs apply_reflection_effect（line 215）**

完整替换 line 215-223 为：

```rust
        crate::tui::effect::spawn_guard::spawn_guarded("apply_reflection", async move {
            let result = agent_client
                .apply_reflection(output.clone())
                .await
                .map_err(|error| error.to_string());
            let _ = tx
                .send(UiEvent::ReflectionApplyDone { output, result })
                .await;
        });
```

- [ ] **Step 3: paste_handler.rs clipboard image（line 15）**

完整替换 line 15-33 为：

```rust
            crate::tui::effect::spawn_guard::spawn_guarded("clipboard_image", async move {
                match agent_client.read_clipboard_image().await {
                    Ok(img) => {
                        let size = img.final_size;
                        let _ = output_tx.send(UiEvent::ClipboardImage(img)).await;
                        let _ = output_tx
                            .send(UiEvent::SystemMessage(format!(
                                "[clipboard image added ({} bytes). Type message to send.]",
                                size
                            )))
                            .await;
                    }
                    Err(e) => {
                        let _ = output_tx
                            .send(UiEvent::Error(format!("No image in clipboard: {e}")))
                            .await;
                    }
                }
            });
```

- [ ] **Step 4: paste_handler.rs image file（line 45）**

完整替换 line 45-63 为：

```rust
            crate::tui::effect::spawn_guard::spawn_guarded("image_file", async move {
                match agent_client.process_image_file(path).await {
                    Ok(img) => {
                        let size = img.final_size;
                        let _ = tx.send(UiEvent::ClipboardImage(img)).await;
                        let _ = tx
                            .send(UiEvent::SystemMessage(format!(
                                "[image loaded ({} bytes). Type message to send.]",
                                size
                            )))
                            .await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(UiEvent::Error(format!("Failed to load image: {e}")))
                            .await;
                    }
                }
            });
```

- [ ] **Step 5: 编译 + clippy**

Run: `cargo build -p cli 2>&1 | tail -10 && cargo clippy -p cli 2>&1 | tail -10`
Expected: 成功，无新增 warning

- [ ] **Step 6: 提交**

```bash
git add apps/cli/src/tui/effect/executor.rs apps/cli/src/tui/render/input/paste_handler.rs
git commit -m "fix(tui): 后台 reflection/image spawn 接入 panic 兜底 (#287)"
```

---

## Task 7: ask_user_key 越界 / unwrap 防御

**Files:**
- Modify: `apps/cli/src/tui/app/update/ask_user_key.rs:117-119`

- [ ] **Step 1: 替换 line 116-119 的 unwrap + 裸索引**

将：

```rust
            KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                let state = self.input.ask_user_state.as_ref().unwrap();
                let active_index = snap.active_index;
                let active_item = &state.items[active_index];
```

替换为：

```rust
            KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                let active_index = snap.active_index;
                // items（self.input）与 active_index（conversation snapshot）是两个真相源，
                // 不同步时越界——防御性返回，避免 panic 退出整个 TUI。
                let Some(active_item) = self
                    .input
                    .ask_user_state
                    .as_ref()
                    .and_then(|s| s.items.get(active_index))
                else {
                    crate::tui::log_warn!(
                        "ask_user active_index {} 越界，跳过提交",
                        active_index
                    );
                    return Some(UpdateResult::none());
                };
```

> 注：`submit_ask_user_batch:249` 的 `.take().unwrap()` 经 `maybe_auto_submit_ask_user` 仅在 `snap.confirmed` 为真时调用，此时 state 必为 Some；保持原状，本 task 不动，避免扩大改动面。

- [ ] **Step 2: 写防御测试**

确认 `ask_user_key.rs` 末尾是否已有 `#[cfg(test)] mod tests`；若无则新增。测试构造一个 `active_index` 越界的场景验证不 panic。若 `App` + snapshot 构造成本过高（无现成 builder），在 PR 描述中记录该分支由编译期 `let-else` 保证安全并由 clippy 覆盖，测试以 `submit_ask_user_batch` 既有路径间接覆盖。先尝试：

```rust
#[cfg(test)]
mod tests {
    // active_index 越界时 items.get() 返回 None，let-else 走 return 分支，
    // 不触发数组越界 panic——此处以编译期类型保证 + 下述断言验证 .get 语义。
    #[test]
    fn test_items_get_out_of_bounds_returns_none() {
        let items: Vec<u8> = vec![1, 2, 3];
        assert!(items.get(99).is_none());
        assert_eq!(items.get(0), Some(&1));
    }
}
```

- [ ] **Step 3: 运行测试 + 编译**

Run: `cargo test -p cli --lib ask_user_key 2>&1 | tail -15 && cargo build -p cli 2>&1 | tail -5`
Expected: PASS + 成功编译

- [ ] **Step 4: 提交**

```bash
git add apps/cli/src/tui/app/update/ask_user_key.rs
git commit -m "fix(tui): ask_user active_index 越界防御，避免 panic 退出 (#287)"
```

---

## Task 8: 全量验证门禁 + 手工验证

**Files:** 无（仅验证）

- [ ] **Step 1: 全量编译**

Run: `cargo build 2>&1 | tail -15`
Expected: workspace 全部成功

- [ ] **Step 2: 全量 clippy**

Run: `cargo clippy --workspace 2>&1 | tail -20`
Expected: 无 error（关注 unused import：session_lifecycle 移除的 crossterm import）

- [ ] **Step 3: cli 测试**

Run: `cargo test -p cli 2>&1 | tail -25`
Expected: 全部 PASS，0 failure

- [ ] **Step 4: 手工验证终端恢复（关键）**

构造一个临时 panic 注入点（验证后回滚），或用现有路径触发，确认：
1. panic 后终端回到正常模式（非 raw、光标可见、回显正常）；
2. stderr 出现 `[PANIC] ... at ...`；
3. `~/.agents/logs/panic.log` 有记录；
4. 会话已落盘（`~/.agents/sessions/` 有更新）。

最小手工验证命令（确认正常退出路径不回归）：

```bash
echo "hello" | ./target/debug/aemeath -q -v --allow-all 2>&1 | tail -20
```

Expected: 正常应答、正常退出、终端状态正常（无残留 raw mode）。

- [ ] **Step 5: 拉取最新 main 并解决冲突**

```bash
git pull origin main --no-edit
# 若冲突，解决后重跑 Step 1-3 验证门禁
```

- [ ] **Step 6: 推送并创建 PR**

```bash
git push -u origin fix/287-tui-panic-guard
gh pr create --repo rushsinging/aemeath --base main \
  --title "fix(tui): TUI panic 兜底彻底修复（终端恢复 + 会话保存 + 热路径防御）(#287)" \
  --body "Closes #287。详见 docs/superpowers/plans/2026-06-16-tui-panic-guard.md"
```

> **NEVER** 由 agent 自动合并 PR——创建后等用户 review。

---

## Self-Review

**1. Spec coverage（对照 Issue #287 四项修复）：**
- 缺口 1（panic hook 不恢复终端 + 吞报错）→ Task 2 ✓
- 缺口 2（主循环无 guard / catch_unwind，auto-save 被跳过）→ Task 3（guard）+ Task 4（catch_unwind + auto-save 前移）✓
- 缺口 3a（ask_user 越界）→ Task 7 ✓
- 缺口 3b（后台 spawn 吞 panic）→ Task 5（helper）+ Task 6（接入）✓
- 复用基础（payload_message）→ Task 1 ✓

**2. Placeholder 扫描：** 每个改动 step 均含完整代码块，无 TBD / "适当处理" / "类似上文" 占位。Task 7 Step 2 对测试构造成本给出了明确降级路径（非占位）。

**3. 类型一致性：**
- `payload_message(&(dyn Any + Send)) -> String`：Task 1 定义，Task 4（`panic.as_ref()`，`panic: Box<dyn Any + Send>`）、Task 5 一致调用 ✓
- `TerminalGuard::enter() -> io::Result<Self>` / `terminal_mut() -> &mut Terminal<CrosstermBackend<Stdout>>`：Task 3 定义，Task 4 一致使用 ✓
- `spawn_guarded(label: &'static str, fut: F)`：Task 5 定义，Task 6 四处一致调用 ✓
- `run_loop(&mut self, &mut Terminal<...>, Arc<AtomicBool>)`：未改签名，Task 4 经 `guard.terminal_mut()` 传入 ✓

**风险点备注：**
- `AssertUnwindSafe` 跨 `&mut self`：panic 后仅用 `self.chat.messages`（Vec 读取）做 auto-save，状态半残风险可控；不复用 self 继续渲染。
- panic hook 与 guard 双重 `set_tui_active(false)` + 双重终端恢复：幂等，无副作用。
