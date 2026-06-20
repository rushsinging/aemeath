# 设计：TUI 会话组装管线重构 —— typed 消息 → intent → timeline 单一真相

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/388
> 日期: 2026-06-20
> 状态: 设计待 review
> 范围: P1 typed 消息契约 + P2 统一组装路径 + P3 timeline 单一真相

## 1. 背景与触发

TUI 中 Bash 等工具的大结果**溢出渲染成蓝色 user 消息块**（Issue #386）。根因是 sdk `text_content()` 裸抓任意块的 `text` 字段 + `MessagesSync` 用 `role/source/text_content` **启发式**反推用户消息回显。这只是结构债的一个症状——现状测绘揭示三类根问题：

1. **三条组装路径契约不统一**：
   - (a) live 事件：`ChatEvent` → `agent_event.rs` / `ToolFlowProjector` → `ConversationIntent` → `ConversationModel`。
   - (b) `MessagesSync`：`ui_event.rs:98` 直接覆盖 `self.chat.messages` + 启发式反推回显（绕过 intent）。
   - (c) resume：`render/display/render.rs` + `history_parse.rs` 裸解析 JSON 重建（独立的一套解析）。
2. **双表示未收敛**：`ConversationModel` 同时维护 legacy `blocks: Vec<ConversationBlock>` 与新 `timeline`（迁移中、双写、有 fallback）。
3. **裸啃 JSON + 启发式分类**：`history_parse.rs`(51+ `.get()`)、`render.rs`、`view_assembler/output.rs`、`tool_result.rs`、sdk `text_content` 到处手解析 `serde_json::Value`、用 `role=="user"`/`text_content().is_empty()` 猜消息类型；工具字段提取 `display>message>text` 在多处重复。

## 2. 指导原则

- **typed core, JSON only at edges**（沿用主重构）：JSON 只在持久化/server/provider 序列化边界；TUI 组装全程 typed。
- **单一真相**：会话显示只有一个权威源（timeline）；三条路径产出同一套 typed intent 喂给它。
- **显式分类**：消息类型由 typed 结构判定，**NEVER** 靠 `text_content`/字符串猜。

## 3. P1 — typed 消息契约

- sdk 新增 typed `ContentBlock` enum（镜像 share 的形态，但 **sdk 不依赖 share**）：
  ```rust
  #[serde(tag = "type", rename_all = "snake_case")]
  pub enum ContentBlock {
      Text { text: String },
      Thinking { thinking: String },
      ToolUse { id: String, name: String, input: serde_json::Value },
      ToolResult { tool_use_id: String, content: serde_json::Value, is_error: bool },
      Image { source: ImageSource },
  }
  ```
  `ChatMessage.content: Vec<ContentBlock>`（serde 成与现在**完全相同**的 JSON——持久化 / server 契约零变化）。
- `message_to_sdk` / `message_from_sdk`（`runtime/core/client/mapping.rs`）做 `share::ContentBlock` ↔ `sdk::ContentBlock` 的 typed 映射（取代 `serde_json::to_value`）。
- **显式分类**（取代启发式）：
  - 用户输入 = `role==User` 且含 `Text` 块 且 `source==User`。
  - 工具结果 = `role==User` 且含 `ToolResult` 块。
  - `text_content()` 只取 `Text` 块（#387 已先行修了 sdk 侧，本重构并入并删除所有裸 `get("text")` 分类点）。
- **消灭**：`history_parse.rs` / `render.rs` / `view_assembler/output.rs` / `tool_result.rs` 中针对消息块的 `get("type")`/`get("text")` 裸解析，全部改走 typed `ContentBlock`。

## 4. P2 — 统一组装路径（核心）

- **唯一 intent 词汇表**：保留并整理现有 `ConversationIntent`，作为三条路径的唯一输出。
- **单一投影器** `messages_to_intents(&[sdk::ContentBlock 消息]) -> Vec<ConversationIntent>`：
  - resume **复用**它（删除 `history_parse.rs` 的独立裸解析重建）。
  - live 事件经 `ToolFlowProjector` 产出**同一套** intent（已是 typed，整理对齐）。
- **MessagesSync 退出 display 路径**（根治本次 bug）：
  - display 只由 typed 事件(live) + resume 驱动；`MessagesSync` 仅更新「持久化消息列表」（喂 SaveSession），**不再反推回显**。
  - **队列用户输入回显**：runtime 在「排队输入被 drain（真正进入对话）」时发**显式 `UserMessageAdded { text }` 事件**；TUI 据此走 intent 显示。取代「从 synced 列表启发式反推」。
  - 影响面：runtime 侧 `input_gate`/queue drain 处新增一个事件（小）；sdk `ChatEvent` 加一个 variant；TUI map 到 `AppendUserMessage` intent。

## 5. P3 — timeline 单一真相

- 删除 `ConversationModel.blocks: Vec<ConversationBlock>` 及 `assemble_legacy_conversation_blocks` fallback；`timeline` 为唯一真相。
- `view_assembler/output.rs` 只从 timeline 组装；顺带把工具结果字段提取（`display>message>text` + EnterWorktree/Edit 特化）**归一到一处**（消除 view_assembler / tool_result / history_parse 三处重复）。
- 拆分过大文件：`render.rs`(584，被 `messages_to_intents` 取代后大幅缩小)、`view_assembler/output.rs`(591)、`model/conversation/model.rs`(574) 按职责拆到 <400 行。

## 6. 分阶段实施（每阶段独立可验证 PR）

| 阶段 | 内容 | 行为变化 | 验收 |
|---|---|---|---|
| **T1** | P1：sdk typed `ContentBlock` + mapping + `text_content` 等分类改 typed | ❌（serde 同 JSON） | 单测：序列化往返不变、分类正确；全量 build/clippy |
| **T2** | P2：`messages_to_intents` 投影器，resume 改用它（删 history_parse 裸解析） | ❌（resume 产出等价） | 单测：投影器各消息类型→intent；resume 与旧产出等价 |
| **T3** | P2：runtime `UserMessageAdded` 事件 + MessagesSync 退出 display 路径 | ✅（回显机制变） | 单测：drain→事件→intent；回归 #386；`-qv` 不覆盖，截图人工核对 |
| **T4** | P3：删 legacy blocks，timeline 单一真相 + view_assembler 字段提取归一 + 拆大文件 | ❌（产出等价） | 单测：view_assembler intents→view 树；三路一致性测 |

T1/T2/T4 行为等价（低风险）；T3 是唯一行为敏感阶段，单独验证。

## 7. 验收手段（TUI 难自动化，按目标约定）

- **纯逻辑单测（主力）**：`messages_to_intents`（各消息类型）、view_assembler（intents→view 树）、消息分类、#386 回归。
- **三路一致性测**：同一组 typed 消息，resume 投影 == live 事件累积 == sync 持久化形态，产出相同 timeline / view。
- **截图人工核对** TUI 渲染；`-qv` 仍不覆盖 TUI（保留）。
- 复用 / 扩展现有 `model_tests.rs`(1196)、`output_tests.rs`(570) 的纯逻辑测试基座。

## 8. 非目标（YAGNI）

- 不改 provider / 持久化 JSON 格式（serde 输出保持一致）。
- 不改 AskUserBatch 交互状态机（仅随 typed 块顺带受益）。
- 不引入新的渲染框架 / 主题改动。
