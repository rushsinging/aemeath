# 日志规范

> 路径触发：`packages/global/logging/**` —— 日志基础设施（UnifiedLogger、14 字段 schema、target 路由）
> 场景触发：新增任何 `log::xxx!` 调用、修改日志字段或路由、新增日志文件

**Scope**：日志 target 命名、14 字段 JSON Lines schema、日志级别策略、preview/脱敏策略。
**不适用**：日志 crate 内部实现细节（rotation、context 全局变量）见 `packages/global/logging/src/` 源码。

## 3.4.1. target 命名

- 统一前缀 `aemeath:`，当前使用二级或三级结构，例如 `aemeath:context`、`aemeath:agent:runtime`。
- 所有 `log::xxx!` 调用的 target 值 **MUST** 来自 `packages/global/logging/src/domain/routing.rs` 的 TargetCatalog，**NEVER** 使用旧前缀（如 `runtime::`、`cli::`、`tools::`）。
- 具有独立运行时边界的 crate **MUST** 在 crate root（library 为 `lib.rs`，binary 为 `main.rs`）唯一定义 crate-private `LOG_TARGET`，并在真实入口与成功/失败/降级终态消费该 owner 常量；调用方 **NEVER** 硬编码 target 字符串或复制同值常量。
- 纯契约 `packages/sdk`、纯函数 `packages/global/utils`、Logging 实现自身与未接入 UnifiedLogger 的 `tools/xtask` **MUST NOT** 定义应用 target 或匿名常量保活；其边界分别由执行 owner、direct emergency diagnostics 或 CLI stdout/stderr 负责。
- 守卫脚本 `.agents/hooks/check-log-target-prefix.sh` 强制检查。

### 3.4.1.1. TargetCatalog

合法 target、owner、sink ID 和日志文件名只在 `packages/global/logging/src/domain/routing.rs` 定义一次。Catalog 覆盖具有独立运行时边界的 owner；Provider 另有专用的 LLM API Error target，Runtime 的 Prompt target保留为专用子能力路由。#941 已将生产调用迁到 owner 常量，并启用从 workspace members 反向校验 runtime owner 与非 runtime member 分类的全仓 Guard。

下表是 TargetCatalog 的非规范性可读投影；代码 catalog 是唯一真相，测试负责校验 target、sink ID 与文件名唯一。

## 3.4.2. 文件映射表

UnifiedLogger 按 target 前缀最长匹配路由到日志文件（JSON Lines，一行一个 JSON 对象）：

| # | target | 日志文件名 | 来源 crate / 模块 | 记录什么 |
|---|--------|-----------|-------------------|---------|
| 1 | `aemeath:tui` | `tui.log` | `apps/cli` | TUI 渲染、输入处理、快捷键 |
| 2 | `aemeath:shared` | `shared.log` | `agent/shared` | 横切基础设施 |
| 3 | `aemeath:composition` | `composition.log` | `agent/composition` | 组合根装配 |
| 4 | `aemeath:agent:provider` | `agent-provider.log` | `agent/features/provider` | provider HTTP / stream + LLM 输入/输出（`llm_input`/`llm_output`/`user_input`，通过 `log::info!` 以 JSON 字符串作为 msg 传入） |
| 5 | `aemeath:agent:runtime` | `agent-runtime.log` | `agent/features/runtime` | agent 循环、compact、token budget、成本 |
| 6 | `aemeath:agent:tools` | `agent-tools.log` | `agent/features/tools` | tool 执行、MCP 通信 |
| 7 | `aemeath:agent:prompt` | `agent-prompt.log` | `agent/features/prompt` | Guidance 选择、系统提示构建 |
| 8 | `aemeath:agent:hook` | `agent-hook.log` | `agent/features/hook` | hook 执行、环境变量注入 |
| 9 | `aemeath:agent:storage` | `agent-storage.log` | `agent/features/storage` | 会话/记忆/历史落盘 |
| 10 | `aemeath:agent:project` | `agent-project.log` | `agent/features/project` | worktree 进入/退出/持久化 |
| 11 | `aemeath:agent:policy` | `agent-policy.log` | `agent/features/policy` | 权限评估决策 |
| 12 | `aemeath:diagnostic:audit` | `audit-diagnostic.log` | `agent/features/audit` | Audit 模块自身的 queue/write/drain 运行诊断；**NEVER** 承载 Audit Fact |
| 13 | `aemeath:agent:update` | `agent-update.log` | `agent/features/update` | 更新检查与安装诊断 |
| 14 | `aemeath:agent:workflow` | `agent-workflow.log` | `agent/features/workflow` | Reasoning Graph / effort 诊断 |
| 15 | `aemeath:context` | `context.log` | `agent/features/context` | Context Management 诊断 |
| 16 | `aemeath:agent:config` | `agent-config.log` | `agent/features/config` | 配置诊断 |
| 17 | `aemeath:agent:memory` | `agent-memory.log` | `agent/features/memory` | Memory 诊断 |
| 18 | `aemeath:agent:task` | `agent-task.log` | `agent/features/task` | Task 诊断 |
| 19 | `aemeath:llm-api-error` | `llm-api-error.log` | `agent/features/provider` | 脱敏后的 LLM API 失败诊断 |
| — | 兜底 | `aemeath.log` | 未注册 target | 硬兜底，写入 `aemeath.log`（**NEVER** 写 stderr） |
| — | emergency | `emergency.log` | logging 自身 | File 模式下 sink degrade / fallback 的兜底输出（**NEVER** 写 stderr，避免污染 TUI alternate screen，见 #1215） |
| — | `panic.log` | `panic.log` | panic_hook.rs 直写 | panic 信息（不纳入 UnifiedLogger） |

### 3.4.2.1. 已废弃文件

以下文件已在日志系统重设计中废弃，**NEVER** 再使用：

| 废弃文件 | 废弃原因 | 替代 |
|---------|---------|------|
| `input.log` | LLM 输入合并到 `agent-provider.log`，用 `event_type="llm_input"` 区分 | `agent-provider.log` |
| `output.log` | LLM 输出合并到 `agent-provider.log`，用 `event_type="llm_output"` 区分 | `agent-provider.log` |
| `tool.log` | tool 日志已路由到 `agent-tools.log`（`aemeath:agent:tools`） | `agent-tools.log` |
| `audit.log` / `agent-audit.log` | Audit Fact 不能伪装成 DiagnosticRecord | Audit 自有 Usage append store；模块运行诊断使用 `audit-diagnostic.log` |

## 3.4.3. 14 字段 schema

所有 DiagnosticRecord 使用 14 个字段和 compact JSON Lines 格式，统一走 `log::log!` → `UnifiedLogger::log()` → `format_diag_json_line`。Audit Event / Usage Fact 使用 Audit 自有 PL 与 append store，**NEVER** 进入该管线。Audit 模块自身的运行故障是普通诊断，使用 `aemeath:diagnostic:audit`。

| # | 字段 | 类型 | 格式 | 来源 | 示例 |
|---|------|------|------|------|------|
| 1 | `ts` | string | 本地时间 RFC3339（毫秒精度，含时区偏移） | `timestamp_local_rfc3339()` | `"2026-06-17T14:30:00.123+08:00"` |
| 2 | `boot_ts` | string \| null | 本地时间 RFC3339 | `context::boot_ts()`（`init_logging` 时一次性设置） | `"2026-06-17T14:00:00.000+08:00"` |
| 3 | `pid` | number | 进程 pid | `context::pid()`（`std::process::id()` 惰性初始化） | `73576` |
| 4 | `ver` | string \| null | semver | `context::app_version()`（`init_logging` 时一次性设置） | `"0.8.2"` |
| 5 | `session` | string | UUID，未设置时 `"-"` | scope-local `LogContext.session_id` | `"a1b2c3d4-..."` |
| 6 | `chat` | string | UUID，未设置时 `"-"` | scope-local `LogContext.chat_id` | `"e5f6g7h8-..."` |
| 7 | `turn` | number \| null | usize | scope-local `LogContext.turn` | `5` |
| 8 | `request_id` | string \| null | UUID，每次 LLM 请求生成 | scope-local `LogContext.request_id` | `"i9j0k1l2-..."` |
| 9 | `model` | string | 模型 ID，未设置时 `"-"` | scope-local `LogContext.model` | `"claude-sonnet-4-20250514"` |
| 10 | `provider` | string \| null | provider 名称 | scope-local `LogContext.provider` | `"claude"` |
| 11 | `role` | string \| null | 消息角色 | scope-local `LogContext.role` | `"default"` / `"sub-agent-1"` |
| 12 | `level` | string | 日志级别 | `record.level()` | `"INFO"` |
| 13 | `target` | string | 日志 target（`aemeath:` 前缀） | `record.target()` | `"aemeath:agent:runtime"` |
| 14 | `msg` | string \| null | 日志消息（诊断行为自由文本，或结构化 JSON payload） | `record.args()` | `"compact triggered"` |

> 部分日志的 `msg` 字段包含序列化后的 JSON payload（诊断行为自由文本，或 LLM I/O 等结构化内容）。
> 消费者可用 `jq 'select(.msg | startswith("{")) | .msg | fromjson' *.log` 解析。

## 3.4.4. 日志级别策略（Issue #338）

### 3.4.4.1. 总则

| Level | 语义 | 频率约束 |
|---|---|---|
| `error` | 已发生且需要用户/开发者关注的**不可恢复**失败 | 正常控制流 **NEVER** 出现 |
| `warn`  | 可恢复但值得关注的异常或降级 | 每次失败 **MAY** 出现，但不应爆炸式重复 |
| `info`  | 用户/运维关心的**关键生命周期事件** | 低频，**NEVER** 含完整 JSON payload 或 per-chunk/per-turn 高频日志 |
| `debug` | 开发调试需要的**中等粒度**细节 | 中频，可含安全截断后的 preview |
| `trace` | 高频、细粒度、默认关闭的诊断 | 无频率上限，但 **NEVER** 泄露未截断的敏感完整内容 |

### 3.4.4.2. `error`

**NEVER** 在正常控制流中频繁出现。用于：

- 操作最终失败且无法自动恢复；
- 数据持久化失败（如 tool_result 写盘失败）；
- provider 请求失败且已返回给用户（最终失败，非中间重试）；
- hook / tool / runtime 关键流程失败（如 task panic）。

### 3.4.4.3. `warn`

可恢复但值得关注的异常或降级。用于：

- 配置项无效并回退默认值；
- 网络失败但不影响主流程继续（如 MCP 重连失败、hook spawn 失败）；
- 兼容旧格式或迁移降级；
- tool/provider 返回异常结构但已容错（如孤儿 tool message 丢弃、流解析容错）；
- 潜在数据不一致风险。

**重试场景降噪**：重试循环中的中间失败 **SHOULD** 用 `debug`，仅末次汇总用 `warn`。

### 3.4.4.4. `info`

用户或运维可能关心的关键生命周期事件。用于：

- session 启动/结束；
- provider/model 选择（一次性）；
- MCP 服务器连接初始化结果；
- tool 调用**摘要**（不含完整 I/O）；
- compact / cost / worktree 等关键状态变化；
- guidance 重新加载策略决策。

**NEVER** 包含：
- 完整 LLM 请求/响应 JSON payload（应为 `debug`）；
- per-chunk / per-turn / per-hook 事件级日志（应为 `debug` 或 `trace`）。

### 3.4.4.5. `debug`

开发调试需要的中等粒度细节。用于：

- 请求参数/响应摘要；
- tool input/output 摘要；
- runtime 状态机转换；
- 配置解析细节；
- 完整 JSON payload（当确实需要落盘时，如 LLM I/O）；
- 重试循环中的中间失败；
- 可包含安全截断后的 preview（**SHOULD** 用 `truncate_preview` 或等价机制截断 body）。

### 3.4.4.6. `trace`

高频、细粒度、默认关闭的诊断。用于：

- SSE chunk 级事件（raw 行、delta 长度）；
- reasoning / content delta 长度统计；
- UI 布局细节、帧级绘制；
- token / stream 累计状态；
- per-chunk 的状态机推进（如 `ToolCallUpdate` 的 `arguments_delta`）。

**NEVER** 泄露未截断的敏感完整内容（如完整用户输入、完整 API key、完整 LLM body）。

### 3.4.4.7. Per-Layer 细则

#### 3.4.4.7.1. provider 层（`agent/features/provider/**`）

| 场景 | Level | 说明 |
|---|---|---|
| provider client 创建、model 选定 | `info` | 一次性生命周期 |
| `send_message` / `stream_message` 入口 | `debug` 或不加 | 非 lifecycle，是 per-turn |
| SSE chunk raw 行 | `trace` | 高频 |
| reasoning / content delta 长度统计（per-chunk） | `trace` | 高频累计 |
| `tool_use_start` / first delta（一次性） | `debug` | 每 turn 一次 |
| 请求/响应摘要（已截断） | `debug` | 中等粒度 |
| 5xx / 非 2xx body | `debug`（**SHOULD** 截断） | 排障用 |
| 重试中间失败 | `debug` | 仅末次失败用 `warn` |
| 流截断（`StreamTruncated`）返回 | `warn`（已容错）或 `error`（最终失败） | 视是否终止主流程 |
| 孤儿 tool message 丢弃 | `warn` | 数据一致性 |

#### 3.4.4.7.2. runtime 层（`agent/features/runtime/**`）

| 场景 | Level | 说明 |
|---|---|---|
| session 启动/结束、turn 起止摘要 | `info` | 低频生命周期 |
| provider/model 选择、skills 加载、MCP 连接结果 | `info` | 一次性 |
| compact 触发、cost 清零、worktree 状态变化 | `info` | 状态变化 |
| **完整 LLM I/O JSON（messages / content_blocks / tool_schemas）** | `debug` | **NEVER info**，payload 过大 |
| 每 turn 的 tool_call 独立日志 | `debug` | per-turn 中频 |
| sub-agent 每 turn 进度回调 | `debug` | per-turn |
| 每 turn 边界的 config_reload 检测 | `debug` | per-turn |
| 每个 hook 事件分发 | `debug` | per-event |
| tool 执行取消（正常控制流） | `debug` | 非异常 |
| tool 执行完成摘要 | `debug` | 中等粒度 |
| 状态机非法转换、持久化失败、PreCompact 阻止 | `warn` | 可恢复降级 |
| 数据持久化失败且影响后续正确性 | `warn` 或 `error` | 视是否终止 |
| runtime→sdk 事件转换（per-stream-event） | `trace` | 高频 |

#### 3.4.4.7.3. tui 层（`apps/cli/src/tui/**` + `apps/cli/src/chat/**`）

| 场景 | Level | 说明 |
|---|---|---|
| chat frontend 启动、session 启动/结束 | `info` | 生命周期 |
| 更新检测完成、guidance 初始化完成 | `info` | 生命周期 |
| streaming delta（`ToolCallUpdate.arguments_delta`） | `trace` | per-chunk 高频 |
| `tool_call_start` / `tool_result`（一次性） | `debug` | 每 turn 有限次 |
| tool I/O 摘要、状态机转换 | `debug` | 中等粒度 |
| TUI 主循环帧级、事件级 | `trace` | 高频 |
| panic 兜底（task panic、事件循环 panic） | `error` | 关键失败 |
| auto-save 失败（可能丢失用户会话） | `warn` 或 `error` | 视严重性 |
| 排队消息丢弃图片（设计预期） | `debug` | 非异常 |

#### 3.4.4.7.4. hook / tools / storage / prompt / update 层

| 场景 | Level |
|---|---|
| hook spawn/write/exit/wait/timeout 失败（返回 HookResult error） | `warn` |
| hook match/start/end/env/result 细节 | `debug` |
| bash 命令执行/完成摘要、被信号终止 | `debug` / `warn` |
| MCP 重连失败、tools/list 重试失败 | `warn` |
| MCP SSE 协议级细节、stderr 转发 | `debug` |
| tool_result **写盘失败**（数据持久化失败） | `error` |
| tool_result 路径遍历拒绝、目录创建失败 | `warn` |
| guidance 目录/文件创建失败、skill 解析失败 | `warn` |
| 更新检测命中、下载完成、校验通过 | `info` |
| 更新缓存写入/序列化失败（次要持久化） | `warn` |

### 3.4.4.8. 强制约束（MUST / NEVER / SHOULD）

- **MUST** 选择 level 时对照本节 Per-Layer 细则表。
- **NEVER** 在 `info!` 中输出完整 LLM I/O JSON payload——用 `debug!`。
- **NEVER** 在 `info!` / `debug!` 中输出 per-chunk 高频事件——用 `trace!`。
- **NEVER** 在 `debug!` 中输出未截断的完整敏感 body——**SHOULD** 用 `truncate_preview` 截断。
- **SHOULD** 重试循环中的中间失败用 `debug!`，末次汇总用 `warn!`。
- **SHOULD** 数据持久化失败视影响升 `error!`。
- **MUST** 新增日志调用时在 PR 描述中标注所遵循的 level 规则条目。

### 3.4.4.9. 待设计点决策（#338 遗留）

| 待设计点 | 决策 | 理由 |
|---|---|---|
| 按模块定义 target 命名规范 | **沿用现状**——`aemeath:<domain>` | 现有规范已满足，见上方 target 命名章节 |
| 统一要求生产代码日志带 `target:` | **MUST**（已落地，守卫强制） | `rust-coding.md` + `domain/routing_guard.rs` 已实现 |
| 从 `log` 迁移到结构化 `tracing` | **推迟**，单开 issue 追踪（#346） | 当前无 trace 可视化后端消费 span 因果链，迁移成本高且与 level 规范正交 |
| 统一 logging helper / macro | **沿用现状**——`LOG_TARGET` 常量 + TUI `log_xxx!` 宏 | 已满足，无需新增 |
| 建立 lint/test 防止错误 level 扩散 | **文档 + code review 为主**；`domain/routing_guard.rs` 仅强制 target 合规 | level 选择存在语义判断，机器化检测收益有限；新增日志的 level 由 PR review 把关 |
| provider/tool/runtime/TUI 补充细则 | **已在本节 Per-Layer 细则落地** | 见上 |

## 3.4.5. preview / 脱敏策略

为防止日志膨胀（曾出现 85MB 日志），所有大文本内容 **MUST** 遵循以下策略：

| 级别 | 策略 | 内容限制 |
|------|------|---------|
| INFO | 仅结构化元数据 | 只记录 `model`、`messages.len()`、`tools_count`、`duration_ms` 等数字/枚举 |
| DEBUG | preview 摘要 | 前 200 字符 + 总长度（如 `role(user, 1500chars):Hello, I need help with...`） |
| TRACE | 完整内容 | 可记录完整 JSON、chunk 文本 |

### 3.4.5.1. preview 函数

- `preview_messages(messages)` — 遍历 messages 列表，每条只记 `role` + content 前 100 字符 + 总长度
- LLM I/O 日志的 `msg` 字段包含完整 JSON payload，由调用方通过 `log::debug!` 传入（见下方级别策略）

### 3.4.5.2. request_id 生命周期

- **生成**：每次 LLM 请求前（`loop_run.rs` 中 `uuid::Uuid::new_v4()`）
- **贯穿**：每次物理请求创建不可变 child `LogContext`，该 scope 覆盖请求开始到最后一个 chunk / 终态响应
- **清理**：request child scope 结束后自动恢复父级 turn context；retry 创建新的 request ID 与新 child scope
- **用途**：将 `llm_request_start` → `llm_chunk` × N → `llm_output` / `llm_error` 串联为一次完整 LLM 调用
