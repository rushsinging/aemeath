# 活动中 Feature

| # | 标题 | 优先级 | 状态 | 确认结果 | 目标 |
|---|------|--------|------|----------|------|
| 1 | Hook 功能 | - | 实施中 | 未确认 | 参考 Claude Code hook 系统，在关键生命周期点执行用户自定义 shell 命令 |
| 2 | SubAgent 可配置 | - | 部分完成 | 未确认 | 支持通过配置文件定义 agent role（绑定 model、description、system_suffix），Agent tool 通过 `role`/`model` 参数路由到不同 LLM |
| 3 | CLI 子命令 | - | ✅ 已完成 | 未确认 | 支持 `aemeath models`、`aemeath sessions` 等子命令 |
| 4 | AskUserQuestion TUI 美化 | - | 待实施 | 未确认 | AskUserQuestion 向用户确认时，TUI 界面需要美化 |
| 5 | Agent 调用显示优化 | - | ✅ 已完成 | 未确认 | 优化 Agent tool call 的 TUI 显示：调用阶段展示 role/model/description，执行过程展示子 agent 关键进度，结果展示区分 agent 输出与普通 tool 输出 |
| 6 | Task 调用显示优化 | - | ✅ 已完成 | 未确认 | 优化 Task 系列 tool call 的 TUI 显示：TaskCreate/TaskUpdate 脱离静默模式展示关键信息，TaskList 结果格式化为可读表格，Task 生命周期状态变更可视化 |
| 7 | Input Queue 优化 | - | ✅ 已完成 | 未确认 | 将单条 queued_input 改为多消息队列（VecDeque），支持处理期间连续排队多条输入 |
| 8 | Memory 系统 | - | 待实施 | 未确认 | 增加 memory 系统，支持跨会话持久化记忆，在合适时机自动写入/检索上下文 |
| 9 | 反思系统 | - | 待实施 | 未确认 | 在关键节点对过去行为/决策做反思总结，提炼经验写入 Memory 系统（依赖 #8） |
| 10 | 日志文件规范化 | - | 待确认 | 未确认 | 规范 aemeath.log / panic.log / agent.log 的职责边界、格式、轮转策略 |
| 11 | OpenAI reasoning_effort 配置支持 | - | 待实施 | 未确认 | 支持 GPT-5.x 系列 `reasoning_effort` (none/low/medium/high/xhigh)，可在 config.json 配置 |
| 12 | Input Queue 双层循环优化 | - | 待实施 | 未确认 | 双层循环让 LLM 不必完成整轮对话即可读取 input queue；在 tool call 完成后可读取用户新输入并补充给 LLM，实现快速响应用户反馈 |
| 13 | Task list 显示在 spinner 下方 | - | 待实施 | 未确认 | 临时区域渲染顺序调整为：queued messages → spinner → task status lines，让 task list 出现在 spinner 下方而非上方 |
| 14 | Session ID 自增无冲突方案 | - | 待实施 | 未确认 | 当前 `{timestamp_ms}{rand_u32}` 24 位 hex 不可读、不自增；探索"按时间单调自增 + 全局无冲突"的方案，兼顾可读性、排序、迁移兼容 |

##  #3 CLI 子命令

**目标**：将扁平的 `--flag` 式 CLI 重构为子命令架构，支持 `aemeath models`、`aemeath sessions` 等独立子命令，同时保持无子命令时默认启动 TUI 的兼容行为。

**设计**：
- `cli.rs` 重构为 `Cli`（顶层）+ `Commands` enum（`Run`、`Models`、`Sessions`），使用 clap `Subcommand` derive
- `Args` 保留为内部结构体，`from_run()` 构造，避免大范围修改 `run_chat` 内部逻辑
- `main()` 根据子命令分发：`Models`/`Sessions` 独立处理后 return，`Run`/None 走原有聊天逻辑

**已实现子命令**：
- `aemeath models` — 列出 config.json 中所有可用模型（表格 / `--json`）
- `aemeath sessions` — 列出已保存会话（`--limit N`、`--json`、`--delete <ID>`）
- `aemeath run [OPTIONS]` — 显式启动聊天（所有原有 flag）
- `aemeath`（无子命令）— 兼容旧行为，默认启动 TUI

**涉及路径**：`aemeath-cli/src/cli.rs`、`aemeath-cli/src/main.rs`

---

### #4 AskUserQuestion TUI 美化

**目标**：当 LLM 调用 AskUserQuestion tool call 时，TUI 中的确认界面需要美化，提升可读性和交互体验。

**当前状态**：基础功能已实现（`UiEvent::AskUser` + `update.rs` 中 `ask_user_reply_tx` 机制），但显示为普通 system message + 纯文本选项，缺乏视觉层次。

**待改进**：
- 问题文本高亮/醒目样式
- 选项列表带序号和视觉区分
- 输入提示区域样式优化

**涉及路径**：`aemeath-cli/src/tui/app/update.rs`（`UiEvent::AskUser` 处理）、`aemeath-cli/src/tui/output_area/`（渲染样式）

---

### #1 Hook 功能（参考 Claude Code 设计）

**已实现（v1）**：
- 4 个生命周期事件（PreToolUse / PostToolUse / Stop / UserPrompt）
- Hook 执行引擎：通过 stdin 传入 JSON，通过 exit code 控制行为（exit 0=成功, exit 2=阻止）
- 配置合并

**本次实施（v2 — 完整设计）**：

#### 事件类型

参考 Claude Code 官方文档（https://code.claude.com/docs/en/hooks）的事件清单，按优先级分批落地。

##### 已实施 / 计划首批（P0–P1）

| 事件 | 优先级 | 说明 |
|------|--------|------|
| PreToolUse | P0（已有） | 工具执行前阻止/修改 |
| PostToolUse | P0（已有，需修复 output 生效） | 工具执行后注入上下文 |
| PostToolUseFailure | P0（新增） | 工具失败后注入修复指导 |
| UserPromptSubmit | P0（重命名 UserPrompt，需实现调用） | 用户输入检查/修改/拒绝 |
| Stop | P0（已有） | Agent 停止前质量门 |
| StopFailure | P1（新增） | API 错误，观察性 |
| SessionStart | P1（新增） | 会话开始，注入上下文 |
| PreCompact | P1（新增，占位） | 上下文压缩前 |
| PostToolBatch | P1（新增，占位） | 批量工具后汇总 |

##### 待补充（与官方文档对齐）

| 事件 | 优先级 | 说明 | 备注 |
|------|--------|------|------|
| SessionEnd | P1 | 会话结束清理（写日志/反思入口） | 与 SessionStart 配对 |
| PostCompact | P1 | 上下文压缩完成后 | 与 PreCompact 配对 |
| SubagentStart | P1 | 子代理启动 | 配合 Feature #2 SubAgent |
| SubagentStop | P1 | 子代理完成 | 配合 Feature #2 SubAgent |
| TaskCreated | P1 | 通过 TaskCreate 创建任务时 | 配合 Feature #6 任务系统 |
| TaskCompleted | P1 | 任务标记完成时 | 反思系统（#9）的天然触发点 |
| PermissionRequest | P2 | 权限对话弹出 | 用于审计 / 自动批准策略 |
| PermissionDenied | P2 | 自动模式拒绝 | 用于审计 / 提示用户 |
| Notification | P2 | Claude 发送通知时 | TUI 通知钩子 |
| InstructionsLoaded | P2 | CLAUDE.md / guidance 加载到上下文 | 用于注入额外规则 |
| ConfigChange | P2 | 会话中配置文件变更 | 配合 hot reload |
| Elicitation | P2 | MCP 服务器请求用户输入前 | 依赖 MCP 体系完善度 |
| ElicitationResult | P2 | 用户响应 MCP elicitation 后 | 同上 |
| UserPromptExpansion | P3 | 用户输入展开为提示时（如 slash 命令） | 官方文档已列，cli.js v2.1.88 尚未落地，跟进观察 |
| CwdChanged | P3 | 工作目录改变 | 价值有限，按需 |
| FileChanged | P3 | 监视文件在磁盘改变 | 需 file watcher 基础设施 |
| TeammateIdle | P3 | 团队队友空闲 | aemeath 暂无 agent team 特性 |
| WorktreeCreate | — | git worktree 创建 | aemeath 不支持 worktree，**不实施** |
| WorktreeRemove | — | git worktree 移除 | 同上，**不实施** |

#### JSON 输出协议（v2 核心改进）

exit 0 + stdout JSON 支持以下字段：
- `continue: false` + `stopReason` — 全局停止
- `decision: "block"` + `reason` — 阻止操作（PostToolUse / UserPromptSubmit / Stop 等）
- `additionalContext` — 注入额外上下文
- `systemMessage` — 系统警告
- `hookSpecificOutput` — PreToolUse 特定控制（allow/deny/ask + updatedInput）

exit 2 = 阻止操作，stderr 反馈给 LLM
exit 其他 = 非阻塞错误

#### 设计原则
- 阻止操作时，反馈消息传给 LLM 让其继续调整，不中断用户交互
- 所有注入上下文在 LLM 对话流中可见
- 不新增 UI 事件类型

**涉及路径**：
- `aemeath-core/src/hook.rs` — 数据结构 + JSON 输出解析
- `aemeath-core/src/config/hooks.rs` — 事件枚举 + 配置
- `aemeath-cli/src/tui/app/input_handler.rs` — UserPromptSubmit 调用
- `aemeath-cli/src/tui/app/stream.rs` — PostToolUse / PostToolUseFailure / PostToolBatch / PreCompact 调用
- `aemeath-cli/src/tui/app/update.rs` — StopFailure 调用
- `aemeath-cli/src/main.rs` — SessionStart 调用

---

### #5 Agent 调用显示优化

**目标**：优化 Agent tool call 在 TUI 中的显示体验，让用户能够看到 agent 的 role/model/description、关键进度和结果。

**当前状态**：
- `push_tool_call_start` 只显示 `● Agent...`，无具体信息
- `push_tool_call` 显示 `● Agent(desc)` + 详情行（prompt 截断 300 字符，role/model 在 detail 行中）
- 子 agent 执行期间只显示 spinner，无进度反馈
- 结果展示与普通 tool 输出无区分

**待改进**：
1. **调用阶段**（`push_tool_call_start` / `push_tool_call`）：
 - header 行展示 `● Agent(desc)` + `role: xxx` / `model: xxx` 信息
 - prompt 预览作为 detail 行显示
2. **执行阶段**：
 - 当前子 agent 无 TUI 进度反馈（日志写入 agent.log 文件），考虑将关键进度（如 turn X/N、工具调用名）通过 UI 事件推送到 TUI
 - 保持轻量：只推送工具调用名称变更（如 `Agent → Read(foo.rs)`），而非逐行文本
3. **结果展示**（`push_tool_result_with_diff`）：
 - Agent 结果输出通常较长，当前截断为 3 行不够友好
 - Agent 输出应获得更多显示行数（当前固定 3 行），或折叠/展开机制

**涉及路径**：
- `aemeath-cli/src/tui/output_area/tool_display.rs` — `format_tool_call` + `push_tool_result_with_diff` 中 Agent 特殊处理
- `aemeath-cli/src/tui/app/stream.rs` — 子 agent 执行期间的进度事件推送
- `aemeath-cli/src/agent_runner.rs` — `run_agent` 中进度回调扩展（当前用 `progress` 闭包写日志，需改为发送 UI 事件）
- `aemeath-cli/src/tui/app/update.rs` — 新增 UI 事件类型（如 `UiEvent::AgentProgress`）

---

### #6 Task 调用显示优化

**目标**：优化 Task 系列 tool call（TaskCreate、TaskUpdate、TaskList、TaskGet、TaskStop、TaskOutput）在 TUI 中的显示体验，让用户清晰感知任务生命周期变化。

**当前状态**：
- `TaskCreate`/`TaskUpdate` 完全静默（`skip_ui = true`），不发送任何 `UiEvent::ToolCall`/`ToolResult`，用户看不到任务的创建和状态变更
- `TaskList`/`TaskGet`/`TaskStop`/`TaskOutput` 显示为普通 tool call，无特殊样式
- `TaskList` 结果获得 20 行显示上限，但输出为纯文本列表，无表格格式化
- 任务状态栏（status bar）已有基本汇总（`✓`/`■`/`□` + subject），但与 tool call 区域的信息割裂

**待改进**：

#### 1. TaskCreate 显示
- 脱离 `skip_ui`，改为发送 `UiEvent::ToolCall`
- header 行：`● TaskCreate: {subject}` + priority 标记
- detail 行：description 截断预览（~80 字符）
- 结果行：显示 task ID + 状态 `pending`

#### 2. TaskUpdate 显示
- 脱离 `skip_ui`，改为发送 `UiEvent::ToolCall`
- header 行：`● TaskUpdate: {task_id_prefix}` + 状态变更箭头（如 `pending → in_progress`）
- detail 行：subject 变更（若有）、priority 变更（若有）、依赖信息（blockedBy/blocks）
- 结果行：显示 unblocked tasks 列表（当前已由 tool 返回，需在 TUI 中格式化展示）

#### 3. TaskList 结果格式化
- `format_tool_call` 中为 `TaskList` 提供专用的 header 显示
- `push_tool_result_with_diff` 中解析 TaskList 的 JSON 返回，格式化为带状态图标的紧凑表格：
  ```
  ✓ #1 Hook 功能        [completed]
  ■ #5 Agent 调用显示优化 [in_progress]
  □ #6 Task 调用显示优化  [pending]
  ```

#### 4. Task 生命周期状态变更可视化
- TaskUpdate 将 task 置为 `completed` 时，结果行使用成功样式（绿色 ✓）
- TaskUpdate 将 task 置为 `in_progress` 时，显示 spinner 动效（与 Agent tool 一致）
- TaskStop 显示为警告样式（黄色）

**涉及路径**：
- `aemeath-cli/src/tui/output_area/tool_display.rs` — `format_tool_call` + `push_tool_result_with_diff` 中 Task 系列工具特殊处理
- `aemeath-cli/src/tui/app/stream.rs` — `is_task_tool` 判断逻辑改为发送 UI 事件而非跳过

---

### #12 Input Queue 双层循环优化

**目标**：让 LLM 不必等完整一轮 assistant 响应结束后才处理用户排队输入。尤其是在 tool call 完成后，如果用户已经在 input queue 中补充了新要求，应立即把这些输入追加到下一次 LLM 调用中，让模型基于最新反馈继续执行。

**新增要求**：
- tool call 执行完成后，检查 input queue 是否已有用户新输入。
- 若有，将这些输入作为新的 user message 或明确的补充上下文注入给 LLM。
- 注入内容需要保留用户原文和顺序，避免丢失多条排队输入。
- 这一步发生在下一次 LLM API 调用前，而不是等当前整轮对话完全结束。

**预期效果**：
1. 用户在 tool 执行期间输入“等等，改成只查 src 目录”。
2. tool 完成后，系统立即读取该输入。
3. 下一次 LLM 调用能看到这条补充，并调整后续行为。

**涉及路径**：
- `aemeath-cli/src/tui/app/stream.rs` — tool result 后、下一轮 LLM 调用前读取 input queue
- `aemeath-cli/src/tui/app/input_handler.rs` — processing 状态下用户输入入队语义
- `aemeath-cli/src/tui/app/mod.rs` — `input_queue` 状态管理

---

### #9 反思系统

**目标**：在关键节点（任务完成、Stop、错误恢复后、用户显式触发）执行反思流程，对最近的行为、决策、失败、用户反馈做结构化总结，将有价值的经验写入 Memory 系统（#8），让 agent 在未来会话中能够基于历史经验做更好的决策。

**依赖**：Feature #8 Memory 系统（反思的输出目标）

**设计草案**：

#### 触发时机
- **任务完成后**：TaskUpdate 将 task 置为 `completed` 时，对该 task 的执行过程做总结
- **Stop 事件**：会话结束 / agent 主动停止时，对整段会话做反思
- **错误恢复后**：tool call 失败 → 修复 → 成功 的链路上，提炼"哪种修复有效"
- **用户显式触发**：`/reflect` slash 命令，对最近 N 轮做即时反思
- **PostCompact 钩子**：上下文压缩前抢救关键经验

#### 反思维度
- **成功模式**：哪些工具组合 / 推理路径达成了目标
- **失败教训**：哪些假设错了、哪些 tool call 走了弯路
- **用户偏好**：用户在本次会话中的纠正、拒绝、确认（参考 superpowers `feedback` 类型）
- **未解决问题**：本次会话中悬而未决的事项（提示下次继续）

#### 输出格式
- 结构化条目（type / title / body / scope），写入 Memory 系统
- 每条反思 must 标注来源会话 ID + 时间戳，便于追溯
- 避免重复：写入前检索 Memory，相似条目优先 update 而非 insert

#### 实施阶段
1. **Phase 1**：实现 `/reflect` 命令 + 基础反思 prompt 模板（依赖 #8 已落地的 Memory 接口）
2. **Phase 2**：接入 Stop / TaskUpdate(completed) 自动触发
3. **Phase 3**：错误恢复链路反思 + PostCompact 钩子

**涉及路径**（待实施）：
- `aemeath-core/src/reflection/` — 反思引擎、prompt 模板、写入策略
- `aemeath-core/src/command/commands/reflect.rs` — `/reflect` 命令
- `aemeath-cli/src/tui/app/update.rs` — Stop 事件触发钩子
- `aemeath-cli/src/tui/app/stream.rs` — TaskUpdate / 错误恢复触发钩子

**开放问题**：
- 反思是否消耗当前 session 的 model 调用，还是用独立的轻量 model（成本权衡）
- 反思失败（如 LLM 返回空）时是否静默丢弃 vs 提示用户
- Memory 容量上限策略：何时压缩 / 淘汰旧反思

---

### #10 日志文件规范化

**目标**：明确 `aemeath.log`、`panic.log`、`agent.log` 三个日志文件的职责边界、写入入口、格式约定与轮转策略，避免日志散落、内容互相覆盖、无法定位问题。

**当前状态（待核实）**：
- `aemeath.log` — 主日志（env_logger 输出，CLAUDE.md 已写明）
- `debug.log` — debug 级独立文件（CLAUDE.md 已写明）
- `agent.log` — 子 agent 执行日志（在 Feature #5 描述中提到 "日志写入 agent.log"）
- `panic.log` — 进程 panic 时的崩溃记录（**目前是否已写入待确认**）

存在的问题：
- 没有统一文档说明每个日志文件的语义和何时写入
- panic 路径（与 Bug #4 相关）的崩溃信息是否落盘、落到哪里不清晰
- agent.log 与 aemeath.log 的内容重叠/分工不明
- 缺乏轮转，长期会话日志会无限增长

**设计目标**：

#### 1. 职责边界
| 文件 | 内容 | 写入入口 | 级别 |
|------|------|----------|------|
| `aemeath.log` | 主进程日志（API 调用、tool 执行、状态变更） | env_logger（lib.rs 路由） | warn 默认，可调 |
| `debug.log` | 详细调试信息（API 请求体、stream 事件、状态机转换） | 显式 debug! 宏 | debug |
| `agent.log` | 子 agent 执行轨迹（每个 sub-agent 一段，含 turn N、tool call 名、token 消耗） | `agent_runner.rs` 中 progress 闭包 | info |
| `panic.log` | 进程 panic 捕获（panic 信息、backtrace、当前会话 ID、最近 N 条事件） | `set_hook` 全局 panic handler | error |

#### 2. 格式约定
- 每行 JSON Lines（机器可解析）或 `[时间戳] [级别] [模块] message`（人可读）— **二选一，需决策**
- 时间戳统一 RFC3339（带本地时区）
- session_id 字段贯穿（便于跨文件 grep）

#### 3. 轮转策略
- 单文件超过 10MB 自动 rotate（`aemeath.log.1` / `aemeath.log.2` ...）
- 保留最近 5 份
- 程序启动时清理超过 30 天的旧轮转文件

#### 4. panic.log 关键设计
- 在 `main.rs` 早期注册 `std::panic::set_hook`
- panic 时写入：panic message + backtrace + 当前 session_id + 最近 20 条 ring buffer 事件 + active tool call 状态
- 与 Bug #4（Output Area panic）联动：panic.log 应能直接还原触发场景

**涉及路径**：
- `aemeath-cli/src/main.rs` — panic handler 注册、log dispatch 初始化
- `aemeath-core/src/lib.rs` — env_logger 配置（已有 file appender）
- `aemeath-cli/src/agent_runner.rs` — agent.log 写入入口
- 新增：`aemeath-core/src/logging.rs` — 统一 log 路径与轮转工具

**开放问题**：
- 是否需要每会话一个独立子目录（`~/.aemeath/sessions/<id>/aemeath.log`）便于追溯
- agent.log 是否应该也按 sub-agent 拆分（`agent.log.<turn>.<id>`）

---

### #11 OpenAI reasoning_effort 配置支持

**目标**：支持 GPT-5.x 系列模型（GPT-5、GPT-5.2、GPT-5.4、GPT-5.5）的 `reasoning_effort` 参数，让用户能精确控制 thinking 强度（速度/质量权衡），并通过 `config.json` 持久化配置。

**背景**：
- OpenAI GPT-5.x 通过 `reasoning_effort` 控制思考深度，取值：`none` / `low` / `medium`（默认）/ `high` / `xhigh`
- 当前 aemeath 在 `openai_compatible` provider 只发 `enable_thinking: false` 兼容字段，未传 `reasoning_effort`，所以接到 GPT-5.5 时只能拿到默认 medium 行为
- 不同 reasoning provider 控制方式不同：
  - **OpenAI GPT-5.x**：`reasoning_effort: "low"|"medium"|"high"|"xhigh"|"none"` 或 Responses API 的 `reasoning: {"effort": "..."}`
  - **DeepSeek**：`thinking: {"type": "enabled"|"disabled"}`（仅 on/off）
  - **GLM/Qwen 等**：`enable_thinking: true|false`（仅 on/off）
  - **Anthropic**：`thinking: {"type": "enabled", "budget_tokens": N}`（带 budget）

**设计**：

#### 1. config.json 字段

新增模型级配置 `reasoning_effort`，与现有 `reasoning` 开关并存：

```json
{
  "models": {
    "default": "gpt-5.5",
    "list": [
      {
        "name": "gpt-5.5",
        "provider": "openai",
        "model": "gpt-5.5",
        "reasoning": true,
        "reasoning_effort": "low"
      },
      {
        "name": "deepseek-r1",
        "provider": "deepseek",
        "model": "deepseek-r1",
        "reasoning": true
      }
    ]
  }
}
```

- `reasoning_effort` 为可选字段，类型 `Option<String>`
- 取值校验：`none|low|medium|high|xhigh`，非法值启动时报错
- 仅对支持的 provider 生效（OpenAI / OpenRouter / OpenAICompatible 路由到 OpenAI 模型时）；其他 provider 收到后忽略 + 日志 warn

#### 2. CLI / 环境变量 / 命令

- CLI 新增 `--reasoning-effort <level>`（与现有 `--reasoning` 并列）
- 环境变量 `AEMEATH_REASONING_EFFORT=low`
- Slash 命令 `/effort [none|low|medium|high|xhigh]`（不带参数显示当前值，带参数切换）
- 配置优先级遵循 CLAUDE.md §1：CLI > env > 项目 config > 全局 config > 默认（medium）

#### 3. Provider 实现

**OpenAI 路径**（`aemeath-llm/src/providers/openai_compatible/non_stream.rs` + `stream.rs`）：
```rust
if reasoning_enabled && self.config.is_openai_reasoning_capable() {
    if let Some(effort) = &self.config.reasoning_effort {
        request_body["reasoning_effort"] = json!(effort);
    }
}
```

**Anthropic 路径**：未来可扩展为 `budget_tokens` 映射（Claude Opus/Sonnet 4.x thinking）：
- `low` → 1024
- `medium` → 4096
- `high` → 16384
- `xhigh` → 32768
- `none` → 不发 thinking 字段

**其他 provider**：忽略 `reasoning_effort`，沿用 on/off 开关。

#### 4. UI 反馈

- status bar 在 reasoning 启用时显示当前 effort 等级（如 `reasoning: high`）
- `/think` 命令保持兼容（仅 on/off），与 `/effort` 互补：
  - `/think off` → 等价于 `reasoning_effort = none`
  - `/think on` → 恢复上次 effort（默认 medium）

#### 5. 模型能力检测

在 `aemeath-core/src/provider.rs` 增加 `supports_reasoning_effort(model: &str) -> bool`：
- GPT-5 / GPT-5.2 / GPT-5.4 / GPT-5.5 / o1 / o3 / o3-mini → true
- 其他 OpenAI 模型 → false（GPT-4o 等不支持）
- 检测时按模型 id 前缀匹配，不匹配时静默忽略 `reasoning_effort` 字段

**测试场景**：
- 配置 `reasoning_effort=low` + GPT-5.5 → 请求体包含 `"reasoning_effort": "low"`
- 配置 `reasoning_effort=high` + GPT-4o → 请求体不包含该字段（不支持）
- 配置 `reasoning_effort=invalid` → 启动报错
- `/effort high` 后切换模型到 deepseek → effort 字段被忽略，日志 warn
- 不配置 `reasoning_effort` → 行为与现状一致（OpenAI 默认 medium）

**涉及路径**：
- `aemeath-core/src/config/models.rs`（新增 `reasoning_effort` 字段）
- `aemeath-core/src/provider.rs`（`supports_reasoning_effort` 能力检测）
- `aemeath-llm/src/providers/openai_compatible/non_stream.rs` + `stream.rs` + `mod.rs`（构造请求体）
- `aemeath-cli/src/cli.rs`（`--reasoning-effort` flag）
- `aemeath-cli/src/main.rs`（env 变量读取 + 配置合并）
- 新增：`aemeath-core/src/command/commands/effort.rs`（`/effort` 命令）
- `aemeath-cli/src/tui/status_bar.rs`（effort 等级显示）

**关联**：与 `/think` 命令、`reasoning` 配置字段并存；后续可扩展到 Anthropic 的 `budget_tokens`。

**开放问题**：
- 是否需要把"按 effort 估算 budget_tokens"的 Anthropic 映射也一起做？还是分两期？
- `/effort` 切换是否在当前 turn 立即生效，还是下一个 turn 生效？（建议下一个 turn，避免请求中断）

---

### #12 Input Queue 双层循环优化

**目标**：让 LLM 在一个 user turn 内部（API call → tool calls → 下一次 API call → tool calls ...）的细粒度节点上**主动检查 input queue**，把用户排队的反馈尽早注入对话流，而不是等整个 agent loop 跑完才"看到"用户的新输入。让用户感受到"agent 听得见我"，而不是"agent 必须把这一摊事干完才理我"。

**背景**：
- Feature #7 已实现多消息 input queue（VecDeque），processing 期间用户可连续排队多条输入
- 当前消费时机是**外层 user-turn 循环**末尾——agent 完成所有 tool call、模型给出最终 stop_reason=EndTurn 后才 pop 一条 queue 进入下一轮
- 痛点：当 agent 进入长链路（连续 N 个 tool call、长 thinking、子 agent 嵌套）时，用户中途看到方向跑偏想纠正，目前必须等整轮结束才能让 agent 看到——体验上像"AI 自顾自跑"，用户反馈延迟极高
- Bug #21（粘贴入队语义）和 Feature #11（reasoning_effort）都是输入控制相关，本 feature 解决"何时让 agent 看到输入"

**设计**：

#### 1. 双层循环模型

```
outer loop: per user turn (现状)
  └─ inner loop: per agent step（API call + tool exec）
     ├─ 每次 inner 迭代开始前：检查 input queue
     ├─ 若 queue 非空：把队列内容作为 user message 注入 messages，跳过本轮原计划，继续 inner loop
     └─ 若 queue 为空：照常发起下一次 API call / 工具执行
```

inner loop 退出条件（沿用现状）：模型返回 `stop_reason = EndTurn` 且无 tool call。

#### 2. 检查点（粗到细）

按介入成本递增分级：

| 检查点 | 介入成本 | 说明 |
|--------|----------|------|
| **A. 每次 API call 前** | 低（必做） | 下一轮请求构造前 pop 全部 queue，作为 user message 拼到 messages 末尾。模型在下次回复时就能看到 |
| **B. tool call 批次完成后** | 低（必做） | 一批并行/顺序 tool call 跑完、准备发回 LLM 前，先 pop queue。最自然的"让 LLM 看到用户新指令"时机 |
| **C. tool call 之间（顺序）** | 中（可选） | 如果 tool call 改顺序执行（Bug #3 的修复方向），可在两个 tool call 之间检查；带"用户已发声"信号意味着后续 tool call 可能被取消 |
| **D. streaming 期间** | 高（不做） | 中断正在进行的 API call。语义复杂、provider 兼容性差，**不在本期范围** |

本期落地 **A + B**。C 留作后续扩展，需要 Bug #3 完成顺序执行后再做。

#### 3. 注入语义

用户排队消息进入 messages 时怎么标记？两种方案：

- **方案 1（普通 user message）**：直接 `Message::user(content)` 拼到末尾，模型自然继续对话
- **方案 2（带元数据的 system note）**：包成 `<user_interrupt>...</user_interrupt>` 或类似标签，提示模型"这是用户中途追加的反馈，请优先采纳"

推荐**方案 1 默认 + 方案 2 配置开关**。普通方案足够大部分场景；标签包裹在 agent 自主决策长链路被纠偏时有用。

#### 4. 取消进行中工作的策略

用户中途插话时，已经 in-flight 的 tool call 怎么办？

- **本期**：让进行中的 tool call **跑完**（不取消），跑完后注入用户消息，下一轮 API call 前模型自己决定要不要采纳
- **后期**（依赖 CancellationToken 基础设施）：选项化的"温柔取消"——给 in-flight tool 发取消信号，taken-effect 后注入用户消息

#### 5. 队列读取并发安全

- 当前 input queue 是 `VecDeque<String>` 包在 App 状态里，UI 线程 push、agent loop 主线程 pop
- 已有共享访问机制（具体待 grep 确认 `Arc<Mutex<...>>` / `tokio::sync::Mutex` / channel）
- 双层循环本期只是**多次调用同一个 pop 接口**，不改并发模型

#### 6. UI 反馈

- 用户在 processing 中输入并 Enter 后：input queue 区显示新条目（已有）
- inner loop 在 A/B 检查点 pop 到消息时：在 output area 注入一条 system 提示行 `[Injected from queue: "..."]`，让用户**看到**"我的反馈被吃进去了"，而不是默默并入下一轮 prompt
- 状态栏可临时高亮 1s 表示"queue 已消费"

#### 7. 配置

`config.json` 新增：
```json
{
  "input_queue": {
    "interrupt_mode": "between_calls",  // off | between_calls | between_tools
    "wrap_with_metadata": false          // 是否用 <user_interrupt> 标签包裹
  }
}
```

CLI 不暴露（属于体验设置，slash 命令 `/queue mode <...>` 切换）。

#### 8. 实施阶段

1. **Phase 1**（本期）：在 `agent_runner.rs` / `processing.rs` 的 inner loop A/B 检查点加 `pop_all_queued()` 调用 + UI 注入提示
2. **Phase 2**：增加 `<user_interrupt>` 包裹选项 + `/queue` slash 命令
3. **Phase 3**（依赖 Bug #3 顺序执行 + cancel 基础设施）：tool call 之间检查（C 检查点）、温柔取消进行中的 tool

**测试场景**：
- 用户 send 消息 → agent 进入 5 个 tool call 链 → 用户在第 2 个 tool 执行时排队 "stop, focus on X" → 期望：第 2 个 tool 跑完后，下一次 API call 前模型立即看到 "stop, focus on X" 并改变方向
- 用户连续排队 3 条 → 一次 pop 全部 → 拼成 3 条 user message 一起注入
- 队列在 inner loop 跑完都没消费过 → 退到 outer loop 时按原逻辑 pop（保持兼容）
- agent 在 ask_user 等待中（Bug #19 已修复）→ queue 不消费，等 ask_user 走完
- subagent 嵌套时：父 agent 的 queue 不应被子 agent 消费；子 agent 自己有独立 inbox（待决策，建议本期父子 agent 都不互通）

**涉及路径**：
- `aemeath-cli/src/agent_runner.rs`（agent 主循环 inner step）
- `aemeath-cli/src/tui/app/processing.rs`（user turn 顶层循环）
- `aemeath-cli/src/tui/app/mod.rs`（input queue 数据结构 + pop 接口）
- `aemeath-cli/src/tui/app/update.rs`（UI 注入提示）
- `aemeath-core/src/config/mod.rs`（`input_queue` 配置）
- 新增（Phase 2）：`aemeath-core/src/command/commands/queue.rs`

**关联**：
- Feature #7（input queue 基础实现，已完成）
- Bug #21（粘贴入队语义）— 必须先确保入队来源干净
- Bug #3（tool call 流式 + 顺序执行）— Phase 3 的 C 检查点依赖
- Bug #19（ask_user 等待态独占 input，已修复）— queue 消费时需绕开 ask_user 状态

**开放问题**：
- 子 agent 是否共享父 agent 的 input queue？默认不共享，但 deeply nested agent 时父用户反馈如何透传？
- 标签包裹 `<user_interrupt>` 是否 model-agnostic？某些模型可能把它当 XML 字面量解析
- 用户排队"取消当前 tool call"语义如何表达？需要一个特殊关键字 / 命令前缀（例如 `/cancel`）还是 LLM 自行从语义判断？

---

### #13 Task list 显示在 spinner 下方

**目标**：把 TUI 临时区域中 task status lines（`✓ / ■ / □ + subject` 任务列表）从 spinner 上方挪到 spinner 下方，让 spinner 紧贴正在流动的输出，而 task list 作为更稳定的"任务面板"位于底部，与 input area 视觉相邻。

**当前现象**：

Bug #24 修复后，临时区域渲染顺序为：

```
queued messages
task status lines      ← task list 当前在 spinner 之上
spinner                ← spinner 紧贴 input area
─────────────
input area
```

实际效果：
- spinner 紧贴 input area，看上去"思考动效"贴住了输入框，与正在生成的内容（output area）距离较远
- task list 夹在 queued messages 与 spinner 之间，每次任务条数变化都会把 spinner 上下推动一行，spinner 像在跳
- 用户感知：task list 是相对稳定的状态，应该"沉底"；spinner 是动效，应该贴近正在生成的内容

**预期行为**：

调整为：

```
queued messages
spinner                ← spinner 紧贴 output 流
task status lines      ← task list 沉到底部，挨着 input area
─────────────
input area
```

理由：
1. spinner 表达"agent 正在工作"，紧贴 output 区域更符合视觉因果（"上面的内容由这个 spinner 推动出来"）
2. task list 是一个相对稳定的"任务面板"，每条任务进入/完成才变化，沉到 input area 上方变成视觉锚点
3. spinner 不再被 task list 条目数变化挤上挤下，动效更稳定

**影响 / 与 Bug #24 的关系**：

- Bug #24 之前的顺序就是 `queued → spinner → task`，但当时因为最终裁剪从底部保留导致 spinner 被挤出，所以才改成 `queued → task → spinner`
- 本期需要把"spinner 永远可见"的保证迁移到新顺序下：当临时区域行数超出可见区域必须裁剪时，**优先裁掉 task status lines 的中部**（保留头几条 + 省略号），而不是裁 spinner
- 也就是说 spinner 行的优先级要高于 task status lines 的"完整列出"

**实现方向**：

1. `aemeath-cli/src/tui/output_area/mod.rs` 调整临时行追加顺序：
   - 改回 `queued messages → spinner → task status lines`
2. 临时区域裁剪策略改为：
   - 必保：spinner 行
   - 次优：queued messages（已有处理）
   - 可截断：task status lines（超长时显示前 N 条 + `… +M more` 行）
3. 验证 Bug #24 描述的场景不退化：
   - 大量 task 行时 spinner 仍可见
   - tool call 切换、queued message 增减时 spinner 不闪烁

**测试场景**：
- 单个 in_progress task → task 行紧贴 input area 上方，spinner 在 task 行之上
- 5+ 条 task（混合 done / in_progress / pending）→ task 列表沉底，spinner 仍位于 task 之上、output 之下
- 临时区域空间不够 → spinner 必显示，task 列表显示前 N 条 + 折叠提示
- 没有 task 时 → spinner 直接挨着 input area，行为退化为 spinner 在底部
- queued messages 同时存在 → 顺序为 queued → spinner → tasks，三段都可见

**涉及路径**：
- `aemeath-cli/src/tui/output_area/mod.rs`（临时区域追加顺序、裁剪策略）
- `aemeath-cli/src/tui/output_area/spinner.rs`（不变，但需确认 spinner 行高仍为 1）
- `aemeath-cli/src/tui/app/render.rs`（reserved height 计算需对应新顺序）

**关联**：
- Bug #24（spinner 偶尔消失，已修复）—— 顺序调整不能让 spinner 重新被挤出，需用裁剪策略保证
- Bug #25（/clear 未清空 status line）—— 顺序调整不影响 reset 路径，但完成时一并验证

**开放问题**：
- task list 折叠阈值取多少合适？建议默认 5 行，超过显示前 4 + `… +N more`
- 是否需要配置项让用户切换"task 在上 / 在下"？倾向不开口子，统一一种布局

---

### #14 Session ID 自增无冲突方案

**目标**：替换当前的 session ID 生成方案，找到一个**单调递增、跨进程/跨设备无冲突、可读性更好**的方案，让 session 列表能按 ID 自然排序，便于人工识别和文件系统按字母序对应时间序。

**当前方案**（`aemeath-core/src/state.rs:new_session_id`）：

```rust
let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(); // u128
let random: u32 = rand::random();
format!("{:016x}{:08x}", timestamp, random)
```

形如：`0000019dc93bab86dfd7032f`（24 位 hex）。

**痛点**：

1. **可读性差**：纯 hex 串，人眼识别不出哪个 session 更新、属于哪一天
2. **排序勉强**：前 16 位是毫秒时间戳 hex，字典序≈时间序——但只有"同一时刻起的字符宽度一致"才成立，时间戳跨阶（如位数变化）排序会错乱（虽然 16 位 padding 暂时够用到 9999 年，实际不会出问题，但仍是巧合而非语义保证）
3. **冲突可能**：同一毫秒并发两次 `new_session_id` 时，仅靠 32 位随机抗冲突——多机/多进程并发概率虽低但仍存在。无显式去重机制
4. **不携带语义**：看 session id 看不出 "什么 cwd / 什么模型 / 第几个 session"
5. **不"自增"**：用户口里说的"自增 ID"通常指可数（#1、#2、#3...）的整数，便于命令行短引用（`/resume 5` 而不是 `/resume 0000019dc93bab86dfd7032f`）

**设计方向**：分两层 ID 共存

#### 1. 内部全局 ID（保留唯一性）

继续生成不可冲突的全局 ID 作为存储路径主键，但格式调整为更清晰：

```
{date_yymmdd}-{seq_in_day}-{rand_4_hex}
例如：260430-0007-a3f1
```

- 前缀 `260430` 可读时间（年月日）
- `0007` 当天第 7 次创建（按本机 `~/.aemeath/sessions/` 同前缀文件计数）
- `a3f1` 4 位随机抗碰撞（即使 seq 计算漏算也极难撞）

或保留底层 ULID / UUIDv7 这类工业方案：
- **ULID**（128bit，时间 prefix + 随机 suffix，base32 编码 26 字符）— 字典序=时间序，跨设备唯一，社区成熟
- **UUIDv7**（标准化的时间排序 UUID）— 与 v4 同等无冲突保证 + 时间可排序

倾向 **UUIDv7 或 ULID**：标准、有现成 crate（`uuid::Uuid::now_v7()` / `ulid` crate）、字典序=时间序、不需要本地状态文件。

#### 2. 用户可见短 ID（人类可数）

在 session 列表中给每个 session 分配一个**单调自增的本地短编号**（per cwd 或 per global），用于命令行短引用：

```
short_id: 1, 2, 3, ... (per global, 不复用)
```

- 存储在 `~/.aemeath/session_index.json`：`{ "next_id": 42, "map": { "1": "<full_id>", ... } }`
- `/resume 5` 可命中 short_id=5 的 session
- 真实持久化目录仍按 full_id（UUIDv7 / ULID），避免 short_id 冲突或迁移困难
- short_id 仅作引用别名，不作主键

#### 3. 迁移兼容

- 旧格式 `{ms_hex_16}{rand_8}` 仍能 load（`validate_session_id` 已支持任意字母数字+`-_`，无需放宽）
- 新建 session 用新格式
- 历史 session 在首次列出时回填 short_id（按文件 mtime 排序分配）

#### 4. 命令行短引用语义

- 输入纯数字 → 当作 short_id 查表
- 输入 base32 / UUID 形态 → 当作 full_id 直接命中
- 都不命中 → 提示并列出最近 N 个

**测试场景**：
- 新建 session → 生成 UUIDv7 / ULID → 字典序与时间序一致
- 同一毫秒并发创建 100 个 session → 全部 ID 不重复
- `/resume 3` 命中 short_id=3 → 正确加载对应 full_id 的 session
- 删除 session → short_id 不复用（避免老引用串接到新 session）
- 迁移：旧 session 仍可 `--resume {old_24_hex}`，列表中也分配到 short_id

**涉及路径**：
- `aemeath-core/src/state.rs`（`new_session_id` 改用 UUIDv7 / ULID）
- `aemeath-core/src/session.rs`（短 ID 索引：加载/写入 session_index.json）
- `aemeath-cli/src/cli.rs` / `aemeath-cli/src/main.rs`（`--resume` 数字形参的解析）
- `aemeath-core/src/command/commands/resume.rs` / `sessions.rs`（短 ID 显示与命中）
- 新增 crate 依赖：`uuid` (with `v7` feature) 或 `ulid`

**关联**：
- 已归档 Bug #15 / #16 / #17（resume 命令路径修复）—— 本期不影响 resume 启动流程，只调整 ID 形态与短引用层
- 与 session 列表 UI（最近 commit "fix: /resume 和 /session list 显示最近 15 条 session + 相对时间"）联动 —— 列表里增加 "#3" 列展示 short_id

**开放问题**：
- short_id 是 per global 自增还是 per cwd 自增？per cwd 更符合"项目内序号"语义，但 cwd 切换时可能混淆
- 已删除的 short_id 是否复用？默认不复用（避免悬空引用），但可能导致编号膨胀
- 是否完全废弃旧格式 ID？建议保留兼容，新建走新方案，存量按需迁移
- ULID vs UUIDv7：ULID 字符更短（26 vs 36），但 UUIDv7 是 IETF 标准；建议 UUIDv7 + 自定义短显示（去掉连字符）

---
**开始日期**：2026-04-27
