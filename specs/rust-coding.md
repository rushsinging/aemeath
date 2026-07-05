# Rust 编码规范

**Scope**：横切所有 crate 的 Rust 代码规范——编码约束、错误处理、验证门禁、日志、测试。任意 `**/*.rs`、`**/Cargo.toml` 改动均适用本分片。
**不适用**：feature 专属的业务规则在各自分片（`tui-cli.md` / `runtime.md` / …）。

## 编码规范

### NEVER
- **NEVER** 在 core 库中使用 `println!` / `eprintln!`（`lib.rs` 已 deny）。
- **NEVER** 直接读取环境变量——使用配置分层（见 `config-compat.md`）。
- **NEVER** 硬编码 API key、base URL。
- **NEVER** 提交没有单元测试覆盖的新增核心逻辑。

### MUST
- **MUST** 错误消息使用中文（`ErrorDisplay`）。
- **MUST** 遵循 `AemeathError` 错误类型体系。
- **MUST** 配置优先于硬编码默认值。
- **MUST** 异步 trait 方法使用 `async_trait`。
- **MUST** TUI 模式下所有应用主日志路由到 `~/.agents/logs/aemeath.log`。
- **MUST** 新增 `pub fn` 在同一文件末尾添加 `#[cfg(test)] mod tests`。
- **MUST** 单元测试覆盖三种路径：正常路径、边界条件、错误路径。

### SHOULD
- **SHOULD** 单个 `.rs` 文件控制在 400 行以内（含测试代码）；过长时按职责拆分。无强制守卫，超限不阻断构建。
- **SHOULD** 为辅助函数（`private fn`）编写测试，除非是一行委托/包装。
- **SHOULD** 测试命名遵循 `test_<被测函数名>_<场景描述>` 模式。

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
- 架构守卫（`target_guard.rs`）在 CI 中扫描全仓库确保合规。

## 测试规范

### TDD（测试先行）

- **MUST** 新增/修改任何核心逻辑前，**MUST** 先写或改对应测试：feature 先写表达期望行为的测试（初始失败），bug 先写复现 bug 的失败测试，重构先确认现有测试已覆盖目标行为。
- **MUST** 实现/修复后，对应测试 **MUST** 通过（`cargo test -p <crate>` 绿）；测试 **NEVER** 为迁就实现而削弱断言。
- **SHOULD** 遵循 Red → Green → Refactor 节奏：先红（失败测试）、再绿（最小实现通过）、最后重构。
- 豁免沿用下方覆盖豁免（UI 渲染、`main.rs` 入口、一行委托/包装）。

### 覆盖要求

- **MUST** 每个包含公共函数的模块文件末尾有 `#[cfg(test)] mod tests`。
- **MUST** 每个公共函数至少 3 个测试用例：正常路径、边界条件、错误路径。
- **MUST** 测试使用 `assert!` / `assert_eq!` / `matches!` 显式断言，不可仅打印后人工观察。
- **SHOULD** 私有辅助函数通过公有函数间接覆盖，或直接 `use super::*` 导入测试。
- **MUST** 纯逻辑函数（无 I/O、无副作用）为最高优先级测试目标。UI 渲染代码、`main.rs` 入口代码可豁免。
- 一行委托/包装函数可豁免 3 测试用例要求，但仍 SHOULD 有测试。
- Code review 时 reviewer **MUST** 检查新增代码的测试覆盖。未覆盖核心逻辑的 PR 不应合并。

## 调试方法论

### 诊断日志先于推理

- **MUST** 定位 bug 时，**MUST** 优先添加日志确认数据流（事件是否到达、字段是否填充），而不是依赖推理和猜测。
- **SHOULD** 诊断日志先用 `info!` 级别确认链路通，验证通过后降回 `debug!` 或删除——避免被全局日志级别过滤掉看不到。
- **MUST** 全局日志级别由 `logging.level` 配置（默认 `info`），debug 级别日志需要 `AEMEATH_LOG_LEVEL=debug` 环境变量拉高，或 `RUST_LOG=aemeath:<target>=debug` 按 target 拉高。详见 `AGENTS.md` 日志级别说明。
- **SHOULD** 诊断完成后 **SHOULD** 清理诊断日志，避免污染生产日志。

### 链路验证不要跳层

- **MUST** 当 A → B → C → D 链路中 D 不显示时，**MUST NOT** 只查 A/B/C 的传递而假设 D 内部正确。**MUST** 确认 D 内部是否真的调用了消费逻辑（如覆写方法是否绕过了默认调用链）。
- **MUST** 排查前 **MUST** 确认用户跑的是最新编译的二进制（对比 `target/debug/aemeath` 时间戳与 `date`），避免在旧二进制上浪费诊断轮次。
- **SHOULD** 在每一层（share → runtime → sdk → tui）的入口/出口加日志，逐层确认数据传递。

### Trait 默认方法覆写陷阱

- **MUST** Trait 默认方法调用其它默认方法时，**任何覆写都可能切断调用链**。新增字段要想被消费，**MUST** 确认实际被调用的入口（而非"应该被调用"的默认方法）。
- 例：`ToolDisplay::format_header_line_with_result` 覆写了默认实现，绕过了 `format_header`——新增在 `format_header` 中解析的字段永远不会被消费。修正方式：在覆写方法中显式调用消费逻辑。
- **SHOULD** 覆写 trait 方法时，**SHOULD** 在文档注释中说明是否调用默认方法链、以及为什么覆写。

### 架构守卫的价值

- **MUST** 架构守卫不是阻碍，是**设计纠偏**。view_model 不能依赖 model internals 这类守卫强制我们在 view_model 层定义独立类型 + 在 view_assembler 处投影。
- **MUST** 守卫拦截后 **MUST NOT** 绕过，**MUST** 修正设计使其合规。如果没有守卫，反向依赖会成为长期技术债。

### 选型穷举所有 case

- **MUST** 选型时 **MUST** 穷举所有 case，确认方案在每种 case 下都能工作。
- 例：role/model 显示有 4 种组合（None/None、Some/None、None/Some、Some/Some）。方案 A（从 input JSON 解析）在 case 2（只有 role 无 model）失败，因为 model 是 runtime 内部 resolve 的。
- **SHOULD** 做当前需求时，**SHOULD** 思考"这个改动是否为后续需求铺路"。优先选择可扩展的方案（如 AgentProgressEvent 携带元数据，为后续"实时显示 subagent 活动"打下通道基础）。
