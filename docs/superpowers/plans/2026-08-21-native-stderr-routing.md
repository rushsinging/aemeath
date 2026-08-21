# 原生 stderr 统一路由实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** TUI 启动时把与 stdout 共用终端的原生 FD 2 追加路由到 `native-stderr.log`，同时保持用户重定向、no-TUI 和工具业务 stderr 语义。

**Architecture:** `LoggingSettings` 增加启动期 `NativeStderrRouting` 策略，Composition 根据 frontend 模式选择策略，`UnifiedLogger::init` 在 Runtime 启动前完成路由。Unix adapter 通过可替换的 FD 操作接口完成检测、建目录、append 打开和 `dup2`；非 Unix 明确空操作。Bash/Hook/MCP 不改动，依靠现有 pipe 边界测试防回归。

**Tech Stack:** Rust、libc、tokio、portable-pty、cargo test、cargo clippy。

---

## 文件结构

- 修改 `packages/global/logging/src/domain/settings.rs`：定义并承载原生 stderr 路由策略。
- 修改 `packages/global/logging/src/lib.rs`：发布策略类型。
- 新建 `packages/global/logging/src/adapters/native_stderr.rs`：Unix FD 检测与路由；非 Unix 空操作。
- 新建 `packages/global/logging/src/adapters/native_stderr_tests.rs`：决策和 adapter 协作测试。
- 修改 `packages/global/logging/src/adapters.rs`：注册新 adapter。
- 修改 `packages/global/logging/src/adapters/file_sink.rs`：logger 安装前调用路由。
- 修改 `packages/global/logging/src/domain/settings_tests.rs`、`adapters/file_sink_tests.rs`：更新构造与启动失败契约。
- 修改 `agent/composition/src/app.rs`：由 frontend 输出模式唯一映射路由策略，并保持同一 logs_dir。
- 修改 `packages/sdk/src/bootstrap.rs`：发布区分 TUI 与 no-TUI 的启动期 stderr 策略输入。
- 修改 `apps/cli/src/args.rs`：TUI 选择路由、quiet 选择保留。
- 修改 `apps/cli/tests/pty_smoke.rs`：真实 PTY 验证屏幕无原生 stderr、文件保留原文。
- 修改 `specs/3.15-logging.md`：登记 `native-stderr.log` 的非结构化诊断边界。

### Task 1：建立策略与决策失败测试

- [ ] **Step 1：在 `settings_tests.rs` 写失败测试**

验证 `LoggingSettings` 能区分 `NativeStderrRouting::AppendToFile` 与 `Preserve`，并生成 `<logs_dir>/native-stderr.log`。

- [ ] **Step 2：运行测试确认 RED**

Run: `cargo test -p logging domain::settings::tests -- --nocapture`
Expected: FAIL，缺少 `NativeStderrRouting` 或相关 accessor。

- [ ] **Step 3：实现最小 domain 类型**

在 `settings.rs` 定义：

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeStderrRouting {
    Preserve,
    AppendToFile,
}
```

给 `LoggingSettings::new` 增加该参数，并提供 `native_stderr_routing()` 与 `native_stderr_path()`；后者只从 `logs_dir` 派生固定文件名，禁止调用方重复拼接。

- [ ] **Step 4：更新现有构造并跑 GREEN**

所有现有测试默认传 `NativeStderrRouting::Preserve`；运行同一命令，Expected: PASS。

### Task 2：实现可测试的 Unix 路由 adapter

- [ ] **Step 1：写启用决策失败测试**

在 `native_stderr_tests.rs` 以 fake `NativeStderrOps` 覆盖：

- Preserve 永不检查/替换 FD；
- stderr 非 TTY 不替换；
- stdout 非 TTY 不替换；
- 两个 FD 不是同一终端不替换；
- 同一终端时 create_dir_all、append 打开并替换 FD 2；
- 打开失败不调用 replace；
- replace 失败返回包含阶段和路径的错误。

- [ ] **Step 2：运行测试确认 RED**

Run: `cargo test -p logging native_stderr -- --nocapture`
Expected: FAIL，模块与路由函数不存在。

- [ ] **Step 3：实现最小 adapter**

定义私有 `NativeStderrOps`，生产实现使用：

```rust
libc::isatty(fd)
libc::fstat(fd, &mut stat)
libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO)
```

同终端身份比较 `st_dev` 与 `st_ino`。目标文件使用 `OpenOptions::new().create(true).append(true)`。所有 `unsafe` 块附 SAFETY 说明，错误包含阶段与路径。

- [ ] **Step 4：运行 adapter 测试确认 GREEN**

Run: `cargo test -p logging native_stderr -- --nocapture`
Expected: PASS。

### Task 3：接入 UnifiedLogger 启动边界

- [ ] **Step 1：写初始化失败契约测试**

扩展 `file_sink_tests.rs`：注入失败的 native stderr router，断言 logger 安装前返回错误，且不会产生半初始化 logger。

- [ ] **Step 2：运行测试确认 RED**

Run: `cargo test -p logging adapters::file_sink::tests -- --nocapture`
Expected: FAIL，`UnifiedLogger::init` 尚未调用 router。

- [ ] **Step 3：最小接入**

在 `UnifiedLogger::init` 开头根据 `LoggingSettings` 调用 native stderr router，成功后再 build/set_logger。Preserve 路径无系统调用。路由文件不进入 14 字段 formatter。

- [ ] **Step 4：运行 logging crate 全测试**

Run: `cargo test -p logging`
Expected: PASS。

### Task 4：把 frontend 意图传到 Composition

- [ ] **Step 1：写 SDK/CLI 映射失败测试**

扩展 `apps/cli/src/args.rs` 测试：

- 默认 TUI → `AppendToFile`；
- TUI verbose → `AppendToFile`；
- quiet → `Preserve`；
- quiet verbose → `Preserve`。

- [ ] **Step 2：运行测试确认 RED**

Run: `cargo test -p cli args::tests -- --nocapture`
Expected: FAIL，bootstrap 参数无原生 stderr 策略。

- [ ] **Step 3：发布最小 SDK 输入并映射**

在 `sdk::ChatBootstrapArgs` 增加不暴露 libc 细节的枚举：

```rust
pub enum NativeStderrMode {
    Preserve,
    RouteToLogs,
}
```

CLI 根据 `quiet` 映射该字段；Composition 将其唯一转换为 logging 的 `NativeStderrRouting`。

- [ ] **Step 4：写 Composition 单一 logs_dir 测试**

验证自定义 `logging.logs_dir` 同时成为 UnifiedLogger 与 native stderr 目标目录；默认值遵循 agents root。

- [ ] **Step 5：运行 SDK、Composition、CLI 定向测试**

Run: `cargo test -p sdk && cargo test -p composition app::tests -- --nocapture && cargo test -p cli args::tests -- --nocapture`
Expected: PASS。

### Task 5：真实子进程与 PTY 回归

- [ ] **Step 1：增加 PTY 失败场景**

给 CLI 增加仅测试二进制使用的隐藏环境触发点，在进入 TUI 后直接向 FD 2 写固定 marker；PTY smoke 断言当前实现下 marker 出现在 PTY，形成 RED。触发点必须由 `cfg`/测试 helper 限制，不能成为生产用户接口。

- [ ] **Step 2：运行 L5 测试确认 RED**

Run: `cargo build -p cli --bin aemeath && AEMEATH_PTY_BIN=target/debug/aemeath cargo test -p cli --test pty_smoke native_stderr -- --ignored --nocapture`
Expected: FAIL，marker 污染 PTY 或日志文件不存在。

- [ ] **Step 3：完成真实路由场景**

PTY 测试断言：

- alternate screen 已进入；
- PTY 输出不包含 marker；
- `$AEMEATH_AGENTS_DIR/logs/native-stderr.log` 包含 marker；
- 双 Ctrl+C 后终端正常恢复。

另加非 TTY stderr 子进程场景，断言用户指定文件仍收到 marker，native 文件不截获。

- [ ] **Step 4：运行 L5 测试确认 GREEN**

Run 同上，Expected: PASS。

### Task 6：工具边界与文档

- [ ] **Step 1：运行工具 stderr 边界测试**

Run: `cargo test -p tools bash -- --nocapture && cargo test -p hook process -- --nocapture && cargo test -p tools mcp -- --nocapture`
Expected: PASS，证明显式 pipe 语义未改变。

- [ ] **Step 2：更新日志规范**

在 `specs/3.15-logging.md` 文件表登记 `native-stderr.log`：来源为当前进程 native/FFI 与意外继承 FD 2 的辅助进程；非 JSONL、不属于 TargetCatalog、不接收 Bash/Hook/MCP 业务 stderr。

- [ ] **Step 3：格式与定向验证**

Run: `cargo fmt --check && cargo test -p logging && cargo test -p composition app::tests && cargo test -p cli`
Expected: 全部 PASS 且无 warning。

### Task 7：全仓门禁与交付

- [ ] **Step 1：运行架构守卫**

Run: `bash .agents/hooks/check-architecture-guards.sh`
Expected: PASS。

- [ ] **Step 2：运行全仓测试**

Run: `cargo test --workspace`
Expected: PASS。

- [ ] **Step 3：运行全仓 Clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS，无 warning。

- [ ] **Step 4：清理检查**

检查 `git diff --check`、`git status`、重复 stderr 路由、废弃 panic/TUI stderr 绕路和测试专属生产入口；发现结构性遗留则在本 Issue 范围内清理并重新验证。

- [ ] **Step 5：更新 Issue checklist、提交并创建 PR**

PR 使用 `Closes #1597`，描述根因、stderr 边界、L0-L5 证据与日志级别规则；等待 required checks 通过后报告，不自行合并。
