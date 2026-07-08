# 会话历史的单一真相（Session History Single Source of Truth）

**日期**：2026-07-08
**对应 Issue**：[#680](https://github.com/rushsinging/aemeath/issues/680)
**关联**：原则版收进 [`docs/design/01-outline.md` §会话历史单一真相](../../design/01-outline.md)；TUI model 层的对话真相收敛见 [`2026-07-01-conversation-single-source-merge-runtime.md`](2026-07-01-conversation-single-source-merge-runtime.md)
**状态**：草案 —— 「现状诊断」已核对当前 `release/v0.0.8` 代码；「目标架构」为收口方向，尚未落地。

## 背景与动机

[#680](https://github.com/rushsinging/aemeath/issues/680) 表面是一个 microcompact 的 off-by-one：保护边界按最近两个 `Role::User` message 计算，但 `ToolResult` 也是 `Role::User`，导致边界偏移。深挖发现根因是**结构性**的——会话历史的「真相」在运行时散落在多处可变副本，`ChatSegment` / `ChatChain` 这个本应表达「一条 user 消息 + 其触发的完整回合」的聚合形同虚设，`ChatChain::start_new_segment()` 是死代码。

本文档梳理会话历史的领域模型、落盘/给 LLM 的派生、TUI resume 数据流，诊断「真相散落」，并按 **DDD + 六边形 + Clean** 给出唯一真相应在何处的裁决与收口方向。它是 #680 及后续会话历史重构的设计依据。

---

## 一、领域模型：`Message`

`Message` 是 **shared 最小共享内核**的领域类型（`agent/shared/src/message/types.rs:216`），被 runtime / provider / tools 各层直接引用（DRY 单一定义）：

```rust
struct Message {
    role: Role,                    // 只有 User | Assistant
    content: Vec<ContentBlock>,
    metadata: Option<MessageMetadata>,
}
```

- **`Role`（`types.rs:44`）只有 `User` / `Assistant`**。`ToolResult` 挂在 `Role::User` 的消息上——这是 #680 off-by-one 的直接来源：按 `Role::User` 数消息会把工具结果也当成一次「用户输入」。
- **`ContentBlock`（`types.rs:75`）**：`Text` / `Image` / `ToolUse` / `ToolResult` / `Thinking`。
  - `ToolResult` 同时持有**结构化 `content`（给 TUI/server）与可选 `text`（给 LLM 的 text-first）**——一份数据、两种消费视图。
  - `Image.placeholder`（`[Image #N]`）是 round-trip 字段，**不发给 LLM**。
- **`MessageMetadata.source`（`types.rs:57`）** = `User` | `SystemGenerated`，区分**真实用户输入**与**系统注入**（guidance / task 提醒）。判断「大 loop 边界」应当依赖它，而非裸 `Role`。

### 三层投影（同一语义、三个视图）

| 层 | 类型 | 位置 | 角色 |
|---|---|---|---|
| 内核领域 | `Message` | `agent/shared/src/message/types.rs:216` | 唯一真相，各层直接用 |
| 运行时分段 | `ChatSegment` / `ChatChain` | `agent/features/runtime/src/business/session/chat_chain.rs:31/77` | 把 `Vec<Message>` 包成「一条 user 消息 + 完整回合」的段链 |
| SDK 边界 | `ChatMessage` | `packages/sdk/src/session.rs:9` | CLI↔Runtime 契约，`role` 降为 `String`，额外带 `input_id`（TUI 清占位用） |

---

## 二、两种派生：落盘 vs 给 LLM

### 落盘 = 忠实全量

持久化容器是 `Session`（`session/types.rs:37`），持有 `chats: Vec<ChatSegment>`（新格式）+ `messages: Vec<Message>`（旧格式兼容，load 时迁移进 `chats` 后置空）。

- 保存：`save_session_from_handle`（`core/client/trait_session.rs:30`）从运行时**扁平** `current_messages` clone，有 summary → 单个 `Compact` 段，否则 → 单个 `Normal(None)` 段（`trait_session.rs:60-66`）。
- 序列化 `serde_json::to_string_pretty`，**忠实保留**结构化 `content`、`text`、`placeholder`、`metadata`；tmp → fsync → rename 原子写 + `.bak` + `.corrupt` 兜底（`session/storage.rs:64`）。
- 兼容：`migrate_legacy_messages`（`storage.rs:44`）把旧扁平 `messages` 包成单个 `Normal` 段。

> **病灶**：loop 全程用扁平 `Vec` 推进，保存时**不调用 `start_new_segment()`**，整段对话被压进**一个** segment。落盘的 segment 边界不对应真实 user turn。

### 给 LLM = 两级 text-first 瘦身克隆（临时，持久化不受影响）

- **Stage A（消息级、provider 无关）**：`build_api_messages`（`chat/looping/loop_phases.rs:95`）前置注入 `user_context`（claudeMd）与 task 提醒，再对每条消息做 `Message::to_llm_view()`（`types.rs:237`）：`ToolResult` 带 `text` → `content` 降为 text-first、剥离结构化 data 与 `text` 字段；无 `text`（旧 session / 占位符）→ 原样保留（`loop_phases.rs:124`）。
- **Stage B（provider HTTP 级）**：Anthropic 风格下 `Message` 的 serde 即 wire 格式（近乎直发）；OpenAI 兼容走 `convert_messages`（`provider/.../openai_compatible/message_conversion.rs:23`）重组为 OpenAI 消息数组。

```
                       ┌─ 落盘 (storage) ───────────────────────────────┐
运行时扁平             │  Session.chats: Vec<ChatSegment{Vec<Message>}> │  忠实全量
Vec<Message> ──────────┤  serde_json 原子写 + .bak/.corrupt 兜底        │  (结构化 content + text 都留)
(current_messages)     └─ 给 LLM ───────────────────────────────────────┘
                          Stage A: build_api_messages + to_llm_view  (ToolResult 降 text-first、剥 data)
                          Stage B: Anthropic serde 直发 │ OpenAI convert_messages 重组
                          (临时克隆，持久化不变)
```

---

## 三、TUI resume reload 数据流

TUI **不走** `load_session` / `SessionSnapshot`，而是**纯事件流**（SDK trait 只有 `chat()` 一个方法，`packages/sdk/src/client.rs:18`）：

```
入口: --resume<id> (apps/cli/src/args.rs) / /resume<id> (tui/app/slash.rs:226)
  → push ChatInputEvent::ResumeSession{id}         (通过 chat() 的 input_events 通道)
runtime: load_session → SessionRestore::from_session → ChatChain::from_chats → flatten → sanitize
  → 事件 ChatEvent::SessionResumed{ messages: Vec<ChatMessage>, session_id, created_at }
        (packages/sdk/src/chat_event.rs:213)
  → App::resume_session_messages (apps/cli/src/tui/effect/session/resume.rs:6) 落两处：
     ├─ self.chat.messages: Vec<ChatMessage>          ← runtime 权威数组的「镜像」，喂下次 chat() 上下文 + stats，不渲染
     └─ self.model.conversation.timeline               ← 「渲染真相」，由 ResumeConversation intent 逐条 HistoryDisplayMessage::parse 重放重建
                                                          (渲染读 tui/view_assembler/output.rs:59)
```

派生逻辑已 DRY 到纯函数 `SessionRestore::from_session`（`session/restore.rs:47`，#636 修复），两条 resume 路径（`load_session` RPC 与 loop 内 `ResumeSession`，`loop_runner.rs:426`）复用它。

---

## 四、诊断：真相散落 ≥5 处

同一份「会话」在运行期同时被物化多份，并带双向同步：

| # | 位置 | 形态 | 谁维护 |
|---|---|---|---|
| 1 | 磁盘 `Session.chats` | `Vec<ChatSegment>` | 持久化序列化 |
| 2 | runtime `current_messages` | 扁平 `Vec<Message>`（**非聚合**，`accessors.rs:46`） | loop 事件回写 |
| 3 | loop 本地 `messages` | 扁平 `Vec<Message>`（`trait_chat.rs:75`） | loop 推进 |
| 4 | TUI `self.chat.messages` | `Vec<ChatMessage>`（`state/chat.rs:6`） | runtime **每 turn 整块覆盖**（`ui_event.rs:128/140/149/155/160/171/178`） |
| 5 | TUI `conversation.timeline` | 渲染 item 流 | resume 重放灌一次 + 细粒度事件**增量**维护 |

关键问题：

1. **聚合被贫血扁平表绕过**：运行期真相是扁平 `Vec<Message>`（#2/#3），`ChatChain` 只在 load/save 瞬间存在，`start_new_segment()`（`chat_chain.rs:124`）是死代码 → turn 边界在运行时被摧毁（**#680 根因**）。
2. **TUI 内部两份独立物化**（#4 上下文镜像 + #5 渲染态），用两套机制重建，可能漂移，且哪个都不是权威。
3. **runtime 每 turn 整块 push 覆盖 `chat.messages`**（7 处）→ 应是 TUI 订阅细粒度事件增量投影。
4. **`sync_current_messages_impl` 反向写回**（TUI → runtime，`trait_session.rs:10`）→ 驱动侧适配器直接 mutate 领域态，依赖倒置。
5. **上下文与渲染职责错配**：TUI 不该持有 `chat.messages` 来「喂下次 chat() 上下文」——上下文归 runtime 聚合，TUI 缓存再回喂是领域态泄漏到适配器。

---

## 五、裁决：唯一真相应在 `ChatChain` 聚合（DDD + 六边形 + Clean）

**唯一真相 = runtime/application core 单一持有、单一可变的领域聚合 `Session` + `ChatChain`。其余全部是它的派生投影。**

- **领域聚合（最内层，唯一真相）**：`Session` 为聚合根，持有 `ChatChain`（`Vec<ChatSegment>`）。turn 边界、段链、compact 分叉、消息完整性等**不变量归它管**。运行期应**直接持有并 mutate 这个聚合**（真实 user 输入才 `start_new_segment`，回合内 assistant/tool 都 `push` 进活跃段）。
- **扁平 `messages(): Vec<Message>` 只是「给 LLM 的按需派生读模型」**，用时计算、算完即弃，**NEVER** 反过来当运行时存储。
- **持久化 = 出站 `SessionRepository` 端口**（`session/storage.rs` 为适配器）。磁盘 JSON 只是聚合的序列化快照，**仅在「静止/重启」时权威**；运行期权威在内存聚合。
- **TUI = 驱动侧适配器 / 纯读模型**：只保留一份只读渲染态，订阅领域事件增量维护；**不拥有、不回写**权威态。resume 数据**单向流** `domain → event/query → TUI view`。

### 对齐项目既有 doctrine（非另立标准）

- `specs/project.md`：`WorkspaceService` 是**唯一可变 workspace 状态源**，单锁，"NEVER 在别处另建可变状态或缓存副本" + COLA 分层 + git 出站端口。
- TUI **输入唯一真相**在 `model.input.document`。

输入与 workspace 都已收敛为「单一可变真相 + 投影」，**会话历史是目前唯一未收口的核心状态**，应照同一模式治理。

---

## 六、目标架构与收口方向

```
        ┌─────────────────────────────────────────────────────────┐
        │  Runtime App Core                                        │
        │    Session (聚合根)                                       │
        │      └── ChatChain  ← 唯一可变真相，按真实 user turn 维护 │
        │            segments: Vec<ChatSegment>                    │
        └──────┬───────────────┬───────────────┬──────────────────┘
   出站端口     │                │ 领域事件       │ 按需派生
   (repository) ▼                ▼(投影)         ▼(read model)
     磁盘 JSON 快照        TUI 渲染投影        LLM text-first 视图
     (仅静止时权威)        (只读, 单向)         (to_llm_view, 用完即弃)
```

收口动作（一次改到位，方向从内到外）：

1. **runtime 直接持有 `ChatChain` 聚合**为唯一活跃真相，按 turn 维护段；扁平 `to_llm_view` 列表仅作按需派生读模型。
2. **持久化端口化**：`save` 忠实序列化聚合（保住真实段），`load` → `SessionRestore` 重建聚合；磁盘仅静止时权威。
3. **TUI 投影化**：只保留 `conversation.timeline` 一份只读渲染态；**删除 `chat.messages` 上下文镜像**（stats 从同一投影派生）；**废除 `sync_current_messages` 回写**；**收敛为单一 resume 落点**。

---

## 七、与 #680 的关系

#680 要「按最近 3 个大 loop 保护」，前提是运行期真的存在 segment 边界。当前边界在 save 时被抹平，microcompact 只能退化成数 `Role::User`（含 tool_result）→ 偏移。**先把 `ChatChain` 提升为唯一活跃聚合、按 turn 维护段**，与本收口是同一件事——#680 是这次收口最直接的受益点。

**当前止血**：在本收口落地前，microcompact 已由 [PR #681](https://github.com/rushsinging/aemeath/pull/681) **完全禁用**（注释 `loop_runner.rs` 与 `loop_helpers.rs` 的 `microcompact_messages` 调用，含 `MicrocompactDone` emit）；#680 修复后需取消 `TODO(#680)` 注释恢复。

---

## 附：迁移风险与兼容

- **旧 session 兼容**：`migrate_legacy_messages`（`storage.rs:44`）已把扁平 `messages` 迁移为单个 `Normal` 段；收口后需保证「单段旧 session」重建聚合时按 `is_user_turn_boundary`（依据 `MessageSource` + 非纯 ToolResult）**重新切分**为多段，否则旧会话仍是一个大段。
- **完整性**：聚合重建 / 落盘往返 **MUST** 经 `sanitize_messages` + `check_message_integrity`（`restore.rs:61-73`），避免孤儿 `ToolResult` / `ToolUse`。
- **分阶段建议**：可先在 runtime 内维护聚合并按 turn 分段（修 #680），再逐步废除 TUI `chat.messages` 镜像与 `sync_current_messages` 回写，降低单 PR 风险。
