# 日志规范

> 路径触发：`packages/global/logging/**` —— 日志基础设施（UnifiedLogger、14 字段 schema、target 路由）
> 场景触发：新增任何 `log::xxx!` 调用、修改日志字段或路由、新增日志文件

**Scope**：日志 target 命名、14 字段 JSON Lines schema、event_type 枚举、日志级别策略、preview/脱敏策略。
**不适用**：日志 crate 内部实现细节（rotation、context 全局变量）见 `packages/global/logging/src/` 源码。

## target 命名

- 统一前缀 `aemeath:`，三级结构 `aemeath:<layer>[:<feature>]`，最长到 `aemeath:agent:runtime`。
- 所有 `log::xxx!` 调用的 target 值 **MUST** ∈ 下面白名单，**NEVER** 使用旧前缀（如 `runtime::`、`cli::`、`tools::`）。
- 各 crate **MUST** 在 `lib.rs` 定义 `pub const LOG_TARGET: &str = "aemeath:<target>"`，调用方 **MUST** import 此常量，**NEVER** 硬编码 target 字符串。
- 守卫脚本 `.agents/hooks/check-log-target-prefix.sh` 强制检查。

### 12 个合法 target 白名单

| # | target | 说明 |
|---|--------|------|
| 1 | `aemeath:tui` | TUI / CLI 入口 |
| 2 | `aemeath:shared` | shared 层（横切基础设施） |
| 3 | `aemeath:composition` | composition 组合根 |
| 4 | `aemeath:agent:provider` | provider HTTP / stream 实现 + LLM 输入/输出 |
| 5 | `aemeath:agent:runtime` | agent 循环、tool 执行编排 |
| 6 | `aemeath:agent:tools` | Tool trait、ToolRegistry、MCP |
| 7 | `aemeath:agent:prompt` | Guidance 系统、系统提示 |
| 8 | `aemeath:agent:hook` | hook 执行 |
| 9 | `aemeath:agent:storage` | memory、task、history 持久化 |
| 10 | `aemeath:agent:project` | worktree 工作区上下文 |
| 11 | `aemeath:agent:policy` | 权限评估 |
| 12 | `aemeath:agent:audit` | 审计 |

## 文件映射表

UnifiedLogger 按 target 前缀最长匹配路由到日志文件（JSON Lines，一行一个 JSON 对象）：

| # | target | 日志文件名 | 来源 crate / 模块 | 记录什么 |
|---|--------|-----------|-------------------|---------|
| 1 | `aemeath:tui` | `tui.log` | `apps/cli` | TUI 渲染、输入处理、快捷键 |
| 2 | `aemeath:shared` | `shared.log` | `agent/shared` | 横切基础设施 |
| 3 | `aemeath:composition` | `composition.log` | `agent/composition` | 组合根装配 |
| 4 | `aemeath:agent:provider` | `agent-provider.log` | `agent/features/provider` | provider HTTP / stream + LLM 输入/输出（`llm_input`/`llm_output`/`user_input`） |
| 5 | `aemeath:agent:runtime` | `agent-runtime.log` | `agent/features/runtime` | agent 循环、compact、token budget、成本 |
| 6 | `aemeath:agent:tools` | `agent-tools.log` | `agent/features/tools` | tool 执行、MCP 通信 |
| 7 | `aemeath:agent:prompt` | `agent-prompt.log` | `agent/features/prompt` | Guidance 选择、系统提示构建 |
| 8 | `aemeath:agent:hook` | `agent-hook.log` | `agent/features/hook` | hook 执行、环境变量注入 |
| 9 | `aemeath:agent:storage` | `agent-storage.log` | `agent/features/storage` | 会话/记忆/历史落盘 |
| 10 | `aemeath:agent:project` | `agent-project.log` | `agent/features/project` | worktree 进入/退出/持久化 |
| 11 | `aemeath:agent:policy` | `agent-policy.log` | `agent/features/policy` | 权限评估决策 |
| 12 | `aemeath:agent:audit` | `agent-audit.log` | `agent/features/audit` | 审计事件 |
| — | 兜底 | `aemeath.log` | 不匹配任何白名单 target | 硬兜底 |
| — | `panic.log` | `panic.log` | panic_hook.rs 直写 | panic 信息（不纳入 UnifiedLogger） |

### 已废弃文件

以下文件已在日志系统重设计中废弃，**NEVER** 再使用：

| 废弃文件 | 废弃原因 | 替代 |
|---------|---------|------|
| `input.log` | LLM 输入合并到 `agent-provider.log`，用 `event_type="llm_input"` 区分 | `agent-provider.log` |
| `output.log` | LLM 输出合并到 `agent-provider.log`，用 `event_type="llm_output"` 区分 | `agent-provider.log` |
| `tool.log` | tool 日志已路由到 `agent-tools.log`（`aemeath:agent:tools`） | `agent-tools.log` |
| `audit.log` | 审计已路由到 `agent-audit.log`（`aemeath:agent:audit`） | `agent-audit.log` |

## 14 字段 schema

诊断日志（`format_diag_json_line`）和审计日志（`format_audit_json_line`）统一使用 14 个字段，compact JSON Lines 格式：

| # | 字段 | 类型 | 格式 | 来源 | 示例 |
|---|------|------|------|------|------|
| 1 | `ts` | string | 本地时间 RFC3339（毫秒精度，含时区偏移） | `timestamp_local_rfc3339()` | `"2026-06-17T14:30:00.123+08:00"` |
| 2 | `boot_ts` | string \| null | 本地时间 RFC3339 | `context::boot_ts()`（`init_logging` 时一次性设置） | `"2026-06-17T14:00:00.000+08:00"` |
| 3 | `ver` | string \| null | semver | `context::app_version()`（`init_logging` 时一次性设置） | `"0.8.2"` |
| 4 | `session` | string | UUID，未设置时 `"-"` | `context::session_id()` | `"a1b2c3d4-..."` |
| 5 | `chat` | string | UUID，未设置时 `"-"` | `context::current_chat_id()` | `"e5f6g7h8-..."` |
| 6 | `turn` | number \| null | usize | `context::current_turn()` | `5` |
| 7 | `request_id` | string \| null | UUID，每次 LLM 请求生成 | `context::current_request_id()` | `"i9j0k1l2-..."` |
| 8 | `model` | string | 模型 ID，未设置时 `"-"` | `context::current_model()` | `"claude-sonnet-4-20250514"` |
| 9 | `provider` | string \| null | provider 名称 | `context::current_provider()` | `"claude"` |
| 10 | `role` | string \| null | 消息角色 | `context::current_role()` | `"default"` / `"sub-agent-1"` |
| 11 | `level` | string | 日志级别 / `"AUDIT"` | `record.level()` | `"INFO"` |
| 12 | `target` | string | 日志 target（`aemeath:` 前缀） | `record.target()` | `"aemeath:agent:runtime"` |
| 13 | `event_type` | string \| null | 事件类型枚举（见下文） | 诊断行默认 `null`，审计行由调用方传入 | `"llm_input"` |
| 14 | `msg` | string \| null | 日志消息 / 审计行为 `null` | `record.args()` | `"compact triggered"` |

> 诊断行通过 `format_diag_json_line(record)` 生成；审计行通过 `format_audit_json_line(event_type, payload)` 生成，payload 字段会与上下文字段**平铺**到同一 JSON 对象。

## event_type 枚举

`event_type` 字段标识审计/生命周期事件类型，仅在审计行中由调用方显式设置。诊断行默认 `null`。

| event_type | 语义 | 关键字段 | 写入方式 |
|-----------|------|---------|---------|
| `llm_request_start` | LLM 请求发起 | `model`, `provider`, `request_id` | `log::debug!` with `event_type =` |
| `llm_input` | 发送给 LLM 的完整 prompt | `messages`, `tools`, `model` | `UnifiedLogger::log_input()` |
| `llm_chunk` | LLM stream 每个 chunk | `request_id`, chunk 文本 | `log::trace!` |
| `llm_output` | LLM 完整响应 | `stop_reason`, `input_tokens`, `output_tokens` | `UnifiedLogger::log_output()` |
| `llm_error` | LLM 请求错误 | `error`, `status_code` | `log::error!` |
| `tool_call` | tool 调用发起 | `tool_name`, `input` | `log::info!` |
| `tool_result` | tool 执行结果 | `tool_name`, `output`, `duration_ms` | `log::info!` |
| `turn_start` | 一轮对话开始 | `turn` | `log::info!` |
| `turn_end` | 一轮对话结束 | `turn`, `duration_ms` | `log::info!` |
| `compact` | 上下文压缩触发 | `reason`, `before_tokens`, `after_tokens` | `log::info!` |
| `user_input` | 用户输入 | `input_text`, `input_length` | `UnifiedLogger::log_user_input()` |

## 日志级别策略

| 级别 | 定位 | 记录什么 | 示例 |
|------|------|---------|------|
| **ERROR** | 致命错误 | 不可恢复的异常、panic 级别故障 | LLM 连接失败、文件写入失败、数据损坏 |
| **WARN** | 可恢复异常 | 降级处理、重试、fallback | API 超时后重试、配置缺失用默认值、权限不足 |
| **INFO** | 用户排障关键事件 | 影响用户可见行为的状态变迁 | 会话开始/结束、tool 执行、compact 触发、provider 切换 |
| **DEBUG** | 开发调试 | 内部状态、决策路径、**带 preview** 的内容摘要 | LLM 请求摘要（messages 数量 + preview）、token 统计、路由决策 |
| **TRACE** | chunk / token 级 | **完整原始内容**，仅在深度调试时开启 | LLM stream 每个 chunk、完整 messages JSON、raw HTTP response |

> **MUST NOT** 在 INFO 级记录大文本内容。DEBUG 级用 preview，TRACE 级可记录完整内容。

## preview / 脱敏策略

为防止日志膨胀（曾出现 85MB 日志），所有大文本内容 **MUST** 遵循以下策略：

| 级别 | 策略 | 内容限制 |
|------|------|---------|
| INFO | 仅结构化元数据 | 只记录 `model`、`messages.len()`、`tools_count`、`duration_ms` 等数字/枚举 |
| DEBUG | preview 摘要 | 前 200 字符 + 总长度（如 `role(user, 1500chars):Hello, I need help with...`） |
| TRACE | 完整内容 | 可记录完整 JSON、chunk 文本 |

### preview 函数

- `preview_messages(messages)` — 遍历 messages 列表，每条只记 `role` + content 前 100 字符 + 总长度
- 审计行（`log_input`/`log_output`）中的 `messages` 字段受 `role_logs_enabled` 配置控制，关闭时不记录

### request_id 生命周期

- **生成**：每次 LLM 请求前（`loop_run.rs` 中 `uuid::Uuid::new_v4()`）
- **贯穿**：从 `set_current_request_id()` 到该请求的最后一个 chunk 响应
- **清理**：请求结束后由下一个 `set_current_request_id()` 覆盖
- **用途**：将 `llm_request_start` → `llm_chunk` × N → `llm_output` / `llm_error` 串联为一次完整 LLM 调用
