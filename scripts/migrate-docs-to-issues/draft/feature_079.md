<!-- Migrated from: docs/feature/active.md#79 -->
### #79 日志模块整理与 hook 可观测性增强

**状态**：设计中（待用户确认）

**背景**：
1. `module_levels` 已从 `LoggingConfig` 中移除，但 `docs/feature/archived/040-claude-compatible-agents-config.md` 记录了该决策；代码与配置层面已无残留。
2. `aemeath.log` 当前通过 `env_logger::Builder` 接收全部 `log::*` 宏输出，所有模块的诊断日志混在一个文件中，TUI 渲染日志与 runtime 业务日志相互淹没，职责不清。
3. Hook 可观测性不足：runner.rs 中常规流水日志为 `info` 级别，在默认 `warn` 级别下不可见。
4. 日志格式为纯文本，不利于机器解析和后续分析。
5. 日志上下文仅含 `session` + `turn`，缺少 `chat_id` 和 `model`。

**设计决策**：

#### A. 统一日志入口：`UnifiedLogger`（路径 C）

**所有日志走一个 `log::Log` 入口 + 按 `record.target()` 前缀路由**。`JsonLogger` 与 `RoutingLogger` 合并为 `UnifiedLogger`，消除双管线：

| target 前缀 | 路由目标 | 写入格式 | 类别 |
|-------------|----------|----------|------|
| `cli::*` | `tui.log` | 诊断 JSON Lines（固定 8 字段） | 诊断 |
| `hook::*` | `hook.log` | 诊断 JSON Lines（固定 8 字段） | 诊断 |
| `audit::input` | `input.log` | 审计 JSON Lines（单行 `serde_json::Value`） | 结构化审计 |
| `audit::output` | `output.log` | 审计 JSON Lines（单行 `serde_json::Value`） | 结构化审计 |
| `audit::tool` | `tool.log` | 审计 JSON Lines（单行 `serde_json::Value`） | 结构化审计 |
| 其他 | `aemeath.log` | 诊断 JSON Lines（固定 8 字段） | 诊断 |
| panic | `panic.log` | 纯文本 + backtrace | 崩溃日志 |

> **统一为 JSON Lines**：诊断与审计均保证"**一行一个 JSON**"。诊断是固定 schema（`ts/session/chat/turn/model/level/target/msg`），审计是 `serde_json::Value::to_string()`（**compact，不带缩进**）单行序列化。`grep` / `jq` 可同时消费 `*.log`。

**`UnifiedLogger` 暴露两层 API**：

1. **`impl log::Log for UnifiedLogger`**（诊断入口）：接受 `&Record`，按 `record.target()` 前缀路由到对应文件 sink。调用方仍是 `log::info!(target: "cli::render", "msg")` 等宏，零侵入。
2. **结构化 sink 方法**（审计入口）：
   ```rust
   UnifiedLogger::log_input(json: serde_json::Value)
   UnifiedLogger::log_output(json: serde_json::Value)
   UnifiedLogger::log_tool(json: serde_json::Value)
   ```
   审计数据不经过 `log::*` 宏，直接以 `serde_json::Value` 形态写入对应审计文件，保留结构。

**`enabled()` 行为**：
- 诊断日志：`UnifiedLogger::enabled(record)` 委托 `env_logger::Logger::enabled()`，按 `RUST_LOG` + `config.level` 过滤
- 审计日志：`log_input/output/tool()` 内部先查 `role_logs_enabled && env_logger::enabled(...)`，确保审计开关与日志级别双控制

**为什么这是真正"统一"**：
- 调用方只面对一个 logger（`log::Log` trait）
- 审计与诊断共享 `enabled()` 过滤逻辑，但写入路径分 sink 保持各自数据形态
- 避免了路径 B（`log::info!(target: "audit::input", "{:#?}", json)`）的"序列化→字符串→反序列化"退化

#### B. 统一 JSON Lines 格式（诊断 + 审计）

所有日志文件均为 **JSON Lines**：每行一个完整 JSON 对象，行间无依赖。

**诊断日志**（`aemeath.log` / `tui.log` / `hook.log`）固定 schema（8 字段）：
```json
{"ts":"2026-06-11T14:30:00+08:00","session":"abc123","chat":"session-abc123-001","turn":3,"model":"deepseek/deepseek-chat","level":"INFO","target":"runtime::business::chat","msg":"chat started"}
```

字段：`ts` / `session` / `chat` / `turn` / `model` / `level` / `target` / `msg`

**审计日志**（`input.log` / `output.log` / `tool.log`）形态为**单行 `serde_json::Value`**：
```json
{"ts":"2026-06-11T14:30:00+08:00","session":"abc123","chat":"session-abc123-001","turn":3,"model":"...","messages":[{"role":"user","content":"..."}],"tools":[...]}
```

- 序列化：`serde_json::to_string(&value)`（**compact，不带缩进**），保证单行
- 字段：调用方提供的 `serde_json::Value` 顶层加 `ts/session/chat/turn/model` 上下文后整体序列化
- 嵌套结构可任意深度，consumer 用 `jq '.messages[0].content'` 即可下钻

**消费者一致性**：`grep -E '^\{' *.log | jq` 同时处理诊断与审计；`jq -c` 强制 compact 输出便于管道传递。

#### C. 全局日志上下文注入

| 变量 | 类型 | setter 位置 | 说明 |
|------|------|------------|------|
| `SESSION_ID` | `OnceLock<String>` | 会话启动 | 已有 |
| `CURRENT_CHAT_ID` | `RwLock<String>` | `loop_runner.rs` 每 chat 开始 | **新增**（chat 生命周期内变化） |
| `CURRENT_TURN` | `AtomicUsize` | 每 turn 开始 | 已有 |
| `CURRENT_MODEL` | `RwLock<String>` | `setup.rs` model 解析后 | **新增**（模型可能切换） |

#### D. Hook 日志降级

`hook/runner.rs` 中 5 处常规流水 `log::info!` → `log::debug!`：
- `hook match`
- `hook start`
- `hook env stdout/stderr`
- `hook end`

错误路径（spawn failed / wait failed / timeout / non-zero exit）保持 `warn`。

#### E. 职责边界规范

`aemeath.log` / `tui.log` / `hook.log` 禁止打印 messages / content blocks / tool results 等 LLM 交互数据；此类数据**必须**走 `UnifiedLogger::log_input/output/tool()` 审计 API，目标 target 为 `audit::input/output/tool`。

**技术实现**：
- 唯一 logger：`UnifiedLogger`（实现 `log::Log`），内部按 `record.target()` 前缀路由到 5 个文件 sink（aemeath/tui/hook/input/output/tool）。
- `UnifiedLogger::enabled()` 委托 `env_logger::Logger::enabled()`，保留 `RUST_LOG` + `config.level` 解析能力。
- 诊断 sink：每条 `Record` → `serde_json::to_string(&diag_record{ts, level, target, msg, ...})`（compact）写文件 → 一行。
- 审计 sink：通过静态方法（`UnifiedLogger::log_input(json)` 等）暴露，绕过宏直接写入：
  1. 包装 `{ts, session, chat, turn, model, ...payload}` 为 `serde_json::Value`
  2. `serde_json::to_string(&value)`（**compact**）→ 一行
- `panic.log` 维持 `panic_hook.rs` 现状，不纳入 `UnifiedLogger` 路由（panic 时 logger 自身可能不可用）。

**分歧记录（路径选择）**：
- 路径 A（双管线：RoutingLogger + JsonLogger 平行）：被否，调用点分裂、过滤逻辑重复。
- 路径 B（RoutingLogger 吞 JsonLogger，用 `log::info!(target: "audit::input", "{:#?}", json)`）：被否，`serde_json::Value` 退化为字符串、丧失结构化优势。
- **路径 C（JsonLogger 实现 `log::Log`，按 target 路由 + 审计 sink 静态方法）** = 选定：真正单入口、共享 `enabled()` 过滤、审计数据保留原始结构。

**改动文件**：
- `packages/global/logging/src/lib.rs` — 重新组织导出
- `packages/global/logging/src/multi_logger.rs` — **删除**（合并入 unified_logger）
- `packages/global/logging/src/json.rs` — **改造**为 unified_logger（实现 `log::Log` + 审计静态方法）
- `packages/global/logging/src/unified_logger.rs` — **新增**（或整合入 json.rs，由实现选择决定）
- `packages/global/logging/src/format.rs` — **新增**（诊断日志的 JSON 行格式化）
- `packages/global/logging/src/context.rs` — **新增**（SESSION_ID / CURRENT_CHAT_ID / CURRENT_TURN / CURRENT_MODEL 全局上下文）
- `agent/features/runtime/src/utils/bootstrap/logging_setup.rs` — 替换 `init_logging` 为 `UnifiedLogger::init(config)`
- `agent/features/runtime/src/business/chat/looping/loop_runner.rs` — `set_current_chat_id()` 调用
- `agent/features/runtime/src/business/agent/runner/setup.rs` — `set_current_model()` 调用
- `agent/features/hook/src/business/hook/runner.rs` — 5 处 `info` → `debug`
- `agent/features/runtime/src/utils/audit/**` — 调用点从 `JsonLogger::log_input(...)` 改为 `UnifiedLogger::log_input(...)`（如保留审计 API 命名）
- `specs/rust-coding.md` — 更新日志规范（统一入口、target 路由表、JSON 格式、职责边界）
- `docs/feature/active.md` — 本条目

**验证**：
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- 启动 TUI，确认 `~/.agents/logs/` 下同时生成 `aemeath.log` + `tui.log`（JSON Lines 格式）
- 触发 hook，确认 `hook.log` 生成且常规日志为 debug 级别
- 触发一次 LLM 调用 + 一次 tool call：
  - `input.log` / `output.log` / `tool.log` 写入**单行** JSON（每行 = 1 个完整 JSON 对象，无 pretty-print 缩进）
  - 用 `wc -l input.log` 行数 = LLM 调用次数；用 `jq '.' input.log` 无 parse error
  - 用 `jq '.messages | length' input.log` 可下钻到嵌套数据
- 确认 `RUST_LOG=hook::runner=debug` 能让 `hook.log` 输出流水日志
- 确认 `role_logs_enabled=false` 时审计文件不生成内容
- 单元测试：`UnifiedLogger::enabled()` 转发 `env_logger` 过滤；`UnifiedLogger::log_input()` 在 `role_logs_enabled=false` 时短路返回；审计 sink 输出不含换行符（保证 JSON Lines）
