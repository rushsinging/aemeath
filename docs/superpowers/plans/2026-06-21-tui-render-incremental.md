# TUI 渲染增量化（B 阶段）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 active（streaming / tool 运行）大会话的单帧 `refresh_output_document_from_model` 成本 ∝ 变化量而非会话总量。

**Architecture:** B1 把 `assemble_from_conversation` 的工具查找从 O(n²) 线性扫描改为一次性索引（O(n) 建、O(1) 查）。B2 把 `RenderedBlock.lines` 改为 `Rc<Vec<RenderedLine>>` 让 clone 退化为 `Rc::clone`，并在 `OutputDocumentRenderer` 增加「带 gutter 的 block」缓存，使 `render_tree` 复用未变 block、只重算变化 block 与运行中 tool（0–1 个）。

**Tech Stack:** Rust / ratatui。crate：`cli`。设计依据：`docs/superpowers/specs/2026-06-21-tui-render-incremental-design.md`。

## Global Constraints

- 所有改动在 worktree `worktree-fix-425-tui-rerender`，NEVER 直接改 main。
- **MUST** TDD：先红后绿，断言不为迁就实现而削弱。
- **MUST** 行为等价：B1/B2 不改任何渲染输出。现有 `output_tests.rs` / `output_unit_tests.rs` / `output_task_tests.rs` / `render_tests.rs` / `selection_tests.rs` 全绿。
- 错误/提示消息中文；**MUST NOT** 手动调格式（`cargo fmt`）。
- 门禁：`cargo test -p cli` + `cargo clippy -p cli --tests` 全绿、零新 warning。
- baseline：当前 worktree `cargo test -p cli` = 893 passed。每个 task 结束保持全绿。
- 性能基准：`cargo test -p cli --release bench_refresh_cost_by_conversation_size -- --ignored --nocapture`（经 `rtk proxy` 看 stdout：`rtk proxy cargo test ...`）。

---

## File Structure

- `apps/cli/src/tui/view_assembler/output.rs` — B1：新增 `ToolIndex`，`assemble_from_conversation` 开头构建，`find_tool_*` 改签名收 `&ToolIndex`。
- `apps/cli/src/tui/render/output/rendered.rs` — B2：`RenderedBlock.lines: Rc<Vec<RenderedLine>>` + `with_line_fill_style` 用 `Rc::make_mut`。
- `apps/cli/src/tui/render/output/document_renderer.rs` — B2：gutted 缓存 + `render_node` 增量复用。
- `apps/cli/src/tui/render/output/blocks/*.rs`、`block_component.rs`、`output_area.rs`、`block_cache.rs` 等 — B2：`RenderedBlock { lines: vec![..] }` 构造点机械改 `Rc::new(vec![..])`（编译驱动）。

---

## Phase B1 — assemble 工具查找索引化

### Task B1: ToolIndex 索引化 find_tool_*

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/output.rs`（新增 `ToolIndex`；改 `assemble_from_conversation`、`find_tool_call`、`find_tool_view`、`find_tool_name_by_id`、`find_tool_result_block`、`tool_result_is_embedded`）
- Test: `apps/cli/src/tui/view_assembler/output_unit_tests.rs`（新增索引正确性单测）

**Interfaces:**
- Consumes: `ConversationModel.chats: Vec<Chat>`（`chat.turns: Vec<ChatTurn>`，`turn.tool_calls: Vec<ToolCall>`，`ToolCall.id: Option<ToolCallId>`、`ToolCall.name`、`ToolCall.status`、`ToolCall.result`）；`ConversationModel.blocks: Vec<ConversationBlock>`（`ConversationBlock::ToolResult { id, chat_id, turn_id, output, content, is_error, image_count }`）。
- Produces: `struct ToolIndex<'a>`，`fn ToolIndex::build(&'a ConversationModel) -> ToolIndex<'a>`；查询方法 `call(&self, chat, turn, tool) -> Option<&'a ToolCall>`、`result_block(&self, chat, turn, tool) -> Option<(&'a str, &'a serde_json::Value, bool, usize)>`。

- [ ] **Step 1: 写失败测试（索引与线性扫描等价）**

在 `apps/cli/src/tui/view_assembler/output_unit_tests.rs` 末尾追加（该文件 `use super::*;` 可见模块内类型；若缺 import 则按编译错补）：

```rust
#[test]
fn test_tool_index_call_matches_linear_scan() {
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};
    use crate::tui::model::conversation::intent::ConversationIntent;
    use crate::tui::model::conversation::model::ConversationModel;

    let mut conv = ConversationModel::default();
    let chat = ChatId::new("c1");
    let turn = ChatTurnId::new("t1");
    let tool = ToolCallId::new("tool-1");
    conv.apply(ConversationIntent::ObserveToolCallStart {
        chat_id: chat.clone(),
        turn_id: turn.clone(),
        id: tool.clone(),
        provider_id: Some("p".to_string()),
        name: "Read".to_string(),
        index: 0,
    });

    let index = ToolIndex::build(&conv);
    let via_index = index.call(&chat, &turn, &tool).map(|c| c.name.clone());
    assert_eq!(via_index.as_deref(), Some("Read"), "索引应命中已登记 tool");
    assert!(
        index.call(&chat, &turn, &ToolCallId::new("missing")).is_none(),
        "未登记 tool 应返回 None"
    );
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p cli test_tool_index_call_matches_linear_scan`
Expected: FAIL（`ToolIndex` 未定义，编译错误）。

- [ ] **Step 3: 实现 ToolIndex 并改造 find_***

在 `output.rs` 顶部 `use` 区加 `use std::collections::HashMap;`。在文件内（`OutputViewAssembler` impl 之前或 helper 区）新增：

```rust
/// assemble 期一次性构建的工具查找索引，把 O(n²) 线性扫描降为 O(1)。
pub(super) struct ToolIndex<'a> {
    calls: HashMap<(&'a ChatId, &'a ChatTurnId, &'a ToolCallId), &'a ToolCall>,
    results: HashMap<
        (&'a ChatId, &'a ChatTurnId, &'a ToolCallId),
        (&'a str, &'a serde_json::Value, bool, usize),
    >,
}

impl<'a> ToolIndex<'a> {
    pub(super) fn build(conversation: &'a ConversationModel) -> Self {
        let mut calls = HashMap::new();
        for chat in &conversation.chats {
            for turn in &chat.turns {
                for call in &turn.tool_calls {
                    if let Some(id) = call.id.as_ref() {
                        calls.insert((&chat.id, &turn.id, id), call);
                    }
                }
            }
        }
        let mut results = HashMap::new();
        for block in &conversation.blocks {
            if let crate::tui::model::conversation::block::ConversationBlock::ToolResult {
                id,
                chat_id,
                turn_id,
                output,
                content,
                is_error,
                image_count,
            } = block
            {
                results.insert(
                    (chat_id, turn_id, id),
                    (output.as_str(), content, *is_error, *image_count),
                );
            }
        }
        Self { calls, results }
    }

    pub(super) fn call(
        &self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
        tool_id: &ToolCallId,
    ) -> Option<&'a ToolCall> {
        self.calls.get(&(chat_id, turn_id, tool_id)).copied()
    }

    pub(super) fn result_block(
        &self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
        tool_id: &ToolCallId,
    ) -> Option<(&'a str, &'a serde_json::Value, bool, usize)> {
        self.results.get(&(chat_id, turn_id, tool_id)).copied()
    }
}
```

改 `assemble_from_conversation`（output.rs:17）：在函数开头构建一次索引，并把它传入各 helper。在 `let mut roots ...` 后加 `let tool_index = ToolIndex::build(conversation);`，并把 `find_tool_view(conversation, ...)`、`tool_result_is_embedded(conversation, ...)`、`find_tool_name_by_id(conversation, ...)`、`find_tool_result_block(conversation, ...)` 的调用改为传 `&tool_index` 替换 `conversation`（保留 `conversation` 仅在仍需遍历 timeline 的地方）。

改各 helper 签名与实现，用索引替换线性扫描：

```rust
fn find_tool_call<'a>(
    index: &ToolIndex<'a>,
    chat_id: &ChatId,
    turn_id: &ChatTurnId,
    tool_id: &ToolCallId,
) -> Option<&'a ToolCall> {
    index.call(chat_id, turn_id, tool_id)
}

fn find_tool_result_block<'a>(
    index: &ToolIndex<'a>,
    chat_id: &ChatId,
    turn_id: &ChatTurnId,
    tool_id: &ToolCallId,
) -> Option<(&'a str, &'a serde_json::Value, bool, usize)> {
    index.result_block(chat_id, turn_id, tool_id)
}
```

`find_tool_view` / `find_tool_name_by_id` / `tool_result_is_embedded` 改为接收 `&ToolIndex` 并内部调 `find_tool_call` / `find_tool_result_block`（把它们原先的 `conversation: &ConversationModel` 形参替换为 `index: &ToolIndex`）。`find_tool_view` 内对 `find_tool_result_block(conversation, ...)` 的调用同步改为 `find_tool_result_block(index, ...)`。

- [ ] **Step 4: 运行确认通过 + 行为等价回归**

Run: `cargo test -p cli` 
Expected: 894 passed（893 + 新增 1），0 failed。现有 assemble 测试全绿即证行为等价。

- [ ] **Step 5: bench 验证 assemble 转线性 + clippy + 提交**

Run: `rtk proxy cargo test -p cli --release bench_refresh_cost_by_conversation_size -- --ignored --nocapture`
Expected: `assemble` 列在 500→4000 turns 大致线性（不再 ~35×），4000turns 从 ~33ms 降到个位数 ms。

Run: `cargo clippy -p cli --tests`，零 warning。

```bash
git add apps/cli/src/tui/view_assembler/output.rs apps/cli/src/tui/view_assembler/output_unit_tests.rs
git commit -m "perf(tui): assemble 工具查找索引化，O(n^2)→O(n)（#425 B1）"
```

---

## Phase B2 — render_tree 增量（Rc 化 + gutted 缓存）

### Task B2a: RenderedBlock.lines 改 Rc<Vec<RenderedLine>>

**Files:**
- Modify: `apps/cli/src/tui/render/output/rendered.rs:111,116-121`
- Modify（编译驱动机械改）：所有 `RenderedBlock { ... lines: vec![..] }` 构造点 —— 已知文件：`render/output/output_area.rs`、`block_component.rs`、`block_cache.rs`、`blocks/{tool_result,tool_call,user_message,ask_user,separator,assistant_message,diagnostic,thinking}.rs`、`document_renderer.rs`、`render/output/selection_tests.rs`、`rendered.rs`（测试）
- Test: 复用现有 `rendered.rs` 单测（`test_rendered_document_total_lines_sums_blocks` 等）

**Interfaces:**
- Produces: `RenderedBlock.lines: std::rc::Rc<Vec<RenderedLine>>`。读访问（`.iter()` / `.len()` / `.is_empty()`）经 `Deref` 不变；写访问改 `Rc::make_mut`。

- [ ] **Step 1: 改类型并修 with_line_fill_style**

`rendered.rs`：`use std::rc::Rc;`（顶部）。`RenderedBlock`：

```rust
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderedBlock {
    pub block_id: String,
    pub lines: Rc<Vec<RenderedLine>>,
}

impl RenderedBlock {
    pub fn with_line_fill_style(mut self, style: Style) -> Self {
        for line in Rc::make_mut(&mut self.lines) {
            line.set_fill_style(style);
        }
        self
    }
}
```

> `total_lines` / `iter_lines`（rendered.rs:131-137）不改：`block.lines.len()` / `block.lines.iter()` 经 `Rc` Deref 自动可用。

- [ ] **Step 2: 编译，按 rustc 错误逐个修构造点**

Run: `cargo build -p cli --tests`
Expected: 一批 E0308「expected `Rc<Vec<RenderedLine>>`, found `Vec<...>`」。对每个报错的 `RenderedBlock { lines: <expr> }`，把 `<expr>` 包成 `Rc::new(<expr>)`（在该文件加 `use std::rc::Rc;`）。这是纯机械替换，无逻辑变化。重复 build 直到通过。

> `document_renderer.rs:114-117` 的 `out.push(RenderedBlock { block_id, lines: gutted })` 改 `lines: Rc::new(gutted)`；`with_line_fill_style` 调用链（`block_component.rs` 等）已由 Step 1 覆盖。

- [ ] **Step 3: 全量回归 + clippy + 提交**

Run: `cargo test -p cli` → 893 passed（数量不变，纯机械重构）。
Run: `cargo clippy -p cli --tests` → 零 warning。

```bash
git add apps/cli/src/tui/render/
git commit -m "refactor(tui): RenderedBlock.lines 改 Rc<Vec> 使 clone 廉价（#425 B2a）"
```

### Task B2b: gutted block 缓存 + render_node 增量复用

**Files:**
- Modify: `apps/cli/src/tui/render/output/document_renderer.rs`（`OutputDocumentRenderer` 加 gutted 缓存；`render_node` 命中复用）
- Test: `apps/cli/src/tui/render/output/document_renderer.rs` 的 `#[cfg(test)] mod tests`（若无则新建）

**Interfaces:**
- Consumes: `RenderedBlock`（lines 已 `Rc`，B2a）；`animated_marker_glyph`/`apply_gutter_with_frame`（gutter.rs）；`BlockNode { block_id, block_version, kind, children }`；`OutputBlockKind::ToolCall(t)` 的 `t.semantic_status == ToolSemanticStatus::Running`。
- Produces: `OutputDocumentRenderer` 私有字段 `gutted: HashMap<String, (GuttedKey, RenderedBlock)>`；`#[cfg(test)]` 探针 `gutted_render_count`。

- [ ] **Step 1: 写失败测试（静态 block 跨 frame 命中、运行中 tool 每帧失效）**

在 `document_renderer.rs` 的 tests mod 追加（构造一个 view_model：1 个 AssistantMessage（静态）+ 1 个 Running ToolCall；连续两帧 render，断言 gutted 重算次数）：

```rust
#[test]
fn test_gutted_cache_reuses_static_block_across_frames() {
    use crate::tui::view_model::output::{BlockNode, OutputBlockKind, OutputViewModel, TextBlockView, SemanticStyle};
    let node = BlockNode {
        block_id: "a".to_string(),
        block_version: 1,
        kind: OutputBlockKind::AssistantMessage(TextBlockView {
            key: "a".to_string(),
            text: "静态文本".to_string(),
            style: SemanticStyle::Normal,
        }),
        children: Vec::new(),
    };
    let vm = OutputViewModel { roots: vec![node] };
    let mut r = OutputDocumentRenderer::default();
    let _ = r.render_model_document(&vm, 80, 80, 0);
    let after_first = r.gutted_render_count();
    // 同一 vm、frame 推进：静态 block 应命中 gutted 缓存，不重算。
    let _ = r.render_model_document(&vm, 80, 80, 1);
    assert_eq!(
        r.gutted_render_count(),
        after_first,
        "静态 block 跨 frame 应复用 gutted 缓存"
    );
}
```

> 若 `BlockNode` / `TextBlockView` 字段名与上不符，以实际定义为准（`view_model/output.rs`）。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p cli test_gutted_cache_reuses_static_block_across_frames`
Expected: FAIL（`gutted_render_count` 不存在，编译错误）。

- [ ] **Step 3: 实现 gutted 缓存与增量复用**

`document_renderer.rs`：`use std::collections::HashMap;`。`OutputDocumentRenderer` 加字段：

```rust
#[derive(Default)]
pub struct OutputDocumentRenderer {
    cache: BlockCache,
    gutted: HashMap<String, (GuttedKey, RenderedBlock)>,
    #[cfg(test)]
    render_count: std::cell::Cell<usize>,
    #[cfg(test)]
    gutted_render_count: std::cell::Cell<usize>,
}

#[derive(PartialEq, Eq, Clone)]
struct GuttedKey {
    block_version: u64,
    text_width: u16,
    depth: usize,
    marker_frame: Option<u64>,
}
```

`render_node`（document_renderer.rs:69-121）改为：先算 `marker_frame`（仅运行中 ToolCall 为 `Some`），组 `GuttedKey`；命中则 `out.push(cached.clone())`（`lines` 为 `Rc`，clone 廉价）并递归 children 后返回；未命中走现有 `render_self`+`apply_gutter`+组装，存入 `gutted` 再 push。

```rust
fn render_node(&mut self, node: &BlockNode, outer_width: u16, depth: usize, animation_frame: u64, out: &mut Vec<RenderedBlock>) {
    let text_width = gutter::effective_block_width(outer_width, depth);
    let marker_frame = match &node.kind {
        crate::tui::view_model::output::OutputBlockKind::ToolCall(t)
            if t.semantic_status == crate::tui::view_model::ToolSemanticStatus::Running =>
        {
            Some(animation_frame / crate::tui::render::output::gutter::TOOL_MARKER_BLINK_DIVISOR)
        }
        _ => None,
    };
    let gkey = GuttedKey { block_version: node.block_version, text_width, depth, marker_frame };
    if let Some((cached_key, cached_block)) = self.gutted.get(&node.block_id) {
        if *cached_key == gkey {
            out.push(cached_block.clone());
            for child in &node.children {
                self.render_node(child, outer_width, depth + 1, animation_frame, out);
            }
            return;
        }
    }
    #[cfg(test)]
    self.gutted_render_count.set(self.gutted_render_count.get() + 1);
    // —— 以下为现有 render_self + gutter 组装逻辑（保持不变，末尾 lines 包 Rc）——
    let key = CacheKey { version: node.block_version, text_width };
    let mut rendered = self.cache.get_or_render(&node.block_id, key, |ctx| {
        #[cfg(test)]
        self.render_count.set(self.render_count.get() + 1);
        node.kind.component().render_self(&node.block_id, ctx)
    });
    if matches!(node.kind, crate::tui::view_model::output::OutputBlockKind::UserMessage(_)) {
        rendered = rendered.with_line_fill_style(Style::default().bg(theme::USER_BG));
    }
    let mut gutted = crate::tui::render::output::gutter::apply_gutter_with_frame(
        &node.kind, depth, (*rendered.lines).clone(), animation_frame,
    );
    if matches!(node.kind, crate::tui::view_model::output::OutputBlockKind::UserMessage(_)) {
        wrap_user_message_card_lines(&mut gutted);
    }
    if depth == 0 {
        gutted.insert(0, RenderedLine::default());
    }
    let block = RenderedBlock { block_id: rendered.block_id, lines: std::rc::Rc::new(gutted) };
    self.gutted.insert(node.block_id.clone(), (gkey, block.clone()));
    out.push(block);
    for child in &node.children {
        self.render_node(child, outer_width, depth + 1, animation_frame, out);
    }
}

#[cfg(test)]
pub fn gutted_render_count(&self) -> usize {
    self.gutted_render_count.get()
}
```

> 注：`apply_gutter_with_frame` 仍要 owned `Vec`，故传 `(*rendered.lines).clone()`（解 `Rc`）——仅未命中路径付此 clone；命中路径走上面的 `Rc::clone` 零拷贝。`TOOL_MARKER_BLINK_DIVISOR` 需在 `gutter.rs` 设为 `pub`。
> 在 `render_tree_with_animation_frame` 末尾的 `self.cache.retain(&live_ids)` 旁，加 `self.gutted.retain(|id, _| live_ids.iter().any(|l| l == id));` 防 gutted 缓存泄漏。

- [ ] **Step 4: 运行确认通过 + 全量回归**

Run: `cargo test -p cli test_gutted_cache_reuses_static_block_across_frames` → PASS。
Run: `cargo test -p cli` → 894 passed（B1）+ 1 = 895，0 failed。渲染输出等价由现有 `render_tests` / `output_tests` 保证。

- [ ] **Step 5: clippy + 提交**

Run: `cargo clippy -p cli --tests` → 零 warning。

```bash
git add apps/cli/src/tui/render/output/document_renderer.rs apps/cli/src/tui/render/output/gutter.rs
git commit -m "perf(tui): gutted block 缓存使 render_tree 增量复用未变 block（#425 B2b）"
```

### Task B2c: bench 验证 render 解耦

**Files:** 无代码改动（验证）。

- [ ] **Step 1: 跑 bench 对比**

Run: `rtk proxy cargo test -p cli --release bench_refresh_cost_by_conversation_size -- --ignored --nocapture`
Expected: `render_warm`（动画 tick：BlockCache+gutted 命中）从 4000turns ~49ms 降到亚毫秒级（只运行中 tool 重算）。

- [ ] **Step 2: 记录结论到 issue 425**

把 B1+B2 前后 bench 对比（assemble 转线性、render_warm 解耦）贴入 issue 425 评论作为验证证据。

---

## Self-Review

- **Spec 覆盖**：B1 索引化 → Task B1；B2 `Rc` 化 → B2a；B2 gutted 缓存 → B2b；验证 → B2c。`不做`（视口虚拟化/增量 assemble/冷启动）已在 spec 列明，无对应 task（正确）。✓
- **Placeholder 扫描**：B2a 的机械构造点用「编译驱动」（rustc 精确导航，非逻辑 placeholder）；其余步骤含完整代码。✓
- **类型一致**：`ToolIndex`/`call`/`result_block`（B1）；`RenderedBlock.lines: Rc<Vec<RenderedLine>>`（B2a）在 B2b 的 `Rc::clone` / `(*rendered.lines).clone()` 一致；`GuttedKey` 字段在 Step3 定义与使用一致。✓
- **风险点**：B2a 构造点若有遗漏由编译器兜底；B2b 若 `BlockNode`/`TextBlockView` 字段名不符以实际定义为准（已在测试旁注明）。
