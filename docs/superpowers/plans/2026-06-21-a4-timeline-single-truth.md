# #390 A4 — timeline 单一真相 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 删除 `ConversationModel.blocks`（legacy 平行表示）及 `assemble_legacy_conversation_blocks` fallback，让 `view_assembler` 只从 `timeline` 组装，工具结果字段归一到 `ChatTurn.tool_calls[].result`。**行为等价**（屏幕产出不变）。

**Architecture:** 现状 `ConversationModel` 双写 `blocks` + `timeline`（~98% 锁步），但 timeline 的 ToolCall/ToolResult 仅是引用，工具结果字段（output/content/is_error/image_count）只在 blocks。先把这些字段搬到 `ChatTurn.tool_calls[].result`（方案 C），再把所有 blocks 读取点迁到 timeline/chats，最后删 blocks。

**Tech Stack:** Rust，ratatui TUI（TEA `ConversationModel`：blocks + timeline 双投影 + chats.turns.tool_calls）。

## Global Constraints
- **MUST** 行为等价：每子任务后 rendering / 三路一致测试（`view_assembler/output_tests.rs`、`render/output/document_renderer/tests.rs`）保持绿，屏幕产出不变。
- **MUST** 单一真相：完成后 display 只读 `timeline`（+ `chats.turns.tool_calls`），不再读 `ConversationModel.blocks`。
- **MUST** 工具结果字段归一一处：`ToolResultPayload`（output/content/is_error/image_count）只存 `ChatTurn.tool_calls[].result`，`find_tool_result_*` 单一入口。
- **NEVER** 改 wire / 持久化 JSON 格式；**NEVER** 改 view 层 ToolCall 嵌套/gutter 渲染逻辑（不依赖 blocks 树）。
- 依赖序：A4.1 → A4.2 →（A4.3 ∥ A4.4）→ A4.5 → A4.6。**A4.6（删 blocks）MUST 最后**。
- 验证门禁：`cargo clippy --all-targets --all-features`(0/0)、`cargo test --workspace`、`bash .agents/hooks/check-architecture-guards.sh`。cargo 前先 `source .cargo/set-target.sh`。

## File Structure（A4 触及）
| 文件 | 责任 / A4 改动 |
|---|---|
| `model/conversation/tool_call.rs` | `ToolCall` 加 `result: Option<ToolResultPayload>`；新增 `ToolResultPayload` 结构 |
| `model/conversation/chat_turn.rs` | `ChatTurn.tool_calls` 容器（A4.1 经此写/读 result） |
| `model/conversation/tool_flow.rs` | `observe_tool_result` 写 `tool_calls[].result`；位置查询改 timeline（A4.3） |
| `model/conversation/tool_order.rs` | 工具插入/重排的 `blocks.iter().position()` 改 timeline（A4.3） |
| `model/conversation/ask_user.rs` | ask_user 状态读写改 timeline-first（A4.4） |
| `model/conversation/model.rs` | 各 mutation 删 blocks 写入（A4.6）；agent_progress 推 timeline（A4.2） |
| `view_assembler/output.rs` | `find_tool_result_block`→`find_tool_result_in_turn`（A4.1）；删 legacy fallback（A4.5） |
| `model/conversation/block.rs` | `ConversationBlock` 枚举保留（类型/文档），模型不再实例化（A4.6） |

---

### Task A4.1：工具结果字段迁移到 `ChatTurn.tool_calls[].result`

**Files:**
- `apps/cli/src/tui/model/conversation/tool_call.rs:5`（加 `result` + `ToolResultPayload`）
- `apps/cli/src/tui/model/conversation/tool_flow.rs:59`（`observe_tool_result` 写 result）
- `apps/cli/src/tui/view_assembler/output.rs:480`（`find_tool_result_block`→`find_tool_result_in_turn`，从 chats 取）
- Test: 上述文件 tests + `view_assembler/output_tests.rs`

**Interfaces:**
- Produces:
```rust
// tool_call.rs
#[derive(Clone, Debug, PartialEq)]
pub struct ToolResultPayload {
    pub output: String,
    pub content: serde_json::Value,
    pub is_error: bool,
    pub image_count: usize,
}
// ToolCall 加字段：pub result: Option<ToolResultPayload>,  (默认 None)
```
  `fn find_tool_result_in_turn<'a>(chats: &'a [Chat], chat_id, turn_id, tool_id) -> Option<&'a ToolResultPayload>`（签名以现有 `find_tool_call`/`find_tool_view` 的 chats 遍历为准）。

- [ ] **Step 1: 失败测试** — `observe_tool_result` 处理一条工具结果后，对应 `ChatTurn.tool_calls[i].result` 为 `Some(ToolResultPayload{ output, is_error, .. })` 且字段值正确。
- [ ] **Step 2: 确认失败** — `cargo test -p cli tool_result_payload_stored_in_turn` → FAIL（无 result 字段）。
- [ ] **Step 3: 实现** — tool_call.rs 加 `ToolResultPayload` + `result` 字段（所有 `ToolCall` 构造点补 `result: None`）；`observe_tool_result`（tool_flow.rs）在写 blocks/timeline 的同时，把 `tool_calls[i].result = Some(payload)`；`view_assembler/output.rs` 新增 `find_tool_result_in_turn`（从 chats.turns.tool_calls[].result 取），`find_tool_view`（:403）改调它。**本 Task 暂保留** blocks `ToolResult` 写入与 `find_tool_result_block`（A4.5/A4.6 才删），确保等价。
- [ ] **Step 4: 确认通过** — `cargo test -p cli tool_result_payload_stored_in_turn` + `cargo test -p cli`（rendering tests 仍绿）→ PASS。
- [ ] **Step 5: 提交** — `git commit -m "feat(cli): 工具结果字段迁移到 ChatTurn.tool_calls[].result (#390 A4.1)"`

---

### Task A4.2：timeline 完整化（mutation 同步 + 镜像一致）

**Files:**
- `apps/cli/src/tui/model/conversation/model.rs`（`record_agent_progress` 推 timeline AgentProgress item）
- `apps/cli/src/tui/model/conversation/tool_flow.rs` / `tool_order.rs`（确认 tool mutation 都已推 timeline ref）
- Test: `model_tests.rs`（镜像一致断言）

**Interfaces:**
- Consumes: A4.1 的 `result`。
- Produces: 所有产生 block 的 mutation 都同时产生等价 timeline item（含 AgentProgress）。

- [ ] **Step 1: 失败测试** — 一个含 user/assistant/tool-call/tool-result/agent-progress 的回合序列后，`timeline.items()` 的种类与顺序 == 对应 blocks 的种类与顺序（逐项对照，AgentProgress 也在 timeline）。
- [ ] **Step 2: 确认失败** — `cargo test -p cli timeline_mirrors_blocks` → FAIL（agent_progress 不在 timeline）。
- [ ] **Step 3: 实现** — `record_agent_progress`（model.rs）在更新 `tool_calls[].activities` 的同时 `timeline.push(OutputTimelineItem::AgentProgress{..})`（若尚未）；核查并补齐任何只写 blocks 未写 timeline 的 mutation。
- [ ] **Step 4: 确认通过** — `cargo test -p cli timeline_mirrors_blocks` + `cargo test -p cli` → PASS。
- [ ] **Step 5: 提交** — `git commit -m "feat(cli): timeline 完整化（agent_progress 等 mutation 同步）(#390 A4.2)"`

---

### Task A4.3：工具位置查询改造（blocks → timeline）

**Files:**
- `apps/cli/src/tui/model/conversation/tool_order.rs:14/61/100`
- `apps/cli/src/tui/model/conversation/tool_flow.rs:16`
- `apps/cli/src/tui/model/conversation/model.rs:301`（`observe_tool_call_update` 的 `blocks.iter().position`）
- Test: `model_tests.rs` tool ordering 用例

**Interfaces:**
- Consumes: timeline 现有 `OutputTimelineItem::{ToolCall,ToolResult,OrphanToolResult}` + 现有 `move_tool_result_after_tool_call`。
- Produces: tool 插入/重排/孤儿提升的位置判断改读 `timeline.items()`（或 chats），不读 `blocks`。

- [ ] **Step 1: 失败测试**（先确认现有 tool ordering 测试覆盖：tool-call 去重插入、tool-result 排在对应 call 后、孤儿提升）——若覆盖不足先补，作为等价基线。
- [ ] **Step 2: 改造** — 逐处把 `self.blocks.iter().position(...)` / `.any(...)` 改为对 `timeline.items()` 的等价查询（必要时给 `OutputTimelineModel` 加 `find_tool_call_index`/`contains_tool_call` 等只读辅助）。blocks 写入暂留（A4.6 删）。
- [ ] **Step 3: 确认通过** — `cargo test -p cli`（tool ordering + rendering 全绿）→ PASS。
- [ ] **Step 4: 提交** — `git commit -m "refactor(cli): 工具位置查询改读 timeline (#390 A4.3)"`

---

### Task A4.4：ask_user 改 timeline-first

**Files:**
- `apps/cli/src/tui/model/conversation/ask_user.rs:37/277/326/343`
- `apps/cli/src/tui/model/conversation/ask_user_timeline.rs`
- Test: ask_user 交互测试（`ask_user*tests` / `app/update/ask_user_key.rs` 相关）

**Interfaces:**
- Produces: ask_user 快照/文本读取与状态修改（导航/勾选/自由输入）直接读写 `timeline` 的 `AskUserBatch` item；删除 blocks-first 的 `sync_ask_user_timeline_item`（同步函数）。

- [ ] **Step 1: 确认基线测试** — ask_user 导航/勾选/自由输入/dismiss 的现有测试覆盖（不足则补），作等价基线。
- [ ] **Step 2: 改造** — `ask_user_snapshot`/`ask_user_chat_text` 改读 timeline AskUserBatch item；`answer_current_ask_user`/`navigate_ask_user_to`/`show_ask_user_batch`/`remove_ask_user_block` 改为直接改 timeline item；删 `sync_ask_user_timeline_item`（blocks→timeline 同步不再需要）。blocks 写入暂留（A4.6 删）。
- [ ] **Step 3: 确认通过** — `cargo test -p cli`（ask_user 全绿 + rendering 等价）→ PASS。
- [ ] **Step 4: 提交** — `git commit -m "refactor(cli): ask_user 状态改 timeline-first (#390 A4.4)"`

---

### Task A4.5：渲染 timeline-only（删 legacy fallback）

**Files:**
- `apps/cli/src/tui/view_assembler/output.rs:24`（删 `if timeline.is_empty() → assemble_legacy_conversation_blocks`）
- `apps/cli/src/tui/view_assembler/output.rs:460/480`（删 `assemble_legacy_conversation_blocks` + `find_tool_result_block`，统一用 A4.1 的 `find_tool_result_in_turn`）
- Test: `view_assembler/output_tests.rs`、`document_renderer/tests.rs`

**Interfaces:**
- Consumes: A4.1 `find_tool_result_in_turn`、A4.2 完整 timeline。

- [ ] **Step 1: 失败/基线测试** — 确认 rendering 测试在「timeline 驱动、无 blocks fallback」下仍产出等价 view 树（不足则补一个覆盖 tool-result 显示的渲染测试）。
- [ ] **Step 2: 改造** — 删 `assemble_from_conversation` 里的 timeline-empty fallback 分支 + `assemble_legacy_conversation_blocks` 函数 + `find_tool_result_block`（其调用点 :103/:354 改用 `find_tool_result_in_turn`）。
- [ ] **Step 3: 确认通过** — `cargo test -p cli`（全部 rendering 测试绿，产出等价）→ PASS。
- [ ] **Step 4: 提交** — `git commit -m "refactor(cli): view_assembler 仅从 timeline 组装，删 legacy fallback (#390 A4.5)"`

---

### Task A4.6：删除 `ConversationModel.blocks`

**Files:**
- `apps/cli/src/tui/model/conversation/model.rs:18`（删字段）+ 所有 `self.blocks.push/retain/insert/iter` 写入点
- 各 mutation（user/assistant/thinking/queued/tool/notice/ask_user）删 blocks 写入分支
- `block.rs`：`ConversationBlock` 枚举**保留**（类型/文档），模型不再实例化
- Test: 全套件

**Interfaces:**
- 前置：A4.1-A4.5 已把所有 blocks 读取点迁走。

- [ ] **Step 1: 确认无 blocks 读取点** — `grep -rn "\.blocks\b" apps/cli/src --include='*.rs' | grep -v _tests` 应仅剩待删的写入点（读取点已在前序迁走）；列出核对。
- [ ] **Step 2: 删除** — 删 `pub blocks` 字段 + 所有写入；删因此变死的 mutation 辅助（如纯 blocks 的 retain）。删/改依赖 blocks 的旧测试（断言改对 timeline / view model）。
- [ ] **Step 3: 验证** — `cargo build --workspace` + `cargo test -p cli` + `cargo clippy --all-targets`（0 warning，无 unused）→ PASS。
- [ ] **Step 4: 提交** — `git commit -m "refactor(cli): 删除 ConversationModel.blocks，timeline 成单一真相 (#390 A4.6)"`

---

### Task A4.7：门禁 + PR

- [ ] **Step 1-3: 门禁** — `cargo clippy --all-targets --all-features`(0/0) + `cargo test --workspace`(全绿) + `bash .agents/hooks/check-architecture-guards.sh`(全过)。
- [ ] **Step 4: 同步 main** — `git fetch origin main` → `git merge refs/remotes/origin/main`（冲突解决后重跑门禁）。
- [ ] **Step 5: PR** — push + `gh pr create`（base main），正文含目标/方案 C/改动/门禁/「行为等价」说明/「关联 #390 A4」+ 提示用户 TUI 验收（产出应与现状一致）。**NEVER 自动合并**。

## Self-review
- 设计 §3.5/§4 A4：删 blocks ✓(A4.6) / view_assembler 只读 timeline ✓(A4.5) / 工具结果字段归一 ✓(A4.1) / 行为等价（每步 rendering 测试）✓。
- 依赖序固化（A4.1 前置、A4.6 最后）✓。
- 类型一致：`ToolResultPayload` / `find_tool_result_in_turn` 全程同名 ✓。
- 风险：A4.1（字段迁移）+ A4.4（ask_user timeline-first）为高/中风险，已置于序列前部、各有等价基线测试 + 最终 opus 终审 + 用户 TUI 验收兜底。
