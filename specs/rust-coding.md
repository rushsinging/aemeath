# Rust 编码规范

**Scope**：横切所有 crate 的 Rust 代码规范——编码约束、错误处理、验证门禁、日志、测试。任意 `**/*.rs`、`**/Cargo.toml` 改动均适用本分片。
**不适用**：feature 专属的业务规则在各自分片（`tui-cli.md` / `runtime.md` / …）。

## 编码规范

### NEVER
- **NEVER** 在 core 库中使用 `println!` / `eprintln!`（`lib.rs` 已 deny）。
- **NEVER** 直接读取环境变量——使用配置分层（见 `config-compat.md`）。
- **NEVER** 硬编码 API key、base URL。
- **NEVER** 提交没有可追溯测试证据的新增核心逻辑。测试证据 **MUST** 按下方 L0-L5 分层选择，不得用低层测试替代必要的契约、场景或系统验证。

### MUST
- **MUST** 错误消息使用中文（`ErrorDisplay`）。
- **MUST** 遵循 `AemeathError` 错误类型体系。
- **MUST** 配置优先于硬编码默认值。
- **MUST** 异步 trait 方法使用 `async_trait`。
- **MUST** TUI 模式下所有应用主日志路由到 `~/.agents/logs/aemeath.log`。
- **MUST** 新增或修改核心行为时，先确定其测试层级与覆盖证据，并按 TDD 落地；测试可位于同文件、同级 `*_tests.rs`、模块 `tests/`、crate integration test 或场景测试。
- **MUST** 跨层链路改动为每一层补相邻测试或契约测试，并用场景测试证明最终组合；**NEVER** 只测首尾。

### SHOULD
- **SHOULD** 单个生产 `.rs` 文件控制在 400 行以内；测试按职责外置后单独保持可读，一个场景文件 SHOULD 聚焦一个稳定用户旅程。无强制守卫，超限不阻断构建。
- **SHOULD** 私有辅助函数优先通过真实生产入口间接覆盖；只有分支复杂且无法清晰归因时直接测试。
- **SHOULD** 测试命名采用"行为 + 条件 + 结果"，例如 `submit_when_idle_emits_user_message`，不强制 `test_` 前缀。

## 错误处理

- 统一使用 `AemeathError`（`agent/shared/src/error.rs`），`thiserror` derive。
- `ErrorDisplay`（同文件 `agent/shared/src/error.rs`）提供中文用户消息和建议。
- `is_retryable()`（同文件）区分可重试/不可重试错误。

## 验证门禁

- **CLI 编译**：`cargo build` 或 `cargo build -p <crate>`
- **完整检查**：`cargo check` / `cargo clippy`
- **测试**：`cargo test -p <crate>`
- 库层面 `#![deny(clippy::print_stdout, clippy::print_stderr)]`

## 日志规范

- UnifiedLogger 驱动，从配置文件的 `logging.level` 读取全局日志级别。
- 设置 `AEMEATH_LOG_STDERR=1` 可恢复 stderr 输出（用于 `--no-tui` 模式）。

### 日志文件路由

UnifiedLogger 按 `record.target()` 前缀路由到对应文件：

| 文件 | target 前缀 | 来源 crate |
|------|-------------|------------|
| `aemeath.log` | 兜底（无匹配前缀） | shared/composition |
| `runtime.log` | `runtime::` | runtime |
| `provider.log` | `provider::` | provider |
| `tools.log` | `tools::` | tools |
| `prompt.log` | `prompt::` | prompt |
| `tui.log` | `cli::` | cli/tui |
| `hook.log` | `hook::` | hook |

原始记录文件（静态方法直写）：
- `input.log` — 用户输入 + LLM 输入（`log_input` / `log_user_input`）
- `output.log` — LLM 输出（`log_output`）

审计文件：
- `audit.log` — 权限/行为审计（`audit`，预留）

特殊文件：
- `panic.log` — panic_hook 直写，不纳入 UnifiedLogger

### Log 规范

> **日志 level 等级选择**：见 `specs/logging.md` 的「日志级别策略」章节（5 级定义 + per-layer 细则）。选择 `trace/debug/info/warn/error` 时 **MUST** 对照该章节。

- **MUST** 所有 `log::xxx!` 调用显式指定 `target:` 参数，格式为 `target: "crate_name::module"`。
  - 例：`log::info!(target: "runtime::loop_runner", "...")`
  - 例：`log::debug!(target: "provider::client", "...")`
- **NEVER** 在生产代码中使用裸 `log::xxx!` 调用（不带 `target:`）。
- **MUST** TUI 层使用 `crate::tui::log_xxx!` 宏（自动设置 `target: "cli::tui"`）。
- **SHOULD** target 前缀与 crate 名一致（runtime crate 用 `runtime::`，provider crate 用 `provider::`，以此类推）。
- 架构守卫（`packages/global/logging/src/domain/routing_guard.rs`）在 CI 中扫描全仓库确保合规。

## 测试规范

> 全仓测试分层、覆盖率、目录组织、fixture/替身与确定性治理的完整定义见 `docs/design/03-engineering/04-testing-and-coverage.md`。本节仅列出 Rust 代码变更 **MUST** 遵守的操作约束。

### TDD（测试先行）

- **MUST** 新增或修改核心逻辑前先建立失败证据：feature 先写表达期望行为的测试，bug 先写复现测试，重构先确认现有测试覆盖目标行为。
- **MUST** 实现或修复后使对应测试通过；测试 **NEVER** 为迁就实现而削弱断言。
- insta 快照场景 **MUST** 先用语义断言建立失败证据，再生成基线。

### 层级选择

- **MUST** 按 L0-L5 六层模型选择最低充分层级的覆盖证据；层级定义与适用场景见 design doc。
- **MUST** 跨层链路改动为每一层补相邻测试或契约测试，并用场景测试证明最终组合；**NEVER** 只测首尾。
- **NEVER** 用 L4/L5 替代 L1-L3 的状态转换、字段完整性或 Adapter 契约测试，也 **NEVER** 用大量 L1 模拟本应由 L3/L4 验证的跨边界组合。
- **MUST** 使用 `assert!`、`assert_eq!`、`matches!` 或等价断言验证行为，**NEVER** 只打印后人工观察。

### 目录组织

- 测试文件 **MUST** 与源码分离：`foo.rs` ↔ `foo_tests.rs`（同级目录），通过 `#[cfg(test)] #[path = "foo_tests.rs"] mod tests;` 引入。**NEVER** 在源码文件内嵌 `#[cfg(test)] mod tests { ... }`（由 `.agents/hooks/check-no-inline-tests.sh` 守卫）。
  - 收益：`cargo build`（不带 `--cfg test`）天然暴露 dead code——任何只被测试引用的 pub 项会变 unused warning。移除 `*_tests.rs` 后 `cargo build` 即可发现仅在测试中使用的代码。
- L2/L4 测试 **MUST** 使用同名文件与目录并存形状（`tests.rs` + `tests/`），**NEVER** 新增 `mod.rs`。
- 测试模块与 fixture **MUST** 跟随被测能力的真实架构层和模块，**NEVER** 建立跨层万能 `test_utils` / `testing`。
- **NEVER** 新增 `include!("tests/*.rs")` 拼接；**NEVER** 一次性移动全仓历史测试（渐进迁移）。

### 确定性

- 时间、timer、ID、随机源 **MUST** 可注入或固定；文件测试 **MUST** 使用每测试唯一临时目录。
- **NEVER** 用短 `sleep` 或毫秒级墙钟差证明状态重置；**NEVER** 修改进程全局 cwd。
- 测试辅助 API **MUST** 受 `cfg(test)` 或 test-only feature 约束，**NEVER** 因测试方便扩大生产 API。
- CI 首次失败后定向重跑只用于分类 flaky，**NEVER** 用重跑成功覆盖首次失败。
