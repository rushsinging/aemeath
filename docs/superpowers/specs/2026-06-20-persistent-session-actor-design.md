# 设计：持久化会话 actor + 事件驱动输入

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/390
> 日期: 2026-06-20
> 状态: 设计待 review
> 取代: #388 原 T3 / T4（并入本设计）

## 1. 背景与根问题

`#388` 深入后暴露 runtime 对话/输入生命周期的结构性问题（均有代码佐证）：

1. **每回合 spawn、跑完即死**：`trait_chat.rs:62` 每次 `chat()` 都 `tokio::spawn(process_chat_loop)`；loop 答完 break、任务结束；下条消息再 spawn 新 loop。
2. **TUI 替 runtime 组装首条**：非忙提交走 `update_enter` 预拼 `ChatRequest.messages`（历史 + 新输入）；忙时提交却已是事件机制（`ChatInputEvent` → `input_gate` drain）。**两套并存、不一致**。
3. **input_gate 越权 + 启发式回显**：`input_gate` 本职 gating/drain，却兼任中途消息 append；回显靠 `MessagesSync` 用 `role/source/text_content` 反推（#386 根源）。
4. **内容去重破坏一致性**：`input_gate.rs:79-86` 按 `(text, image_paths)` 静默去重；`/clear` abort 回滚 `messages.pop()`——靠 FIFO 顺序对应"占位↔回显"不安全。
5. **双表示未收敛**：`ConversationModel` 同时维护 legacy `blocks` 与 `timeline`。

## 2. 指导原则

**runtime 拥有对话与输入；TUI 是纯视图——只发输入事件、只靠 runtime 通知回显。** 单一真相（timeline）；显式 typed 分类（不靠字符串/text_content 猜）。

## 3. 目标架构

### 3.1 常驻会话 actor（生命周期）
- `process_chat_loop` 随 TUI 启动 **spawn 一次**；回合结束后**不 break，`await` 下一条输入事件**，收到后继续下一回合。直到会话关闭 / 取消才退出。
- `AgentClient::chat()` 契约：从"每回合发一次请求"→ **"启动会话一次，返回事件流(入) + 通知流(出)"**。首条不再随请求体直传 messages。
- 兼容：`chat_text`（非 TUI 一次性）内部转成"喂一条输入事件"复用同一 actor。

### 3.2 统一输入事件 + 单一 append 点
- 首条与中途**同构**：都走 `ChatInputEvent::UserMessage { id, text, image_paths }`。
- runtime **单一** `append_user_message(id, text, images)`：append 进工作集 + 发归宿事件。**只此一处**（不再散落 input_gate / chat_impl，也不再由 TUI 预拼）。`input_gate` 回归纯 gating（drain + 决策），不碰回显。

### 3.3 correlation id 归宿（保证 drain==回显一致）
- **`InputId`**（ULID，复用 `sdk::ids` 既有 ULID 体系，新增一个 newtype）；TUI 提交时分配，全链路携带。
- runtime 对**每条**输入回带同 id 的归宿事件：
  - 接受 → `UserMessageAdded { id, text }`（= 回显）。
  - 去重/abort 丢弃 → `UserMessageDropped { id }`。
- TUI 占位块以 **id 为 key**，按归宿事件清除/回显——一致性**由 id 构造保证**，与顺序/去重/abort 无关。
- **去重改为按 id**（防重发），**移除内容去重**（用户可重复发同一文本）；任何被丢弃的 id 都发 `Dropped` 通知，杜绝孤儿占位。

### 3.4 TUI 纯化
- 只发 `ChatInputEvent`；回显**只**来自 `UserMessageAdded`；占位**只**靠 id 归宿事件清。
- `MessagesSync` 退出 display 路径：降级为**纯持久化**——仅更新落盘镜像（`current_messages`），不再驱动任何 display；display 完全由归宿/通知事件驱动。

### 3.5 timeline 单一真相（原 #388 T4）
- 删 `ConversationModel.blocks` 及 `assemble_legacy_conversation_blocks` fallback；view_assembler 只从 timeline 组装；工具结果字段提取归一到一处。

## 4. 分阶段实施（每阶段独立 PR）

| 阶段 | 内容 | 行为变化 | 验收 |
|---|---|---|---|
| **A1** | 常驻 loop：spawn 一次 + 回合间 await 输入；chat() 契约 → 启动会话 + 事件/通知流 | ✅ 生命周期变 | 纯逻辑单测 loop 跨回合；**TUI 肉眼验**对话连续性 |
| **A2** | `InputId` + 统一输入事件（首条也走事件）+ 单一 `append_user_message` + `UserMessageAdded/Dropped` 归宿 + id 去重（去内容去重） | ✅ 输入路径变 | 单测 append→归宿事件、id 对应、去重→Dropped；**TUI 验**首条/插话 |
| **A3** | TUI 纯化：回显只认归宿事件、占位按 id 清、MessagesSync 退出 display | ✅ 回显机制变 | 单测投影；**TUI 验**占位清除/不重不漏不错位（回归 #386） |
| **A4** | timeline 单一真相：删 legacy blocks + view_assembler 归一 | ❌ 产出等价 | 单测 intents→view 树；三路一致性 |

A1-A3 行为敏感（强交互），A4 行为等价。

## 5. 验收手段

- **纯逻辑单测（主力）**：`append_user_message`→归宿事件、id 对应、id 去重、loop 跨回合状态、view_assembler intents→view。
- **三路一致性测**：常驻 loop 跨回合产出 == resume 投影。
- **交互渲染人工/截图验**（首条/插话回显、占位按 id 清除、不重/不漏/不错位）——TUI 难自动化区，需用户在场。
- `-qv` 不覆盖 TUI（保留）。

## 6. 风险

- **runtime 核心 loop 生命周期改造**，影响所有对话；A1-A3 强交互、失败模式纯视觉。**MUST 与用户配合做 TUI 验收**（改一段→跑→看）。
- 取消 / abort / `/clear` 语义在常驻 loop 下需重新对齐（原靠 loop break，现靠状态机回到空闲）。
- 并发：常驻 loop 的可变状态（messages、turn 状态）单一 owner，避免与 sync 锁竞争。

## 7. 非目标（YAGNI）

- 不改 provider / 持久化 JSON 格式。
- 不引入多会话并发 actor（单会话单 loop 即可）。
- AskUserBatch 交互状态机随 typed 块顺带受益，不专门重做。

## 8. 已敲定的 open decisions

- **去重**：移除内容去重，改 id 去重（防重发）；被丢弃 id 必发 `UserMessageDropped`。
- **id 类型**：新增 `InputId`（ULID，复用既有 id 基建），不复用 `ChatTurnId`（语义不同：一次输入 ≠ 一个回合）。
