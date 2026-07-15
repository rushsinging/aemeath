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
- **SHOULD** 测试命名采用“行为 + 条件 + 结果”，例如 `submit_when_idle_emits_user_message`，不强制 `test_` 前缀。

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

> 全仓测试分层、覆盖率与生产可达性目标设计见 `docs/design/03-engineering/04-testing-and-coverage.md`。本节是 Rust 代码变更时必须执行的规范。

### TDD（测试先行）

- **MUST** 新增或修改核心逻辑前先建立失败证据：feature 先写表达期望行为的测试，bug 先写复现测试，重构先确认现有测试覆盖目标行为。
- **MUST** 实现或修复后使对应测试通过；测试 **NEVER** 为迁就实现而削弱断言。
- **SHOULD** 遵循 Red → Green → Refactor：先红、最小实现变绿、最后整理结构。
- insta 等快照首次生成不天然构成 Red。快照场景 **MUST** 先用 Effect payload、状态不变量或关键 cell 等语义断言建立失败证据，再生成并人工审阅快照基线。
- 一行委托/getter 可由上层行为间接覆盖；无逻辑的 `main.rs` 装配入口可由 L5 smoke 覆盖。UI render **NEVER** 整体豁免，必须按 widget Buffer、L4 场景或 L5 PTY 职责提供证据。

### L0-L5 测试层级

| 层级 | 验证责任 | 典型位置 |
|---|---|---|
| L0 编译期约束 | 类型、trait、feature、架构依赖、生产可达性 | compiler、clippy、architecture guards |
| L1 单元测试 | 值对象、纯函数、单条状态转换、局部不变量 | inline `mod tests`、同级 `*_tests.rs` |
| L2 模块协作测试 | 同一 crate 内 service、port、reducer、assembler 协作 | `src/<module>/tests/` |
| L3 契约测试 | Published Language、Port/Adapter、序列化与兼容性 | crate 根 `tests/`、共享 contract suite |
| L4 场景测试 | 跨内部层的用户或业务旅程 | `scenario_tests/` |
| L5 系统 smoke | 真进程、PTY、平台与发布资产 | 独立 CI suite |

- **MUST** 把测试放在能直接验证目标责任的最低充分层级，同时保留跨层链路所需的相邻测试和场景证据。
- **NEVER** 用 L4/L5 的最终结果替代 L1-L3 的状态转换、字段完整性或 Adapter 契约测试。
- **NEVER** 用大量 L1 测试模拟本应由 L3/L4 验证的跨边界组合。

### 按行为选择覆盖证据

| 行为类型 | 必要覆盖证据 |
|---|---|
| 纯函数/parser | 有效等价类、关键边界、错误输入 |
| 状态机/reducer | 重要可达转换、非法转换、状态不变量 |
| Application Service | 成功、Port 失败及适用的幂等/重试/并发语义 |
| Adapter | 共享契约、协议错误、兼容性、资源释放 |
| 序列化类型 | round-trip、缺省字段、旧格式兼容、未知字段策略 |
| TUI widget | 关键尺寸、Unicode、style/cell、选择区域不变量 |
| TUI 场景 | 用户旅程、中间状态、Effect payload、最终 framebuffer |
| 一行委托/getter | 允许由上层真实行为间接覆盖 |

- **MUST** 使用 `assert!`、`assert_eq!`、`matches!` 或等价断言验证行为，**NEVER** 只打印后人工观察。
- 失败消息 **SHOULD** 描述期望不变量和关键上下文，避免只留下无语义的 `left != right`。
- Code review 时 reviewer **MUST** 检查新增行为是否选择了正确测试层级；未覆盖核心逻辑或跳过必要中间层的 PR 不应合并。

### 目录组织

- 小型 L1 测试 **SHOULD** 放在生产文件末尾的 `#[cfg(test)] mod tests`。
- 单一模块的较大 L1 测试 **MAY** 放同级 `*_tests.rs`，由生产文件以 `#[cfg(test)] #[path = "*_tests.rs"] mod tests;` 挂载。
- L2 模块协作测试 **SHOULD** 使用 `src/<owning-layer>/<module>/tests/` 普通模块树，一个文件聚焦一个稳定行为；不分层 crate 可省略 `<owning-layer>`。
- 测试模块与 fixture **MUST** 跟随被测能力的架构层和模块。对于 `domain/application/ports/adapters/shared` 等六边形分层，**NEVER** 为测试便利建立跨层万能 `testing` 层。
- L3 契约测试 **SHOULD** 放 crate 根 `tests/`；共享契约逻辑定义一次，通过 factory/fixture 复用。
- L4 场景测试 **MUST** 将 Harness/fixture 与场景分离，例如 `testing/` 与 `scenario_tests/`，并受 `cfg(test)` 约束。
- **NEVER** 新增 `include!("tests/*.rs")` 拼接测试文件；现存用法在相关模块变更时渐进迁移。
- **NEVER** 为目录统一一次性移动全仓历史测试。新测试立即遵守本规范，旧测试仅在相关行为变更时渐进迁移。

### Fixture 与测试替身

- fixture 与替身 **SHOULD** 按 crate、架构层和领域模块归属，优先放在被测模块相邻的 `testing/` 或测试模块内；**NEVER** 建立知道所有领域类型或跨越架构层的万能 `test_utils` / `testing`。
- `StubX` 返回固定结果；`FakeX` 提供简化可工作实现；`SpyX` 记录调用；`ScriptedX` 按预设队列执行；`MockX` 仅用于 mock framework 生成对象。
- 测试辅助 constructor、setter、状态读取器和 adapter **MUST** 受 `cfg(test)` 或批准的 test-only feature 约束，**NEVER** 因测试方便扩大生产 API。

### 确定性与 flaky 治理

- 时间、timer、TTL 与超时比较 **MUST** 使用可注入 Clock/VirtualClock；**NEVER** 用短 `sleep` 或毫秒级墙钟差证明状态重置。
- ID 和随机源 **MUST** 固定或注入；异步事件通过有上限的 `run_until` 或脚本队列推进。
- 文件测试 **MUST** 使用每测试唯一临时目录，**NEVER** 使用固定 `/tmp/aemeath_xxx`。
- 测试 **NEVER** 修改进程全局 cwd；共享环境变量测试必须串行隔离，并 **SHOULD** 优先改为配置注入。
- CI 首次失败后定向重跑只用于分类 flaky，**NEVER** 用重跑成功覆盖首次失败。flaky 测试必须修复确定性，或登记阻断 Issue、owner 与退役期限。

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
