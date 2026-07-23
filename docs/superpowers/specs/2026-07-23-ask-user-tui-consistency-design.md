# AskUserQuestion TUI 输入、完成态与 resume 一致性设计

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/1358
> 日期: 2026-07-23
> 状态: 已批准，待实现

## 1. 目标

本修复让 AskUserQuestion 在实时交互、完成态展示和 session resume 三个阶段使用同一组语义：

1. 单问且无预设选项时，字符、编辑光标和提交都属于 Ask 块，底部 InputArea 保持为空。
2. 完成态采用紧凑两行布局：Q 与“已回答”标题同边界，A 只缩进一级。
3. resume 恢复历史中所有成功完成的 Ask 问答摘要，顺序与实时会话一致。
4. 连续出现新的 Ask 时，只替换尚未完成的交互块，已经完成的摘要继续保留。

本修复不改变 Runtime suspension 协议、SDK 消息 schema、session 落盘格式、Ask 工具输入格式或 ToolResult 格式。

## 2. 根因

### 2.1 输入真相分裂

`UiEvent::AskUserBatch` 对“单问 + 无选项”保留了一条特殊路径：只保存
`ask_user_reply_tx`，键盘输入写入全局 `InputDocument`。2026-07-05 的 chat-input cursor
改动已经让渲染读取 Ask timeline item 的 `chat_input_text/chat_input_cursor`，但没有同步删除这条
遗留输入路径。因此 Ask 块显示自己的空光标，实际文本却进入底部 InputArea。

### 2.2 完成态叠加固定缩进

`qa_summary_lines` 同时为 marker、Q 行和 A 行添加固定空格，再叠加输出区 gutter，导致摘要相对
标题横向漂移。完成态没有交互 marker，不需要保留确认页的导航缩进。

### 2.3 Ask 完成态只存在于实时 TUI 投影

实时回答通过 `OutputTimelineItem::AskUserBatch { confirmed: true }` 展示摘要；session 中持久化的
仍是标准 `ToolUse + ToolResult`。`ResumeConversation` 只重建通用 ToolCall/ToolResult，而
AskUserQuestion 的 result render policy 为 Hidden，所以 resume 后只剩 `✓ Ask`，没有问答摘要。

此外，所有 Ask 块共用固定 ID `ask-user`，`show_ask_user_batch` 会删除任何旧 Ask 块，连已完成
摘要也一起移除。这使实时历史本身也无法稳定保留多次 Ask。

## 3. 方案比较

### 方案 A：稳定 Ask 身份 + 从标准历史重建（采用）

- 每个 Ask batch 使用由首个 tool call ID 派生的稳定 timeline ID。
- 交互状态只定位尚未完成的 Ask 块，完成块保持不可变。
- 实时路径统一使用 Ask 块内输入状态。
- resume 从已持久化的 ToolUse input 与匹配 ToolResult 派生完成态 Ask 块。

该方案不改落盘协议，同时保证实时和恢复后的显示语义一致。

### 方案 B：只在 resume 渲染临时文本摘要

改动较小，但实时 timeline 仍会删除旧完成块，恢复视图与实时视图继续分叉；临时文本也绕过
Ask view model/render 规则，无法共享布局与测试。

### 方案 C：新增 session 专用 Ask 展示字段

可以直接落盘 TUI 状态，但会把纯展示状态扩散到 SDK、Runtime、Context/Storage 和兼容迁移。
现有 ToolUse/ToolResult 已完整保存问题、答案与 identity，没有新增 schema 的必要。

## 4. 目标模型与不变量

### 4.1 稳定 ID

Ask timeline ID 使用固定前缀加 batch 中首个 slot 的 tool call ID，例如
`ask-user-<tool-call-id>`。同一 provider response 中的多个 Ask tool calls 仍组成一个 batch，首个
ID 作为 batch identity；tool call ID 在单个 session 内唯一，因此无需第二套随机 ID。

### 4.2 活跃块与完成块

Ask timeline 必须满足：

- 同一时刻最多一个 `confirmed == false` 的活跃 Ask 块。
- 可以存在任意多个 `confirmed == true` 的完成块。
- `ask_user_snapshot`、字符编辑、导航、回答和确认只读取或修改活跃块。
- 新的实时 Ask 到达时，仅删除旧的未完成块；完成块保持原顺序。
- resume 追加的完成块没有 reply channel，也永远不能重新进入交互态。

内部实现复用一个 push helper 创建 Ask timeline item；实时入口创建活跃块，历史恢复入口创建
完成块，避免两套字段初始化逻辑漂移。

### 4.3 无选项问题自动进入块内输入态

创建或导航到当前 slot 时：

- `options.is_empty()` → `chat_input_active = true`。
- 存在 options → `chat_input_active = false`，只有选择内建 `Type something...` 后才激活。

该规则适用于单问和多问，也适用于重新导航到无选项问题。所有 Ask 事件都保存为
`AskUserState { reply_tx, items }`；删除 `InputState.ask_user_reply_tx` 及其读取全局
`InputDocument` 的特殊键盘分支。Ask 回答期间全局输入文档不再作为备用真相。

## 5. 实时数据流

```text
ChatEvent::AskUserBatch
  → UiEvent::AskUserBatch
  → 所有 items 统一转换为 AskUserSlot
  → ConversationIntent::ShowAskUserBatch
  → 新建唯一活跃 Ask timeline item
  → InputState::AskUserState 仅持 reply_tx + 原始 items

无选项当前 slot
  → chat_input_active=true
  → key events 修改 chat_input_text/chat_input_cursor
  → AnswerCurrentAskUser
  → 单问直接 confirmed；多问进入下一题或确认页
  → submit 从 timeline 收集答案并发送 AskUserReply
```

底部 `InputDocument` 在整个流程中保持不变；Ask 取消时只清理 Ask coordinator 与活跃块。

## 6. resume 投影

`ResumeConversation` 继续以标准 SDK `ChatMessage` 为输入，不新增持久化字段。

对每条 assistant message：

1. 使用现有 `collect_following_tool_results` 收集紧随其后的 user ToolResult blocks，以
   `tool_use_id` 建立映射。
2. 仍按现有逻辑重建每个通用 ToolCall 与 ToolResult，保持工具状态和结果 payload 行为不变。
3. 同时收集本条 assistant message 内 `name == "AskUserQuestion"` 且存在成功匹配结果的调用。
4. 从 ToolUse input 读取非空 `question`；从匹配结果依次读取：
   - object content 的 `answer` 字段；
   - ToolResult 的 text-first `text` 字段；
   - 字符串 content。
5. 对成功解析的调用生成只含恢复展示所需字段的 `AskUserSlot { id, question, answer }`；
   options/default/multi-select 不参与完成态渲染，无需重演交互状态。
6. 按 tool call 在 assistant message 中的顺序，把收集到的 slots 作为一个 confirmed Ask batch
   追加到 timeline。

每条 assistant message 形成至多一个恢复 batch，因此同一轮并行 Ask 的分组与实时
`AskUserBatch` 事件保持一致；跨多轮的完成 batch 全部保留。

### 6.1 兼容与失败语义

- 匹配 ToolResult 缺失：保留通用 pending/ready Ask 工具行，不生成“已回答”摘要。
- `is_error == true`：保留错误工具结果，不生成完成摘要。
- input 缺少非空 question：跳过该摘要，不使整个 resume 失败。
- result 无法解析答案：跳过该摘要，不伪造空答案。
- 一个 batch 中部分调用可恢复、部分不可恢复：只展示可证明成功的 slots；所有通用工具行仍完整保留。
- 旧 session 的 string content 和 text-first 字段均受支持；不覆写或迁移历史文件。

## 7. 完成态布局

采用用户确认的紧凑两行方案：

```text
━━ 已回答 ━━

Q1. 你打算吃什么？
  ❯ 日料吧
```

Q 行不再包含确认页导航 marker 的预留列；A 行仅比 Q 行缩进一级。确认页仍可保留自己的导航
marker 布局，因此完成态与确认态应使用不同的 summary prefix 参数或独立的轻量 helper，避免
为修复完成态破坏可交互确认页。

## 8. 测试策略

严格执行 TDD：每项生产行为都先新增复现测试，运行并确认因当前缺陷失败，再做最小修复。
新增或迁移的测试必须放在同级 `*_tests.rs` 或现有 `scenario_tests/` 中；不得向生产 `.rs`
文件继续追加 inline test。修改到已有 inline test 模块时，只迁移本次涉及的模块，不做全仓整理。

### L1：状态、解析与渲染

- 新建无选项 Ask 后自动激活块内输入；进入下一道无选项题和重新导航时同样激活。
- 新 Ask 只移除未完成块，保留多个完成块；snapshot 永远选择活跃块。
- batch ID 稳定派生自首个 tool call ID，多个完成块不冲突。
- resume 答案解析覆盖 `content.answer`、text-first、字符串 content、缺失、error 与畸形 input。
- 完成态 plain lines 精确断言 Q 无额外前导空格、A 只有一级缩进；确认页 marker 不回归。

### L2：相邻层投影

- `ResumeConversation` 用两轮 Ask ToolUse/ToolResult 重建两个 confirmed timeline items，顺序、
  slot identity、question 和 answer 完整。
- ViewAssembler 将两个完成块分别投影为稳定 `AskUserBatchBlockView`，不合并、不覆盖。
- 实时 UiEvent 映射后的 model 状态证明无选项 Ask 使用 coordinator + timeline 输入，而非
  `InputDocument`。

### L4：TUI 场景测试（用户明确要求）

- TestBackend 展示无选项单问后输入 `222`：Ask 行出现 `222` 和块内光标，底部 InputArea
  仍为空；Enter 后 reply 为 `Answers(["222"])`，最终 framebuffer 显示紧凑完成摘要。
- 用包含两轮 Ask ToolUse/ToolResult 的 session 消息执行 resume：最终 framebuffer 同时显示
  两个“已回答”摘要及各自 Q/A，顺序正确。
- 场景先用语义断言建立 RED 证据，再更新/新增 insta snapshot；snapshot 不替代语义断言。

### L0 与门禁

- `cargo fmt --all -- --check`
- CLI 定向测试与 `cargo test -p cli`
- `cargo build -p cli`
- `cargo check -p cli`
- `cargo clippy -p cli --all-targets`
- 相关 architecture guards；创建 PR 前执行仓库完整 pre-push 门禁。

L3 不适用：不修改 SDK Published Language、Port 或序列化格式。L5 不适用：无 raw mode、真实
EventStream 或平台职责变化，L4 TestBackend 覆盖完整用户旅程。

## 9. 文档对齐与退役检查

开发前核对：

- `specs/tui-cli.md` 的输入单一真相、TEA 副作用纪律和 Ask result hidden policy。
- `docs/design/03-engineering/04-testing-and-coverage.md` 的 L0-L5 责任、TUI L4 场景测试与
  测试文件分离规则。

实现中必须检查并退役：

- `InputState.ask_user_reply_tx`。
- `update_ask_user_key` 中读取全局 InputDocument 的无选项 Ask 分支。
- 仅服务该分支的 `update_ask_user_input_key`。
- 固定单例 `ASK_USER_BLOCK_ID` 及“删除全部 Ask 块”的旧 helper。

若发现这些路径仍有其他生产消费者，必须回到根因分析并更新设计，不能保留第二输入真相作为
静默兼容层。
