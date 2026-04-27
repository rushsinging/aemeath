# 活动中 Feature

| # | 标题 | 优先级 | 状态 | 目标 |
|---|------|--------|------|------|
| 1 | Hook 功能 | - | 实施中 | 参考 Claude Code hook 系统，在关键生命周期点执行用户自定义 shell 命令 |
| 2 | SubAgent 可配置 | - | 部分完成 | 支持通过配置文件定义 agent role（绑定 model、description、system_suffix），Agent tool 通过 `role`/`model` 参数路由到不同 LLM |
| 3 | CLI 子命令 | - | ✅ 已完成 | 支持 `aemeath models`、`aemeath sessions` 等子命令 |
| 4 | AskUserQuestion TUI 美化 | - | 待实施 | AskUserQuestion 向用户确认时，TUI 界面需要美化 |
| 5 | Agent 调用显示优化 | - | ✅ 已完成 | 优化 Agent tool call 的 TUI 显示：调用阶段展示 role/model/description，执行过程展示子 agent 关键进度，结果展示区分 agent 输出与普通 tool 输出 |
| 6 | Task 调用显示优化 | - | ✅ 已完成 | 优化 Task 系列 tool call 的 TUI 显示：TaskCreate/TaskUpdate 脱离静默模式展示关键信息，TaskList 结果格式化为可读表格，Task 生命周期状态变更可视化 |
| 7 | Input Queue 优化 | - | ✅ 已完成 | 将单条 queued_input 改为多消息队列（VecDeque），支持处理期间连续排队多条输入 |
| 8 | Memory 系统 | - | 待实施 | 增加 memory 系统，支持跨会话持久化记忆，在合适时机自动写入/检索上下文 |
| 9 | 反思系统 | - | 待实施 | 在关键节点对过去行为/决策做反思总结，提炼经验写入 Memory 系统（依赖 #8） |

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

#### 事件类型（9 个）

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
**开始日期**：2026-04-27
