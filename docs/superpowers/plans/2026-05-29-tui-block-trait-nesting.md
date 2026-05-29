# TUI Block trait 化 + 真正渲染树 + gutter 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把扁平的 `render_block` 自由函数升为 `BlockComponent` trait + 真正的 `BlockNode` 渲染树（tool result 升为子块）+ 行首固定宽度 gutter（缩进 + 状态 marker），三者整体落地。

**Architecture:** ViewModel 从扁平 `Vec<OutputBlockView>` 升为 `Vec<BlockNode>` 树；每个 view 实现 `BlockComponent`（`cache_version` + `render_self`，产无缩进 depth=0 行）；渲染器 DFS 递归走树、组合期注入 gutter（缩进 + marker，只进 spans 不进 plain）、展平为扁平 `RenderedDocument`；选区/缓存复用 #58 机制，仅适配树与 gutter 列偏移。终端本质仍行渲染，树/gutter 为逻辑外壳。

**Tech Stack:** Rust, ratatui, 既有 `render/output/` 管线（`document_renderer`、`block_cache`、`rendered`、`primitives/fenced`、`blocks/*`）。

**前置阅读（实现前必看）：**
- spec：`docs/superpowers/specs/2026-05-29-tui-block-trait-nesting-design.md`（尤其 §0 现状修订、§5 缓存不折叠、§6 缩进不进 plain、§6.5 gutter）
- 现有类型：`apps/cli/src/tui/render/output/rendered.rs`（`RenderCtx{width}`、`RenderedLine{spans,plain}`、`RenderedBlock{block_id,lines}`、`RenderedDocument{blocks}`）
- 现有 ViewModel：`apps/cli/src/tui/view_model/output.rs`（`OutputBlockView{block_id,block_version,kind}`、`OutputBlockKind` enum、各 `*BlockView`、`ToolSemanticStatus`）
- 现有分发：`apps/cli/src/tui/render/output/blocks/mod.rs`（`render_block(kind, block_id, ctx)` match）
- 现有缓存/渲染器：`apps/cli/src/tui/render/output/block_cache.rs`、`document_renderer.rs`
- 现有 assembler：`apps/cli/src/tui/view_assembler/output.rs`（`assemble_from_conversation`、`semantic_version`、`map_tool_status`、`find_tool_view`）
- `INDENT`：`apps/cli/src/tui/render/output_area/types.rs:8`（`pub const INDENT: &str = "  ";`）

**验证门禁（每个 Task 收尾跑）：** `cargo test -p cli`、`cargo clippy -p cli`、`bash .agents/hooks/check-architecture-guards.sh`

---

## Phase 1：BlockComponent trait 落地（行为不变）

把 `render_block` 的 match 分发改为 trait 分发；`render_self` 复用现有 `render_xxx` 函数体。此阶段不动 ViewModel 结构、不动缓存键、不动渲染输出——纯重构，所有现有测试必须不改而通过。

### Task 1.1：定义 BlockComponent trait

**Files:**
- Create: `apps/cli/src/tui/render/output/block_component.rs`
- Modify: `apps/cli/src/tui/render/output/mod.rs`（加 `pub mod block_component;`）

- [ ] **Step 1: 写失败测试**

在 `block_component.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::rendered::RenderCtx;
    use crate::tui::view_model::output::{OutputBlockKind, TextBlockView};
    use crate::tui::view_model::style::SemanticStyle;

    fn ctx() -> RenderCtx {
        RenderCtx { width: 80 }
    }

    #[test]
    fn test_component_dispatch_renders_self_lines() {
        let kind = OutputBlockKind::SystemNotice(TextBlockView {
            key: "s".into(),
            text: "ok".into(),
            style: SemanticStyle::Muted,
        });
        let block = kind.component().render_self("s", &ctx());
        assert_eq!(block.block_id, "s");
        assert_eq!(block.lines[0].plain, "ok");
    }

    #[test]
    fn test_cache_version_stable_for_same_content() {
        let a = OutputBlockKind::SystemNotice(TextBlockView {
            key: "s".into(),
            text: "ok".into(),
            style: SemanticStyle::Muted,
        });
        let b = a.clone();
        assert_eq!(a.cache_version(), b.cache_version());
    }

    #[test]
    fn test_cache_version_differs_for_different_content() {
        let a = OutputBlockKind::SystemNotice(TextBlockView {
            key: "s".into(),
            text: "ok".into(),
            style: SemanticStyle::Muted,
        });
        let b = OutputBlockKind::SystemNotice(TextBlockView {
            key: "s".into(),
            text: "changed".into(),
            style: SemanticStyle::Muted,
        });
        assert_ne!(a.cache_version(), b.cache_version());
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p cli block_component 2>&1 | head -20`
Expected: 编译失败（`component`/`render_self`/`cache_version` 未定义）。

- [ ] **Step 3: 写 trait + 分发实现**

`block_component.rs` 顶部：

```rust
//! BlockComponent trait：统一 block 渲染模板与缓存指纹。
//!
//! - `render_self`：仅渲染自身内容（不含子块、不含 gutter/缩进），产 depth=0 行。
//!   不变式：每行 plain == spans 可见文本拼接。
//! - `cache_version`：自身语义指纹，作为 block 缓存 key 的 version 分量。
//! gutter（缩进 + marker）由渲染器在组合期注入，组件永不自写（见 spec §6.5）。

use crate::tui::render::output::blocks;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock};
use crate::tui::view_model::output::OutputBlockKind;
use std::hash::{Hash, Hasher};

pub trait BlockComponent {
    fn render_self(&self, block_id: &str, ctx: &RenderCtx) -> RenderedBlock;
}

impl OutputBlockKind {
    /// enum → trait 分发入口。
    pub fn component(&self) -> &dyn BlockComponent {
        self
    }

    /// 自身语义指纹（不含子）。取代 view_assembler::semantic_version。
    pub fn cache_version(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

impl BlockComponent for OutputBlockKind {
    fn render_self(&self, block_id: &str, ctx: &RenderCtx) -> RenderedBlock {
        match self {
            OutputBlockKind::AssistantMessage(text) => {
                blocks::assistant_message::render_assistant_message(block_id, text, ctx)
            }
            OutputBlockKind::ToolCall(tool) => {
                blocks::tool_call::render_tool_call(block_id, tool, ctx)
            }
            OutputBlockKind::ThinkingMessage(text) => {
                blocks::thinking::render_thinking(block_id, text, ctx)
            }
            OutputBlockKind::QueuedSubmission(text) => {
                blocks::queued_submission::render_queued_submission(block_id, text, ctx)
            }
            OutputBlockKind::UserMessage(text) => {
                blocks::user_message::render_user_message(block_id, text, ctx)
            }
            OutputBlockKind::AskUser(ask) => blocks::ask_user::render_ask_user(block_id, ask, ctx),
            OutputBlockKind::Separator => blocks::separator::render_separator(block_id),
            OutputBlockKind::SystemNotice(text) | OutputBlockKind::DiagnosticNotice(text) => {
                blocks::diagnostic::render_diagnostic(block_id, text, ctx)
            }
        }
    }
}
```

在 `render/output/mod.rs` 加 `pub mod block_component;`（若 `blocks` 子模块当前为私有，确保 `pub mod blocks;` 及各 block 子模块为 `pub`，使分发可见）。

- [ ] **Step 4: 运行验证通过**

Run: `cargo test -p cli block_component 2>&1 | tail -10`
Expected: 3 测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/render/output/block_component.rs apps/cli/src/tui/render/output/mod.rs
git commit -m "feat(tui): 引入 BlockComponent trait 与 enum 分发 (refs #60)"
```

### Task 1.2：document_renderer 改用 trait + cache_version

**Files:**
- Modify: `apps/cli/src/tui/render/output/document_renderer.rs`
- Modify: `apps/cli/src/tui/view_assembler/output.rs`（`semantic_version` 改委托 `kind.cache_version()`）

- [ ] **Step 1: 改 document_renderer 渲染闭包**

把 `document_renderer.rs` 渲染闭包内的 `render_block(&block.kind, &block.block_id, ctx)` 改为：

```rust
let rendered = self.cache.get_or_render(&block.block_id, key, |ctx| {
    #[cfg(test)]
    self.render_count.set(self.render_count.get() + 1);
    block.kind.component().render_self(&block.block_id, ctx)
});
```

顶部 import 改：删 `use crate::tui::render::output::blocks::render_block;`，加 `use crate::tui::render::output::block_component::BlockComponent;`。

- [ ] **Step 2: assembler 的 semantic_version 委托**

`view_assembler/output.rs` 的 `semantic_version`：

```rust
fn semantic_version(kind: &OutputBlockKind) -> u64 {
    kind.cache_version()
}
```

删除其原 `DefaultHasher` 实现体与 `use std::hash::*`（若仅此处用）。

- [ ] **Step 3: 运行全量 TUI 测试**

Run: `cargo test -p cli 2>&1 | tail -15`
Expected: 全 PASS（行为未变，现有 block/renderer/assembler 测试不改而过）。

- [ ] **Step 4: clippy + guard**

Run: `cargo clippy -p cli 2>&1 | tail -5 && bash .agents/hooks/check-architecture-guards.sh 2>&1 | tail -5`
Expected: 无 error、guard 通过。

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/render/output/document_renderer.rs apps/cli/src/tui/view_assembler/output.rs
git commit -m "refactor(tui): document_renderer/assembler 改用 BlockComponent (refs #60)"
```

### Task 1.3：保留 render_block 包装或删除

**Files:**
- Modify: `apps/cli/src/tui/render/output/blocks/mod.rs`

- [ ] **Step 1: 收口 render_block**

`render_block` 现仅被测试/历史路径引用。grep 确认无非测试调用：

Run: `grep -rn 'render_block(' apps/cli/src/tui --include=*.rs | grep -v 'block_component.rs\|fn render_block'`
- 若为零（除测试）：删除 `blocks/mod.rs::render_block` 及其 `#[cfg(test)] mod tests` 里依赖它的用例，改为通过 `kind.component().render_self()` 调用。
- 若仍有引用：逐个改为 `kind.component().render_self(id, ctx)`。

- [ ] **Step 2: 运行 + 提交**

Run: `cargo test -p cli 2>&1 | tail -10`
Expected: PASS。

```bash
git add apps/cli/src/tui/render/output/blocks/mod.rs
git commit -m "refactor(tui): 移除 render_block match 分发，统一走 trait (refs #60)"
```

---

## Phase 2：BlockNode 树骨架（children 暂空，行为不变）

引入 `BlockNode` 树与递归渲染器，但 assembler 暂不建子节点（所有 node children 为空），渲染输出与 Phase 1 完全一致。

### Task 2.1：定义 BlockNode 与 OutputViewModel.roots

**Files:**
- Modify: `apps/cli/src/tui/view_model/output.rs`

- [ ] **Step 1: 写失败测试**

在 `view_model/output.rs` 末尾追加：

```rust
#[cfg(test)]
mod node_tests {
    use super::*;

    fn leaf(id: &str) -> BlockNode {
        let kind = OutputBlockKind::Separator;
        BlockNode {
            block_id: id.into(),
            block_version: 0,
            kind,
            children: Vec::new(),
        }
    }

    #[test]
    fn test_block_node_leaf_has_no_children() {
        let n = leaf("a");
        assert!(n.children.is_empty());
    }

    #[test]
    fn test_block_node_can_nest_child() {
        let mut parent = leaf("p");
        parent.children.push(leaf("c"));
        assert_eq!(parent.children[0].block_id, "c");
    }

    #[test]
    fn test_output_view_model_roots_default_empty() {
        let vm = OutputViewModel::default();
        assert!(vm.roots.is_empty());
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p cli node_tests 2>&1 | head -15`
Expected: 编译失败（`BlockNode`/`roots` 未定义）。

- [ ] **Step 3: 加 BlockNode + roots（与 blocks 并存过渡）**

在 `view_model/output.rs`：

```rust
/// 渲染树节点。children 为子块（如 tool result 子块）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockNode {
    pub block_id: String,
    pub block_version: u64,
    pub kind: OutputBlockKind,
    pub children: Vec<BlockNode>,
}
```

`OutputViewModel` 增加 `pub roots: Vec<BlockNode>,` 字段，`Default` 里 `roots: Vec::new()`。**过渡期保留 `blocks` 字段**（Phase 6 删），避免一次性大改 assembler/renderer。

- [ ] **Step 4: 运行验证通过 + 全量**

Run: `cargo test -p cli 2>&1 | tail -10`
Expected: 全 PASS（新增字段有默认值，现有构造处补 `roots: Vec::new()` 或用 `..Default::default()`；逐个修编译错误）。

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/view_model/output.rs
git commit -m "feat(tui): 新增 BlockNode 树节点与 OutputViewModel.roots (refs #60)"
```

### Task 2.2：assembler 同时产 roots（叶子，children 空）

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/output.rs`

- [ ] **Step 1: 写测试**

在 `view_assembler/output_tests.rs` 追加（断言 roots 与 blocks 一一对应、children 空）：

```rust
#[test]
fn test_assemble_roots_mirror_blocks_as_leaves() {
    use crate::tui::model::conversation::model::ConversationModel;
    let mut conv = ConversationModel::default();
    conv.observe_user_message_for_test("u1", "hello"); // 若无此 helper，用现有构造 user message 的测试入口
    let vm = OutputViewAssembler::assemble_from_conversation(&conv, 1);
    assert_eq!(vm.roots.len(), vm.blocks.len());
    assert!(vm.roots.iter().all(|n| n.children.is_empty()));
    assert_eq!(vm.roots[0].block_id, vm.blocks[0].block_id);
}
```

> 注：若 `ConversationModel` 无 `observe_user_message_for_test`，复用 `output_tests.rs` 现有构造会话的方式（读该测试文件首部 helper）。

- [ ] **Step 2: 运行失败**

Run: `cargo test -p cli test_assemble_roots_mirror_blocks 2>&1 | head -15`
Expected: FAIL（roots 为空）。

- [ ] **Step 3: assembler 末尾同步构造 roots**

在 `assemble_from_conversation` 返回前，由已构造的 `blocks` 平铺成叶子 roots：

```rust
let roots = blocks
    .iter()
    .map(|b| BlockNode {
        block_id: b.block_id.clone(),
        block_version: b.block_version,
        kind: b.kind.clone(),
        children: Vec::new(),
    })
    .collect();
OutputViewModel {
    blocks,
    roots,
    version,
    follow_tail_hint: true,
}
```

import 加 `use crate::tui::view_model::BlockNode;`（按现有 import 风格）。

- [ ] **Step 4: 运行通过**

Run: `cargo test -p cli 2>&1 | tail -10`
Expected: 全 PASS。

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/view_assembler/output.rs apps/cli/src/tui/view_assembler/output_tests.rs
git commit -m "feat(tui): assembler 产 roots 叶子树（与 blocks 镜像，过渡）(refs #60)"
```

### Task 2.3：渲染器递归走 roots（depth=0，输出不变）

**Files:**
- Modify: `apps/cli/src/tui/render/output/document_renderer.rs`

- [ ] **Step 1: 写测试（递归展平顺序 + 子块缓存独立）**

在 `document_renderer.rs` 测试模块追加：

```rust
#[test]
fn test_render_tree_dfs_flattens_parent_then_children() {
    use crate::tui::view_model::output::{BlockNode, OutputBlockKind, TextBlockView};
    use crate::tui::view_model::style::SemanticStyle;

    fn node(id: &str, text: &str, children: Vec<BlockNode>) -> BlockNode {
        let kind = OutputBlockKind::SystemNotice(TextBlockView {
            key: id.into(),
            text: text.into(),
            style: SemanticStyle::Muted,
        });
        BlockNode { block_id: id.into(), block_version: kind.cache_version(), kind, children }
    }

    let vm = OutputViewModel {
        blocks: Vec::new(),
        roots: vec![node("p", "parent", vec![node("c", "child", vec![])])],
        version: 1,
        follow_tail_hint: true,
    };
    let mut renderer = OutputDocumentRenderer::default();
    let doc = renderer.render_tree(&vm, 80);

    assert_eq!(doc.blocks.len(), 2);
    assert_eq!(doc.blocks[0].block_id, "p");
    assert_eq!(doc.blocks[1].block_id, "c");
}
```

- [ ] **Step 2: 运行失败**

Run: `cargo test -p cli test_render_tree_dfs 2>&1 | head -15`
Expected: FAIL（`render_tree` 未定义）。

- [ ] **Step 3: 实现 render_tree（递归 DFS）**

在 `OutputDocumentRenderer` impl 增加（暂与旧 `render(blocks)` 并存）：

```rust
use crate::tui::render::output::block_component::BlockComponent;
use crate::tui::view_model::output::BlockNode;

impl OutputDocumentRenderer {
    pub fn render_tree(&mut self, view_model: &OutputViewModel, width: u16) -> RenderedDocument {
        let mut blocks = Vec::new();
        let mut live_ids = Vec::new();
        for root in &view_model.roots {
            self.render_node(root, width, 0, &mut blocks, &mut live_ids);
        }
        self.cache.retain(&live_ids);
        RenderedDocument {
            blocks: trim_blocks_to_max_lines(blocks, MAX_LINES),
        }
    }

    fn render_node(
        &mut self,
        node: &BlockNode,
        width: u16,
        _depth: usize,
        out: &mut Vec<RenderedBlock>,
        live_ids: &mut Vec<String>,
    ) {
        let key = CacheKey { version: node.block_version, width };
        let rendered = self.cache.get_or_render(&node.block_id, key, |ctx| {
            #[cfg(test)]
            self.render_count.set(self.render_count.get() + 1);
            node.kind.component().render_self(&node.block_id, ctx)
        });
        live_ids.push(node.block_id.clone());
        out.push(rendered);
        for child in &node.children {
            self.render_node(child, width, _depth + 1, out, live_ids);
        }
    }
}
```

> 注：`_depth` 暂未用（gutter 在 Phase 4 接入）。此步仅展平。

- [ ] **Step 4: 切换调用方到 render_tree**

grep 找 `document_renderer` / `.render(` 的调用点（应在 `adapter/output_widget.rs`），改为调 `render_tree`。

Run: `grep -rn '\.render(.*view_model\|OutputDocumentRenderer' apps/cli/src/tui --include=*.rs | grep -v document_renderer.rs`

- [ ] **Step 5: 运行 + clippy + guard + commit**

Run: `cargo test -p cli 2>&1 | tail -10 && cargo clippy -p cli 2>&1 | tail -5`
Expected: 全 PASS。

```bash
git add apps/cli/src/tui/render/output/document_renderer.rs apps/cli/src/tui/adapter/output_widget.rs
git commit -m "feat(tui): document_renderer 递归 render_tree DFS 展平 (refs #60)"
```

---

## Phase 3：primitive 去 indent 契约（assistant 行为不变）

`render_fenced_markdown` 移除 `indent` 参数——产无缩进行（spans/plain 均不含缩进）。缩进将在 Phase 4 由渲染器组合期注入。assistant 现传 `""` 空缩进 → 行为不变；tool_call 现传 `INDENT` → 暂在调用方自己拼前缀（Phase 6 子块化后移除）。

### Task 3.1：render_fenced_markdown 去 indent

**Files:**
- Modify: `apps/cli/src/tui/render/output/primitives/fenced.rs`
- Modify: `apps/cli/src/tui/render/output/blocks/assistant_message.rs`（若调用点传 indent）
- Modify: `apps/cli/src/tui/render/output/blocks/tool_call.rs`（调用方改为自拼 INDENT 前缀，过渡）

- [ ] **Step 1: 写测试（产出不含缩进）**

在 `fenced.rs` 测试模块追加：

```rust
#[test]
fn test_render_fenced_markdown_no_indent_in_plain_or_spans() {
    use ratatui::style::Style;
    let lines = render_fenced_markdown("hello\nworld", Style::default(), 80);
    assert!(!lines[0].plain.starts_with(' '), "plain 不应含前导缩进");
    let first_span = lines[0].spans[0].content.as_ref();
    assert!(!first_span.starts_with(' '), "spans 不应含前导缩进");
}
```

- [ ] **Step 2: 运行失败**

Run: `cargo test -p cli test_render_fenced_markdown_no_indent 2>&1 | head -15`
Expected: 编译失败（签名仍有 indent 参数）。

- [ ] **Step 3: 改签名删 indent**

`render_fenced_markdown(text, base_style, indent, width)` → `render_fenced_markdown(text, base_style, width)`；函数体内删除所有 `indent` 前缀拼接（每处 `format!("{indent}{...}")` → 去掉 `{indent}`）；更新文件头注释（删除 indent 段）。同时更新 fenced.rs 内既有测试中对 indent 的调用。

- [ ] **Step 4: 改调用方**

- assistant_message.rs：若调 `render_fenced_markdown(.., "", ..)`，删中间空串实参。
- tool_call.rs `format_result_lines`：改为先调 `render_fenced_markdown(result, base, width)`，再对每行自拼 `INDENT`（过渡，保持当前视觉）：

```rust
let rendered = render_fenced_markdown(result, base, width);
let rendered: Vec<RenderedLine> = rendered
    .into_iter()
    .map(|l| {
        let mut spans = vec![Span::styled(INDENT.to_string(), base)];
        spans.extend(l.spans);
        RenderedLine::with_plain(spans, format!("{INDENT}{}", l.plain))
    })
    .collect();
```

> 注：此过渡保持现状（含 indent 进 plain），Phase 6 子块化后这段连同 INDENT 一并删除，改由 gutter 注入。

- [ ] **Step 5: 运行 + commit**

Run: `cargo test -p cli 2>&1 | tail -10`
Expected: 全 PASS（assistant/tool 视觉不变）。

```bash
git add apps/cli/src/tui/render/output/primitives/fenced.rs apps/cli/src/tui/render/output/blocks/assistant_message.rs apps/cli/src/tui/render/output/blocks/tool_call.rs
git commit -m "refactor(tui): render_fenced_markdown 去 indent 参数，产无缩进行 (refs #60)"
```

---

## Phase 4：gutter 注入（渲染器组合期，缩进/marker 不进 plain）

每行前置固定宽度 gutter = `[depth 缩进] + [marker 列]`，只进 spans 不进 plain；marker 按 kind/status 取静态字形，仅首行画、后续等宽空白。组件 `render_self` 不再自写 marker/缩进。选区列偏移补偿 gutter 宽度。

### Task 4.1：gutter 模块（字形映射 + 行前置）

**Files:**
- Create: `apps/cli/src/tui/render/output/gutter.rs`
- Modify: `apps/cli/src/tui/render/output/mod.rs`（`pub mod gutter;`）

- [ ] **Step 1: 写测试**

`gutter.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::rendered::RenderedLine;
    use crate::tui::view_model::output::{OutputBlockKind, ToolCallBlockView, ToolSemanticStatus, TextBlockView};
    use crate::tui::view_model::style::SemanticStyle;
    use ratatui::text::Span;

    fn tool(status: ToolSemanticStatus) -> OutputBlockKind {
        OutputBlockKind::ToolCall(ToolCallBlockView {
            key: "t".into(), chat_id: None, turn_id: None, tool_call_id: None,
            title: "Grep".into(), icon: "●".into(), semantic_status: status,
            style: SemanticStyle::Running, args_preview: None, summary: None,
            activity_summary: None, result_summary: None, collapsible: false, collapsed: false,
        })
    }

    #[test]
    fn test_marker_glyph_for_tool_status() {
        assert_eq!(marker_glyph(&tool(ToolSemanticStatus::Success)), "✓");
        assert_eq!(marker_glyph(&tool(ToolSemanticStatus::Error)), "✗");
        assert_eq!(marker_glyph(&tool(ToolSemanticStatus::Running)), "●");
    }

    #[test]
    fn test_apply_gutter_first_line_has_marker_rest_blank_not_in_plain() {
        let kind = tool(ToolSemanticStatus::Success);
        let lines = vec![
            RenderedLine::new(vec![Span::raw("Grep /x/")]),
            RenderedLine::new(vec![Span::raw("detail")]),
        ];
        let out = apply_gutter(&kind, 0, lines);
        // 首行 spans 第一个含 ✓，plain 不含
        assert!(out[0].spans[0].content.as_ref().contains('✓'));
        assert_eq!(out[0].plain, "Grep /x/");
        // 后续行 gutter 为等宽空白，plain 不变
        assert!(out[1].spans[0].content.as_ref().chars().all(|c| c == ' '));
        assert_eq!(out[1].plain, "detail");
    }

    #[test]
    fn test_apply_gutter_depth_widens_indent() {
        let kind = OutputBlockKind::SystemNotice(TextBlockView {
            key: "s".into(), text: "x".into(), style: SemanticStyle::Muted,
        });
        let d0 = apply_gutter(&kind, 0, vec![RenderedLine::new(vec![Span::raw("x")])]);
        let d1 = apply_gutter(&kind, 1, vec![RenderedLine::new(vec![Span::raw("x")])]);
        let w0 = d0[0].spans[0].content.chars().count();
        let w1 = d1[0].spans[0].content.chars().count();
        assert!(w1 > w0, "depth 越深，gutter 前导越宽");
        assert_eq!(d1[0].plain, "x", "缩进不进 plain");
    }
}
```

- [ ] **Step 2: 运行失败**

Run: `cargo test -p cli gutter 2>&1 | head -15`
Expected: 编译失败（`marker_glyph`/`apply_gutter` 未定义）。

- [ ] **Step 3: 实现 gutter**

```rust
//! 行首标志槽 gutter：depth 缩进 + marker 列。组合期注入，只进 spans 不进 plain。
//! marker 静态（按 kind/status），仅首行画，后续行等宽空白。见 spec §6.5。

use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::theme;
use crate::tui::view_model::output::{OutputBlockKind, ToolSemanticStatus};
use ratatui::style::Style;
use ratatui::text::Span;

/// marker 列字符宽度（字形 1 + 空格 1）。
pub const GUTTER_WIDTH: usize = 2;
const PER_DEPTH_INDENT: usize = 2;

/// 按 block kind/status 取静态 marker 字形。
pub fn marker_glyph(kind: &OutputBlockKind) -> &'static str {
    match kind {
        OutputBlockKind::ToolCall(t) => match t.semantic_status {
            ToolSemanticStatus::Success => "✓",
            ToolSemanticStatus::Error => "✗",
            ToolSemanticStatus::Cancelled => "–",
            ToolSemanticStatus::Orphaned => "?",
            ToolSemanticStatus::Pending | ToolSemanticStatus::Running => "●",
        },
        OutputBlockKind::UserMessage(_) => ">",
        _ => " ", // 无状态块：marker 列留空格（保持对齐）
    }
}

/// marker 颜色（与现 tool 状态色一致；非工具用 muted）。
fn marker_color(kind: &OutputBlockKind) -> ratatui::style::Color {
    match kind {
        OutputBlockKind::ToolCall(t) => match t.semantic_status {
            ToolSemanticStatus::Success => theme::SUCCESS,
            ToolSemanticStatus::Error => theme::ERROR,
            ToolSemanticStatus::Running | ToolSemanticStatus::Pending => theme::TOOL_RUNNING,
            ToolSemanticStatus::Cancelled => theme::TEXT_MUTED,
            ToolSemanticStatus::Orphaned => theme::WARNING,
        },
        OutputBlockKind::UserMessage(_) => theme::USER,
        _ => theme::TEXT_MUTED,
    }
}

/// gutter 总显示宽度（供选区列偏移补偿用）。
pub fn gutter_width(depth: usize) -> usize {
    depth * PER_DEPTH_INDENT + GUTTER_WIDTH
}

/// 为一个 block 的所有行前置 gutter（首行带 marker，余行等宽空白）。
/// gutter 只进 spans，不进 plain。
pub fn apply_gutter(kind: &OutputBlockKind, depth: usize, lines: Vec<RenderedLine>) -> Vec<RenderedLine> {
    let indent = " ".repeat(depth * PER_DEPTH_INDENT);
    let glyph = marker_glyph(kind);
    let color = marker_color(kind);
    lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let gutter_text = if i == 0 {
                format!("{indent}{glyph} ")
            } else {
                " ".repeat(depth * PER_DEPTH_INDENT + GUTTER_WIDTH)
            };
            let mut spans = vec![Span::styled(gutter_text, Style::default().fg(color))];
            spans.extend(line.spans);
            // plain 保持原样，不含 gutter
            RenderedLine::with_plain(spans, line.plain)
        })
        .collect()
}
```

> 注：`theme::USER` 若不存在，用现有用户消息色常量（查 `render/theme/palette.rs`）。`GUTTER_WIDTH` 与首行 `{glyph} ` 宽度一致（glyph 1 + 空格 1 = 2）。

- [ ] **Step 4: 运行通过**

Run: `cargo test -p cli gutter 2>&1 | tail -10`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/render/output/gutter.rs apps/cli/src/tui/render/output/mod.rs
git commit -m "feat(tui): 新增 gutter 模块（缩进+marker，不进 plain）(refs #60)"
```

### Task 4.2：渲染器组合期注入 gutter + 组件去自写 marker

**Files:**
- Modify: `apps/cli/src/tui/render/output/document_renderer.rs`（`render_node` 注入 gutter）
- Modify: `apps/cli/src/tui/render/output/blocks/tool_call.rs`（移除首行自写 `● `/icon、移除 detail 自拼 INDENT）
- Modify: `apps/cli/src/tui/render/output/blocks/user_message.rs`、`assistant_message.rs` 等（若自写前缀/缩进则移除）

- [ ] **Step 1: render_node 注入 gutter**

`render_node` 内，缓存取回的是「无 gutter」行；注入在缓存**外**（gutter 依 depth/status，不进缓存内容）：

```rust
let rendered = self.cache.get_or_render(&node.block_id, key, |ctx| {
    #[cfg(test)]
    self.render_count.set(self.render_count.get() + 1);
    node.kind.component().render_self(&node.block_id, ctx)
});
live_ids.push(node.block_id.clone());
// gutter 在缓存外注入：缩进+marker 随 depth/status 变，但不污染缓存内容
let gutted = crate::tui::render::output::gutter::apply_gutter(&node.kind, _depth, rendered.lines);
out.push(RenderedBlock { block_id: rendered.block_id, lines: gutted });
```

把 `_depth` 改名 `depth`（现在用到了）。

- [ ] **Step 2: 组件去自写 marker/缩进**

- `tool_call.rs::render_tool_call`：首行不再写 `Span::styled(format!("{} ", view.icon), ...)`；header 只渲标题（marker 由 gutter 出）。detail 行删除 `{INDENT}` 前缀。result 部分（Phase 6 会搬走）暂保留但去掉自拼 INDENT（因 gutter 接管缩进）。
- `user_message.rs` 等：若有 `> ` 前缀，删除（gutter 出 `>`）。

> 关键：组件产「纯内容、无 gutter」行，否则会与 gutter 叠加导致双缩进/双 marker。逐个组件核对。

- [ ] **Step 3: 更新组件测试期望**

现有 `tool_call.rs` 测试断言 `spans[0].content == "● "` 等——改为断言标题内容存在即可（marker 移到 gutter，由 `gutter.rs` 测试覆盖）。逐个修正受影响断言。

- [ ] **Step 4: 运行 + 目视**

Run: `cargo test -p cli 2>&1 | tail -15`
Expected: 全 PASS。

Run（目视确认 gutter 对齐，可选）：`cargo run -p cli -- --help >/dev/null 2>&1 || true`（或在交互环境手测）。

- [ ] **Step 5: clippy + guard + commit**

Run: `cargo clippy -p cli 2>&1 | tail -5 && bash .agents/hooks/check-architecture-guards.sh 2>&1 | tail -5`

```bash
git add apps/cli/src/tui/render/output/document_renderer.rs apps/cli/src/tui/render/output/blocks/
git commit -m "feat(tui): 渲染器组合期注入 gutter，组件去自写 marker/缩进 (refs #60)"
```

### Task 4.3：选区列偏移补偿 gutter 宽度

**Files:**
- Modify: `apps/cli/src/tui/render/output_area/render.rs`（`screen_map` 构造、`sel_range_for_line`）

- [ ] **Step 1: 理解现状**

读 `output_area/render.rs:53-69`：`screen_map.push((idx, CharIdx::ZERO, char_end))` 把每行可选范围设为 `[0, plain.chars().count())`。现状 plain 含 INDENT（过渡），gutter 化后 plain 不含 gutter，但**屏幕上行首多了 gutter 宽度的列**——鼠标点击列号 → plain 字符偏移需减 gutter 宽度。

- [ ] **Step 2: 写测试**

在 `output_area/render.rs` 测试模块加（构造一个 depth=0 的 block，gutter 宽 2，断言选区从屏幕列 2 起对应 plain 第 0 字符）。因 gutter 宽度需随行已知，最简做法是 `RenderedLine` 增加可选 `gutter_cols: usize` 字段记录该行 gutter 显示宽度；渲染器注入 gutter 时写入。测试：

```rust
#[test]
fn test_selection_skips_gutter_cols() {
    // 一行 plain="ab"，gutter_cols=2；屏幕列 0..2 属 gutter（不可选），列 2 起对应 plain[0]
    // 详见实现：sel 映射 screen_col -> plain_char = screen_col.saturating_sub(gutter_cols)
}
```

- [ ] **Step 3: 实现**

`rendered.rs::RenderedLine` 增加 `pub gutter_cols: usize`（默认 0；`new`/`with_plain` 设 0）。`gutter::apply_gutter` 设 `gutter_cols = gutter_width(depth)`。`output_area/render.rs` 构造 `screen_map` 时，把每行的可选起始屏幕列设为 `gutter_cols`，并在屏幕列→plain 字符映射处减去 `gutter_cols`；`sel_range_for_line` 的 `plain_len` 不变（仍 `plain.chars().count()`）。

> 注：这是本 Phase 最易出错处。务必新增覆盖「CJK + gutter」「多行 gutter」用例。

- [ ] **Step 4: 运行 + commit**

Run: `cargo test -p cli 2>&1 | tail -10`
Expected: PASS（含选区回归）。

```bash
git add apps/cli/src/tui/render/output/rendered.rs apps/cli/src/tui/render/output/gutter.rs apps/cli/src/tui/render/output_area/render.rs
git commit -m "feat(tui): 选区列偏移补偿 gutter 宽度（gutter 不进 plain）(refs #60)"
```

---

## Phase 5：嵌套规则表 + 校验 + guard

### Task 5.1：allowed_child + MAX_BLOCK_DEPTH

**Files:**
- Create: `apps/cli/src/tui/render/output/nesting.rs`
- Modify: `apps/cli/src/tui/render/output/mod.rs`（`pub mod nesting;`）

- [ ] **Step 1: 写测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::view_model::output::*;
    use crate::tui::view_model::style::SemanticStyle;

    fn tool() -> OutputBlockKind { OutputBlockKind::ToolCall(/* ... 同 gutter 测试构造 */ ) }
    fn text() -> OutputBlockKind { OutputBlockKind::AssistantMessage(TextBlockView{key:"a".into(),text:"x".into(),style:SemanticStyle::Normal}) }

    #[test]
    fn test_toolcall_allows_assistant_child() {
        assert!(allowed_child(&tool(), &text()));
    }
    #[test]
    fn test_usermessage_is_leaf() {
        assert!(!allowed_child(&text(), &text())); // 非 ToolCall 父不允许任何子
    }
    #[test]
    fn test_max_block_depth_is_three() {
        assert_eq!(MAX_BLOCK_DEPTH, 3);
    }
}
```

- [ ] **Step 2: 运行失败 → 实现**

```rust
//! block 嵌套合法性规则。见 spec §4。
use crate::tui::view_model::output::OutputBlockKind;

pub const MAX_BLOCK_DEPTH: usize = 3;

/// 仅 ToolCall 可含子（AssistantMessage/Diagnostic/SystemNotice 等富渲染子块）；其余为叶子。
pub fn allowed_child(parent: &OutputBlockKind, child: &OutputBlockKind) -> bool {
    matches!(parent, OutputBlockKind::ToolCall(_))
        && matches!(
            child,
            OutputBlockKind::AssistantMessage(_)
                | OutputBlockKind::DiagnosticNotice(_)
                | OutputBlockKind::SystemNotice(_)
        )
}
```

- [ ] **Step 3: assembler 建树校验调用**

在 assembler 构造 children 处（Phase 6 会真正建子节点；此处先加校验函数 + 在 push child 前断言）。新增 helper：

```rust
fn push_child_checked(parent: &mut BlockNode, child: BlockNode, depth: usize) {
    use crate::tui::render::output::nesting::{allowed_child, MAX_BLOCK_DEPTH};
    if depth >= MAX_BLOCK_DEPTH || !allowed_child(&parent.kind, &child.kind) {
        debug_assert!(false, "非法嵌套或超深: parent={:?} depth={}", parent.kind, depth);
        log::warn!("丢弃非法子块: depth={depth}");
        return;
    }
    parent.children.push(child);
}
```

- [ ] **Step 4: guard 脚本**

新建 `.agents/hooks/check-tui-block-nesting.sh`：grep 确保 ① 组件 `render_self` 内无 `apply_gutter`/`INDENT` 自写（gutter 唯一在渲染器）；② assembler 建子节点只经 `push_child_checked`（无裸 `.children.push(`）。接入 `.agents/hooks/check-architecture-guards.sh`。

- [ ] **Step 5: 运行 + commit**

Run: `cargo test -p cli 2>&1 | tail -10 && bash .agents/hooks/check-tui-block-nesting.sh`

```bash
git add apps/cli/src/tui/render/output/nesting.rs apps/cli/src/tui/render/output/mod.rs apps/cli/src/tui/view_assembler/output.rs .agents/hooks/check-tui-block-nesting.sh .agents/hooks/check-architecture-guards.sh
git commit -m "feat(tui): 嵌套规则表 allowed_child/MAX_BLOCK_DEPTH + 校验 + guard (refs #60)"
```

---

## Phase 6：ToolCall result 升为子块（删内联渲染）

### Task 6.1：assembler 把 ToolResult 建为子节点

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/output.rs`
- Modify: `apps/cli/src/tui/render/output/blocks/tool_call.rs`（删 result 内联渲染分支）
- Modify: `apps/cli/src/tui/view_model/output.rs`（删 `blocks` 字段，roots 成唯一真相）

- [ ] **Step 1: 写测试（result 成为 ToolCall 子节点）**

在 `output_tests.rs` 构造一个带 result 的 tool call 会话，断言：

```rust
#[test]
fn test_tool_result_becomes_child_node() {
    // 构造 ToolCall + ToolResult(同 id, 文本结果)
    let vm = OutputViewAssembler::assemble_from_conversation(&conv, 1);
    let tool_node = vm.roots.iter().find(|n| matches!(n.kind, OutputBlockKind::ToolCall(_))).unwrap();
    assert!(!tool_node.children.is_empty(), "result 应成为 tool call 子节点");
    // 子节点是 result 富渲染块（AssistantMessage/Diagnostic）
}
```

- [ ] **Step 2: 运行失败 → assembler 建子节点**

在 `assemble_from_conversation` 的 ToolCall 分支：构造 tool 父 `BlockNode` 后，读其 result 文本（现 `result_summary`/`find_tool_view` 已有），按 §7 规则建子 `BlockNode`（Edit diff → 走 edit_diff 渲染的子块；其余文本 → `AssistantMessage` 子块走 fenced markdown；纯摘要 → `Diagnostic/SystemNotice`），用 `push_child_checked` 挂入。`block_version` 用子 `kind.cache_version()`。删除把 ToolResult 作顶层块的旧分支（`tool_result_is_embedded` 为真时跳过保留）。

> 子块复用现有渲染：子块 kind 为 `AssistantMessage(TextBlockView{text: result})` 时，其 `render_self` 已走 `render_fenced_markdown`（assistant 组件），天然富渲染。Edit diff 需一个承载 diff 的子块 kind——初版可用 `AssistantMessage` 文本 + 在 assistant 组件中已有的 fenced ```diff 路径，或新增轻量 `OutputBlockKind` 变体（评估后定；若新增变体，需在 `block_component`/`gutter`/`nesting` 同步分支）。

- [ ] **Step 3: 删 tool_call 内联 result 渲染**

`render_tool_call` 移除 `render_edit_diff(...)` 调用与 `format_result_lines`/`activity_summary`/`result_summary` 渲染循环——父块只渲 header + args detail。删除 Phase 3 过渡加的 INDENT 自拼。

- [ ] **Step 4: 删 OutputViewModel.blocks，roots 成唯一真相**

删 `OutputViewModel.blocks` 字段及 assembler 中 `blocks` 构造；`document_renderer` 删旧 `render(blocks)` 方法（仅留 `render_tree`），调用方已在 Phase 2.3 切换。grep 确认无 `.blocks` 残留引用（除 `RenderedDocument.blocks`）。

- [ ] **Step 5: 运行（含 #65 加固回归）+ commit**

在 `output_tests.rs` 加 #65 加固断言：result 含 fenced code block 时，其后的兄弟 root 块首行不带 CODE 色。

Run: `cargo test -p cli 2>&1 | tail -15 && cargo clippy -p cli 2>&1 | tail -5`
Expected: 全 PASS。

```bash
git add apps/cli/src/tui/view_assembler/output.rs apps/cli/src/tui/render/output/blocks/tool_call.rs apps/cli/src/tui/view_model/output.rs apps/cli/src/tui/render/output/document_renderer.rs apps/cli/src/tui/view_assembler/output_tests.rs
git commit -m "feat(tui): tool result 升为子块，删父块内联渲染与 blocks 字段 (refs #60)"
```

---

## Phase 7：缓存 retain 全树 DFS + MAX_LINES 按 root 子树裁剪

### Task 7.1：retain 全树 + 按 root 子树裁剪

**Files:**
- Modify: `apps/cli/src/tui/render/output/document_renderer.rs`

- [ ] **Step 1: 写测试**

```rust
#[test]
fn test_child_version_change_only_rerenders_child() {
    // 父 + 子两 node；改子 block_version，断言 render_count 只 +1（父命中缓存）
}

#[test]
fn test_retain_keeps_all_tree_block_ids() {
    // 父+子渲染后，再渲染只含父的树，子 block_id 应被 retain 清除
}

#[test]
fn test_trim_drops_oldest_root_subtree_as_group() {
    // 两个 root 子树（各含 1 子=2 行/树），MAX 设 3 → 丢最旧整棵 root 子树（2 行），保留新树
}
```

- [ ] **Step 2: 运行失败 → 实现**

- `render_node` 已在 Phase 2.3 把每个 node 的 `block_id` push 进 `live_ids`（全树 DFS）—确认 retain 用的是全树 live_ids。
- `trim_blocks_to_max_lines` 改为「按 root 子树分组裁剪」：`render_tree` 收集时记录每个 RenderedBlock 所属 root 边界（如 `render_node` 额外产出 `Vec<(root_start_idx, line_count)>`，或给 RenderedBlock 标 `root_id`）。裁剪时从尾部按整棵 root 子树累加行数，超 MAX 丢弃最旧整棵子树。

```rust
// 简化实现：render_tree 为每个 root 产出 (root_blocks: Vec<RenderedBlock>)，
// trim 在「Vec<Vec<RenderedBlock>>」层面按 root 组裁剪，再 flatten。
```

- [ ] **Step 3: 运行通过**

Run: `cargo test -p cli document_renderer 2>&1 | tail -10`
Expected: 3 新测试 PASS。

- [ ] **Step 4: 全量 + clippy + guard**

Run: `cargo test -p cli 2>&1 | tail -15 && cargo clippy -p cli 2>&1 | tail -5 && bash .agents/hooks/check-architecture-guards.sh 2>&1 | tail -5`
Expected: 全绿。

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/render/output/document_renderer.rs
git commit -m "feat(tui): retain 全树 DFS + MAX_LINES 按 root 子树裁剪 (refs #60)"
```

---

## 收尾：文档联动

### Task 8.1：更新 feature #60 状态

**Files:**
- Modify: `docs/feature/active.md`

- [ ] **Step 1:** 把 #60 行状态从「设计中」改为「待确认」，补一句实现摘要（trait + 树 + gutter 已落地，result 子块化，#65 加固回归通过）。
- [ ] **Step 2: Commit**

```bash
git add docs/feature/active.md
git commit -m "docs: #60 状态更新为待确认 (refs #60)"
```

---

## Self-Review（已对照 spec）

- **§1 trait** → Phase 1。**§2 树** → Phase 2。**§3 渲染器递归** → 2.3。**§4 嵌套规则** → Phase 5。**§5 缓存不折叠** → 2.3（version=node 自身）+ 7.1（retain 全树）。**§6 缩进不进 plain** → Phase 3（去 indent）+ 4.3（列偏移补偿）。**§6.5 gutter** → Phase 4。**§7 assembler 子块** → Phase 6。**§8 #65 加固（不认领）** → 6.1 回归断言。**§9 测试**散落各 Task。**§10 guard** → 5.1。
- **遗留确认项**（实现时拍板，不阻塞）：Edit diff 子块用「AssistantMessage + ```diff fence」还是「新增 OutputBlockKind 变体」——见 Task 6.1 Step 2 注；若新增变体，须同步 `block_component`/`gutter`/`nesting` 三处分支。
- **类型一致性**：`render_self(block_id, ctx)`、`apply_gutter(kind, depth, lines)`、`gutter_width(depth)`、`allowed_child(parent, child)`、`MAX_BLOCK_DEPTH`、`push_child_checked` 全程一致。
