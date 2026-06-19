# Chat 链 + Summary 注入 System 通道 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Session 从扁平 `messages: Vec<Message>` 重构为 `chats: Vec<ChatSegment>` chat 链结构。full compact 变为持久化分叉操作（旧链冻结 + 新建 `Compact` 段），summary 走 system 通道注入；删除 microcompact 事后截断；resume 只加载活跃链（从最后一个 `Compact` 段到末端），天然不需再 compact。

**Architecture:**
- Session 持久化 `chats: Vec<ChatSegment>`，可能包含多条链（compact 冻结的旧链 + 活跃链）。
- 每条 user 消息 = 一个 `Normal` 段（`parent_id` 指向前一段）；`Compact` 段（`parent_id=None`, `summary=Some(...)`）是新链起点。
- resume 时从最后一个 `kind==Compact`（或无 Compact 则首个 `parent_id==None`）的段向后顺链加载 messages。
- summary 不再作为 user 消息注入 messages，而是拼入 `system_blocks` 走 system 通道。
- 运行时引入 `ChatChain` 管理活跃链的 segments，提供扁平 `messages()` 视图供 loop 使用。
- 删除 `microcompact`：产生时已由 `truncate.rs` 定型，事后截断破坏缓存前缀。

**Tech Stack:** Rust、serde、tokio fs、cargo test、cargo clippy。

---

## File Structure

- Create: `agent/features/runtime/src/business/session/chat_chain.rs`
  - `ChatSegment` / `SegmentKind` 类型定义、`ChatChain` 活跃链管理器。
- Modify: `agent/features/runtime/src/business/session/types.rs`
  - `Session` 结构：新增 `chats` 字段；加载/保存适配。
- Modify: `agent/features/runtime/src/business/session/storage.rs`
  - `load_session` / `save_session` 适配新结构；向后兼容迁移。
- Modify: `agent/features/runtime/src/business/session/mod.rs`
  - 导出 `chat_chain` 模块。
- Modify: `agent/features/runtime/src/core/client/trait_session.rs`
  - `save_current_session_impl`：从 `ChatChain` 序列化 segments；`load_session_impl`：只加载活跃链。
- Modify: `agent/features/runtime/src/business/chat/looping/loop_runner.rs`
  - `ChatLoopContext` 增 `active_summary: Option<String>`；compact 后替换 messages + 注入 summary 到 system。
- Modify: `agent/features/runtime/src/business/chat/looping/compact.rs`
  - `auto_compact` 返回 `CompactOutcome { summary, messages }` 而非 `Option<Vec<Message>>`；删除 microcompact 预处理。
- Modify: `agent/features/runtime/src/business/compact/summary.rs`
  - `compact_messages_with_llm` 返回 summary 文本 + recent tail（而非注入 user 消息）。
- Modify: `agent/features/runtime/src/business/compact.rs`
  - 删除 `microcompact` re-export；更新模块文档。
- Modify: `agent/features/runtime/src/business/compact/micro.rs`
  - 删除或清空文件（保留模块声明防引用断裂，或直接删除）。
- Modify: `agent/features/runtime/src/business/agent/runner/loop_helpers.rs`
  - 删除子代理循环中的 `microcompact` 调用。
- Modify: `agent/features/runtime/src/business/session/tests.rs`
  - 补充新结构的序列化/反序列化测试。

---

## Phase 1: 数据模型与持久化

### Task 1: 新增 ChatSegment / SegmentKind / ChatChain 类型

**Files:**
- Create: `agent/features/runtime/src/business/session/chat_chain.rs`
- Modify: `agent/features/runtime/src/business/session/mod.rs`

- [ ] **Step 1: 定义 ChatSegment 和 SegmentKind**

Create `agent/features/runtime/src/business/session/chat_chain.rs`:

```rust
//! Chat 链结构：Session 内按 user 消息分段，compact 产生新链。

use serde::{Deserialize, Serialize};
use share::message::Message;
use sdk::ids::ChatId;

/// 段类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    /// 正常对话段（一条 user 消息 + 其触发的完整回合）
    #[default]
    Normal,
    /// compact 产生的新链起点
    Compact,
}

/// Session 内的一个 chat 段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSegment {
    /// 段 ID（UUIDv7）
    pub id: String,
    /// 父段 ID；Normal 段指向前一段，Compact 段为 None（新链起点）
    #[serde(default)]
    pub parent_id: Option<String>,
    /// 段类型
    #[serde(default)]
    pub kind: SegmentKind,
    /// Compact 段的摘要文本（走 system 通道）；Normal 段为 None
    #[serde(default)]
    pub summary: Option<String>,
    /// 该段的消息列表
    #[serde(default)]
    pub messages: Vec<Message>,
}

impl ChatSegment {
    /// 创建 Normal 段
    pub fn normal(parent_id: Option<String>) -> Self {
        Self {
            id: ChatId::new_v7().to_string(),
            parent_id,
            kind: SegmentKind::Normal,
            summary: None,
            messages: Vec::new(),
        }
    }

    /// 创建 Compact 段（新链起点）
    pub fn compact(summary: String, recent_messages: Vec<Message>) -> Self {
        Self {
            id: ChatId::new_v7().to_string(),
            parent_id: None,
            kind: SegmentKind::Compact,
            summary: Some(summary),
            messages: recent_messages,
        }
    }
}

/// 运行时活跃链管理器
#[derive(Debug, Clone, Default)]
pub struct ChatChain {
    /// 活跃链的所有段（从 Compact/首个段到末端）
    segments: Vec<ChatSegment>,
}

impl ChatChain {
    /// 从 Session 的全部 chats 中提取活跃链
    pub fn from_chats(chats: &[ChatSegment]) -> Self {
        let start = chats
            .iter()
            .rposition(|s| s.kind == SegmentKind::Compact)
            .or_else(|| chats.iter().position(|s| s.parent_id.is_none()));
        let segments = match start {
            Some(idx) => chats[idx..].to_vec(),
            None => Vec::new(),
        };
        Self { segments }
    }

    /// 扁平视图：合并所有段的 messages（供 loop 使用）
    pub fn messages(&self) -> Vec<Message> {
        self.segments
            .iter()
            .flat_map(|s| s.messages.iter().cloned())
            .collect()
    }

    /// 活跃链的 summary（首个 Compact 段的 summary）
    pub fn active_summary(&self) -> Option<&str> {
        let first = self.segments.first()?;
        if first.kind == SegmentKind::Compact {
            first.summary.as_deref()
        } else {
            None
        }
    }

    /// 追加消息到最后一个段
    pub fn push(&mut self, msg: Message) {
        if let Some(last) = self.segments.last_mut() {
            last.messages.push(msg);
        }
    }

    /// 新建 Normal 段（新 user 消息边界）
    pub fn start_new_segment(&mut self) {
        let parent_id = self.segments.last().map(|s| s.id.clone());
        self.segments.push(ChatSegment::normal(parent_id));
    }

    /// compact 分叉：用 summary + recent tail 替换活跃链
    pub fn compact(&mut self, summary: String, recent_messages: Vec<Message>) {
        self.segments = vec![ChatSegment::compact(summary, recent_messages)];
    }

    /// 活跃链的段列表（供持久化）
    pub fn active_segments(&self) -> &[ChatSegment] {
        &self.segments
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.segments.iter().all(|s| s.messages.is_empty())
    }
}
```

- [ ] **Step 2: 导出 chat_chain 模块**

在 `agent/features/runtime/src/business/session/mod.rs` 中添加 `pub mod chat_chain;` 和 `pub use chat_chain::{ChatChain, ChatSegment, SegmentKind};`。

### Task 2: Session 结构迁移

**Files:**
- Modify: `agent/features/runtime/src/business/session/types.rs`

- [ ] **Step 3: Session 增加 chats 字段（向后兼容）**

```rust
#[derive(Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub cwd: String,
    #[serde(default)]
    pub messages: Vec<Message>,        // 旧格式兼容（加载后迁移到 chats）
    #[serde(default)]
    pub chats: Vec<ChatSegment>,       // 新格式
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub metadata: SessionMetadata,
    #[serde(default)]
    pub tasks: Option<TaskSnapshot>,
    #[serde(default)]
    pub workspace: Option<PersistedWorkspaceContext>,
}
```

- [ ] **Step 4: Session::new 初始化 chats 为空 Vec**

`Session::new` 中 `chats: Vec::new()`。

- [ ] **Step 5: 更新 Session::summary / display_title 适配**

`summary()` 方法查找第一条 user 消息时，优先从 `chats` 提取（如非空），否则回退到 `messages`。

### Task 3: load/save 适配 + 向后兼容迁移

**Files:**
- Modify: `agent/features/runtime/src/business/session/storage.rs`

- [ ] **Step 6: load_session 加迁移逻辑**

`load_session` 成功反序列化后：
- 若 `chats` 为空且 `messages` 非空（旧格式）：把 `messages` 包装为单个 `ChatSegment::normal(None)`，存入 `chats`。
- 清空旧 `messages`（已迁移）。

- [ ] **Step 7: save_session 序列化 chats**

确保 `chats` 字段正确序列化。旧 `messages` 字段写入空数组。

### Task 4: trait_session 适配

**Files:**
- Modify: `agent/features/runtime/src/core/client/trait_session.rs`

- [ ] **Step 8: save_current_session_impl 适配**

读取 `current_messages` + `current_chat_chain`（新增锁字段），写入 `session.chats`。

- [ ] **Step 9: load_session_impl 返回活跃链 messages**

`ChatChain::from_chats(&chats)` 提取活跃链，返回 `chat_chain.messages()`。同时把 `active_summary` 存入 `AgentClientImpl` 供 system 注入。

---

## Phase 2: compact 流程重构

### Task 5: compact_messages_with_llm 返回 summary + tail

**Files:**
- Modify: `agent/features/runtime/src/business/compact/summary.rs`

- [ ] **Step 10: 定义 CompactResult 并修改返回类型**

```rust
pub struct CompactResult {
    pub summary: String,
    pub recent_messages: Vec<Message>,
}
```

`compact_messages_with_llm` 返回 `Option<CompactResult>`。

- [ ] **Step 11: 移除 summary 注入 user 消息**

LLM 生成 summary 文本；recent tail = `messages[split_point..]`；返回 `CompactResult`，不再注入 messages。

- [ ] **Step 12: 删除 summary.rs 中的 microcompact 调用**

`compact_messages` 和 `compact_messages_with_llm` 中的 `microcompact` 预处理全部删除。

### Task 6: auto_compact 返回 CompactOutcome

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/compact.rs`

- [ ] **Step 13: 定义 CompactOutcome**

```rust
pub(crate) struct CompactOutcome {
    pub summary: String,
    pub messages: Vec<Message>,
}
```

- [ ] **Step 14: auto_compact 返回 Option<CompactOutcome>**

删除 microcompact 预处理；should_compact 为真 → 直接 `compact_messages_with_llm` → 返回 `CompactOutcome`。

### Task 7: loop_runner 适配 compact + summary 注入

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/loop_runner.rs`

- [ ] **Step 15: ChatLoopContext 增加 active_summary 字段**

- [ ] **Step 16: compact 后替换 messages + 设置 summary**

`auto_compact` 返回 `CompactOutcome` 时：`messages = outcome.messages`，`active_summary = Some(outcome.summary)`。

- [ ] **Step 17: summary 注入 system_blocks**

构建 API 请求前，若 `active_summary` 存在，拼入 `system_blocks`：

```rust
let mut effective_system_blocks = system_blocks.clone();
if let Some(ref summary) = active_summary {
    effective_system_blocks.push(provider::api::SystemBlock {
        text: format!("<compact-summary>\n{summary}\n</compact-summary>"),
        cache_control: None,
    });
}
```

- [ ] **Step 18: 新 user 消息时调用 start_new_segment**

---

## Phase 3: 删除 microcompact

### Task 8: 删除 microcompact 调用与定义

**Files:**
- Modify: `agent/features/runtime/src/business/compact.rs`
- Modify: `agent/features/runtime/src/business/compact/micro.rs`
- Modify: `agent/features/runtime/src/business/agent/runner/loop_helpers.rs`

- [ ] **Step 19: 删除 compact.rs 中的 microcompact re-export**

移除 `pub use micro::microcompact;` 和 `pub mod micro;`。

- [ ] **Step 20: 删除 loop_helpers.rs 中的 microcompact 调用**

`loop_helpers.rs:94` 删除子代理循环中的 microcompact 调用。

- [ ] **Step 21: 更新 compact.rs 模块文档**

移除第 2 层（microcompact）描述。

---

## Phase 4: resume 不 compact

### Task 9: resume 路径跳过 compact

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/compact.rs`

- [ ] **Step 22: auto_compact 首 turn 跳过**

resume 后首轮（`turn_count == 1 && last_api_input_tokens == 0`）直接返回 None。

- [ ] **Step 23: ChatChain.from_chats 天然跳过旧链**

已在 Task 1 实现。

---

## Phase 5: 验证

### Task 10: 编译与测试

- [ ] **Step 24: cargo build**
- [ ] **Step 25: cargo clippy --all-targets -- -D warnings**
- [ ] **Step 26: cargo test（更新因数据模型变更导致的测试失败）**
- [ ] **Step 27: 手动验证 resume 场景**
- [ ] **Step 28: 手动验证向后兼容**

---

## 风险与注意事项

1. **SDK 类型变更**：优先用 `AgentClientImpl` 内部字段避免改 SDK 契约。
2. **user_context 注入**：确认 CLAUDE.md 注入与 summary 走 system 不冲突。
3. **ChatChain 与 messages 同步**：loop 直接操作 messages，持久化时用边界信息切分。
4. **compact_hook 语义**：`messages_after` 现在是 recent tail，需更新 hook data。
