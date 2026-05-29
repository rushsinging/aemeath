# TUI 输出区渲染管线统一重构 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 TUI 输出区（spinner 上方）从「ViewModel → 纯文本切行 → 拍平桥 → OutputLine」的有损双表示管线，统一重构为「ViewModel(带 block 版本) → OutputDocumentRenderer + BlockRenderer 组件 → RenderedDocument(spans+plain) → OutputArea 显示容器」单一富渲染管线，恢复 markdown/语法/主题色，并消除有损桥与多表示。

**Architecture:** 三层职责严格分离——ViewAssembler 只做 Model→ViewModel(语义+block 版本，无渲染)；OutputDocumentRenderer 持 block 级缓存，按 block 变体分发到 BlockRenderer 组件，产出 `RenderedDocument`（每行含显示 `spans` 与逻辑 `plain`）；OutputArea 只做 viewport/scroll/selection/spinner，不碰业务、不解析 markdown。markdown/syntax/diff/table 改造为产出 `RenderedLine` 的共享纯函数原语。选区基于 `plain` 字符偏移，唯一上色路径 `apply_selection_overlay`。采用方案 A 逐 block 增量切换，旧路径在对应 block 全切后才删。

**Tech Stack:** Rust + ratatui（`Span`/`Line`）+ unicode-width；`sdk::CharIdx`（字符索引）；`syntect`（语法高亮，经 `render/syntax.rs`）；`std::hash::DefaultHasher`（block 版本）。

---

## 设计决策（约束 —— 所有 Task MUST 遵守）

这些决策来自对 spec 与现状代码的交叉审查，覆盖 spec 与现状的偏差，**优先级高于 spec 原文**：

1. **`RenderedLine` 命名冲突**：现状 `apps/cli/src/tui/render/output/cache.rs` 已有 `RenderedLine{ line, screen_entries, rendered_text }` 并被 `output/mod.rs` `pub use`。本重构的新 `RenderedLine{ spans, plain }` 落在 `render/output/rendered.rs`，迁移期 **NEVER** 在 `output/mod.rs` 顶层 `pub use rendered::RenderedLine`，一律用全路径 `crate::tui::render::output::rendered::RenderedLine` 引用。Phase 5 删除 `cache.rs` 旧 `RenderedLine` 后，Phase 6 才允许提升到顶层 re-export。

2. **block 版本不侵入 Model**：`ConversationModel` 不新增 version 字段。`block_version` 在 `OutputViewAssembler` 组装时对 block 语义数据用 `std::hash::DefaultHasher` 计算。要求 `TextBlockView`/`ToolCallBlockView`/`ToolSemanticStatus`/`SemanticStyle` 全部新增 `Hash` derive。

3. **`OutputBlockView` 重构为带 id/version 的 struct**：
   ```rust
   pub struct OutputBlockView { pub block_id: String, pub block_version: u64, pub kind: OutputBlockKind }
   pub enum OutputBlockKind { UserMessage(TextBlockView), QueuedSubmission(TextBlockView),
       AssistantMessage(TextBlockView), ThinkingMessage(TextBlockView), ToolCall(ToolCallBlockView),
       DiagnosticNotice(TextBlockView), SystemNotice(TextBlockView), Separator }
   ```
   `block_id`：文本/工具块用其 `key`；`Separator` 用 `format!("sep-{seq}")`（seq 为该 block 在 blocks 中的序号）。

4. **`RenderCtx` 只持 `width`**：现状主题是编译期 `render/theme/palette.rs` 常量，无运行时 `Theme` 实例。故 `RenderCtx{ width: u16 }`，渲染层直接引用 `theme::*` 常量。`CacheKey = (block_version, width)`，spec 的 `theme_version` 在引入运行时主题前退化省略（代码注释标注 TODO）。

5. **原语统一产出 `RenderedLine`**：新建 `render/output/primitives/`，包装现有 `markdown::inline_markdown_lines`、`diff::build_diff_lines`、`syntax::highlight_line`、`markdown::render_table_block`，统一返回 `Vec<RenderedLine>`（显示用 ratatui `Span`，`plain` 为可见文本）。提供 `SpanPart -> Vec<Span>` 与 `spans -> plain` 两个 helper。markdown 的 `plain` 复用现成 `markdown::strip_inline_formatting`。

6. **选区唯一上色路径**：所有 block 共用 `apply_selection_overlay(line: &RenderedLine, sel: Option<SelRange>) -> Vec<Span<'static>>`，只设选区内字符 `bg=SELECTION_BG`、保留原 `fg`，必要时按边界 split span。**NEVER** 让任何 block 类型绕过该函数自行上色（防 #61/#62 回归）。复制取 `plain` 字符切片。

7. **MAX_LINES 改 block 级裁剪**：超过上限时丢弃最旧的整个 block（而非按行 pop），消除 #71 的陈旧行下标越界类问题。

8. **关联 bug 回归**：本重构需在测试中覆盖并在 `docs/bug/active.md` 联动：#61（diff 选中高亮丢失/行号贴边）、#62（Grep 标题不可见）、#51/#48/#60（复制/CJK 偏移）、#71（缓存越界）、#65/#74（fence/System 样式跨 block 泄漏）、#80（滚动累加）、以及本次两 bug（markdown + theme 恢复）。

---

## File Structure

**新增**
- `render/output/rendered.rs` — `RenderedLine` / `RenderedBlock` / `RenderedDocument` / `RenderCtx`（值类型，纯数据）。
- `render/output/document_renderer.rs` — `OutputDocumentRenderer`（持 `BlockCache`，按 kind 分发）。
- `render/output/block_cache.rs` — `CacheKey` / `CachedBlock` / `BlockCache`。
- `render/output/blocks/mod.rs` + 每组件一文件：`user_message.rs`、`queued_submission.rs`、`assistant_message.rs`、`thinking.rs`、`tool_call.rs`、`diagnostic.rs`、`separator.rs`。
- `render/output/primitives/mod.rs`（+ `markdown.rs`/`diff.rs`/`table.rs`/`convert.rs` 子文件）。
- `render/output/selection_overlay.rs` — `SelRange` + `apply_selection_overlay`（唯一上色路径）。

**改造**
- `view_model/output.rs` — `OutputBlockView` 改 struct{id,version,kind}；新增 `OutputBlockKind`；补 `Hash` derive。
- `view_model/style.rs` — `SemanticStyle` 补 `Hash`。
- `view_assembler/output.rs` — 产出带 `block_id`/`block_version`；`QueuedUserMessage` 映射到 `QueuedSubmission`。
- `render/output_area/mod.rs` / `render.rs` / `content.rs` / `scroll.rs` / `spinner.rs` — `OutputArea` 持 `RenderedDocument` 替代 `VecDeque<OutputLine>`；render 画 `spans`；接选区叠加 + plain 复制。
- `render/output_area/selection_render.rs` — 改为基于 `RenderedDocument`/`plain` 调 `apply_selection_overlay`。
- `render/output/status_line.rs` — `append_status_lines`/`color_tool_call_dots` 适配 `RenderedLine`。

**删除（Phase 5）**
- `adapter/output_widget.rs`（`line_to_plain_text` + `replace_lines_from_view_model` 拍平桥）。
- `render/output_view_model.rs`（`output_view_model_lines` 纯文本路径）。
- `render/output_area/types.rs` 的 `OutputLine` / `LineStyle`（及 `SpanPart` 若无残留引用）。
- `render/output/cache.rs` 旧 `RenderedLine`/`RenderedCache`、`view_state/cache.rs` 的 `OutputRenderCacheState`。
- **旧行级渲染链**：`render/output/line.rs`（`render_range`/`collect_table_ranges`，仅被 `cache.rs` 调用）、`render/output/block.rs`（`CodeBlockInfo`/`scan_code_blocks`，已 `#[allow(dead_code)]`）、`render/output/span.rs`（`slice_spans`，已 `#[allow(dead_code)]`）——删 `cache.rs` 后整条孤立。
- `render/output_area/queued.rs`（`build_queued_message_lines`）及 `OutputArea` 的 `queued_messages`/`queued_line_count`。
- `render/output_area/streaming.rs` 的 `do_rerender` `<think>` 扫描路径。

---

## 验证门禁（每个 Task 末尾按需运行）

- 编译：`cargo build -p cli`
- 单测：`cargo test -p cli <test_name>`（或整包 `cargo test -p cli`）
- Lint：`cargo clippy -p cli -- -D warnings`
- 架构 guard：`.agents/hooks/check-architecture-guards.sh`

---

## Phase 0：追踪登记

### Task 0.1：登记 feature 与关联 bug

**Files:**
- Modify: `docs/feature/active.md`
- Modify: `docs/bug/active.md`

- [ ] **Step 1: 在 `docs/feature/active.md` 表格新增一行**

```
| 58 | TUI 输出区渲染管线统一重构 | 高 | 活动中 | 未确认 | 统一为单一 ViewModel→Render 管线，恢复 markdown+theme，消除有损桥/双表示；详见 [plan](../superpowers/plans/2026-05-29-tui-output-render-pipeline.md) 与 [spec](../superpowers/specs/2026-05-29-tui-output-render-pipeline-design.md) |
```

- [ ] **Step 2: 在 `docs/bug/active.md` 对应行状态更新为「修复中」并备注本 feature**

对 #61、#62、#65、#71、#74、#80 在状态列追加「（随 #58 渲染管线重构修复中）」。

- [ ] **Step 3: Commit**

```bash
git add docs/feature/active.md docs/bug/active.md
git commit -m "docs: 登记 #58 TUI 输出区渲染管线重构 feature 与关联 bug (refs #58)"
```

---

## Phase 1：新表示与骨架（不接线，与现状并存）

本阶段全是新增代码，不改任何现有渲染路径，编译通过即可（新模块仅被测试引用）。

### Task 1.1：定义 `RenderedLine` / `RenderedBlock` / `RenderedDocument` / `RenderCtx`

**Files:**
- Create: `apps/cli/src/tui/render/output/rendered.rs`
- Modify: `apps/cli/src/tui/render/output/mod.rs`（仅加 `pub mod rendered;`，**不** re-export `RenderedLine`）

- [ ] **Step 1: 写失败测试（写在 `rendered.rs` 末尾）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Style};
    use ratatui::text::Span;

    #[test]
    fn test_rendered_line_new_derives_plain_from_spans() {
        let line = RenderedLine::new(vec![
            Span::styled("Hello ", Style::default().fg(Color::Red)),
            Span::styled("世界", Style::default().fg(Color::Blue)),
        ]);
        assert_eq!(line.plain, "Hello 世界");
        assert_eq!(line.spans.len(), 2);
    }

    #[test]
    fn test_rendered_line_with_plain_keeps_explicit_plain() {
        let line = RenderedLine::with_plain(vec![Span::raw("**x**")], "x".to_string());
        assert_eq!(line.plain, "x");
    }

    #[test]
    fn test_rendered_document_total_lines_sums_blocks() {
        let doc = RenderedDocument {
            blocks: vec![
                RenderedBlock { block_id: "a".into(), lines: vec![RenderedLine::default(), RenderedLine::default()] },
                RenderedBlock { block_id: "b".into(), lines: vec![RenderedLine::default()] },
            ],
        };
        assert_eq!(doc.total_lines(), 3);
        assert_eq!(doc.iter_lines().count(), 3);
    }
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cargo test -p cli test_rendered_line_new_derives_plain_from_spans`
Expected: FAIL（`RenderedLine` 未定义 / 模块不存在）

- [ ] **Step 3: 实现 `rendered.rs`**

```rust
//! 输出区渲染产物的值类型：显示 spans 与逻辑 plain 分离。
//!
//! 不变式：每个 `RenderedLine` 的 `plain` 等于其 `spans` 可见文本拼接
//! （见 primitives / blocks 各组件单测断言）。
use ratatui::text::Span;

/// 渲染管线的渲染上下文。
///
/// 当前主题是编译期 `render::theme` 常量，无运行时 Theme，故只持宽度。
/// TODO(theme): 引入运行时主题后加 `theme` 字段并把 theme_version 纳入 CacheKey。
#[derive(Clone, Copy, Debug)]
pub struct RenderCtx {
    pub width: u16,
}

/// 一行渲染产物。`spans` 用于显示（含 markdown/语法/theme 色），
/// `plain` 是逻辑纯文本（选中/复制用）。
#[derive(Clone, Debug, Default)]
pub struct RenderedLine {
    pub spans: Vec<Span<'static>>,
    pub plain: String,
}

impl RenderedLine {
    /// 从 spans 构造，`plain` 由 spans 可见文本拼接得到。
    pub fn new(spans: Vec<Span<'static>>) -> Self {
        let plain = spans.iter().map(|s| s.content.as_ref()).collect::<String>();
        Self { spans, plain }
    }

    /// 显式提供 plain（用于 markdown 等显示文本 ≠ 逻辑文本的场景）。
    pub fn with_plain(spans: Vec<Span<'static>>, plain: String) -> Self {
        Self { spans, plain }
    }
}

/// 一个 block 的渲染产物（多行）。
#[derive(Clone, Debug, Default)]
pub struct RenderedBlock {
    pub block_id: String,
    pub lines: Vec<RenderedLine>,
}

/// 整个输出文档的渲染产物（按 block 顺序）。
#[derive(Clone, Debug, Default)]
pub struct RenderedDocument {
    pub blocks: Vec<RenderedBlock>,
}

impl RenderedDocument {
    pub fn total_lines(&self) -> usize {
        self.blocks.iter().map(|b| b.lines.len()).sum()
    }

    pub fn iter_lines(&self) -> impl Iterator<Item = &RenderedLine> {
        self.blocks.iter().flat_map(|b| b.lines.iter())
    }
}
```

- [ ] **Step 4: 在 `output/mod.rs` 加模块声明**

```rust
pub mod rendered;
```

- [ ] **Step 5: 运行测试验证通过**

Run: `cargo test -p cli test_rendered_ -- --list && cargo test -p cli rendered::tests`
Expected: 3 tests PASS

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/tui/render/output/rendered.rs apps/cli/src/tui/render/output/mod.rs
git commit -m "feat(tui): 新增 RenderedLine/RenderedBlock/RenderedDocument/RenderCtx 值类型 (refs #58)"
```

### Task 1.2：`OutputBlockView` 重构为 `{block_id, block_version, kind}` + 补 Hash

**Files:**
- Modify: `apps/cli/src/tui/view_model/output.rs`
- Modify: `apps/cli/src/tui/view_model/style.rs`

- [ ] **Step 1: 给 `SemanticStyle` 补 `Hash`（`style.rs` 第 1 行 derive）**

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SemanticStyle { /* 不变 */ }
```

- [ ] **Step 2: 写失败测试（`output.rs` 末尾 tests 模块新增）**

```rust
#[test]
fn test_output_block_view_holds_id_version_kind() {
    let view = OutputBlockView {
        block_id: "u1".into(),
        block_version: 42,
        kind: OutputBlockKind::UserMessage(TextBlockView {
            key: "u1".into(), text: "hi".into(), style: SemanticStyle::Normal,
        }),
    };
    assert_eq!(view.block_id, "u1");
    assert_eq!(view.block_version, 42);
    assert!(matches!(view.kind, OutputBlockKind::UserMessage(_)));
}

#[test]
fn test_output_block_kind_has_queued_submission_variant() {
    let kind = OutputBlockKind::QueuedSubmission(TextBlockView {
        key: "q1".into(), text: "later".into(), style: SemanticStyle::Muted,
    });
    assert!(matches!(kind, OutputBlockKind::QueuedSubmission(_)));
}
```

- [ ] **Step 3: 运行验证失败**

Run: `cargo test -p cli test_output_block_view_holds_id_version_kind`
Expected: FAIL（`OutputBlockView` 仍是 enum / 无 `OutputBlockKind`）

- [ ] **Step 4: 重构 `output.rs`**

把原 `pub enum OutputBlockView { ... }` 改为：

```rust
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct OutputBlockView {
    pub block_id: String,
    pub block_version: u64,
    pub kind: OutputBlockKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum OutputBlockKind {
    UserMessage(TextBlockView),
    QueuedSubmission(TextBlockView),
    AssistantMessage(TextBlockView),
    ThinkingMessage(TextBlockView),
    ToolCall(ToolCallBlockView),
    DiagnosticNotice(TextBlockView),
    SystemNotice(TextBlockView),
    Separator,
}
```

并给 `TextBlockView`、`ToolCallBlockView`、`ToolSemanticStatus` 的 derive 追加 `Hash`。

- [ ] **Step 5: 修复编译错误（旧 `output_view_model.rs` 等对 `OutputBlockView::UserMessage(..)` 的匹配）**

`render/output_view_model.rs` 的 `block_lines` 仍需工作（Phase 5 才删它）。将其匹配从 `OutputBlockView::UserMessage(t)` 改为匹配 `block.kind` 上的 `OutputBlockKind::UserMessage(t)`，并把新增的 `QueuedSubmission` 暂按 `UserMessage` 同样处理（保持现状行为不变）：

```rust
pub(crate) fn output_view_model_lines(view_model: &OutputViewModel) -> Vec<Line<'static>> {
    view_model.blocks.iter().flat_map(|b| block_lines(&b.kind)).collect()
}

fn block_lines(kind: &OutputBlockKind) -> Vec<Line<'static>> {
    match kind {
        OutputBlockKind::UserMessage(t) | OutputBlockKind::QueuedSubmission(t) => text_lines(t),
        OutputBlockKind::AssistantMessage(t)
        | OutputBlockKind::ThinkingMessage(t)
        | OutputBlockKind::DiagnosticNotice(t)
        | OutputBlockKind::SystemNotice(t) => text_lines(t),
        OutputBlockKind::ToolCall(t) => tool_lines(t),
        OutputBlockKind::Separator => vec![Line::default()],
    }
}
```

- [ ] **Step 6: 运行验证通过 + 全包编译**

Run: `cargo test -p cli test_output_block_view_holds_id_version_kind test_output_block_kind_has_queued_submission_variant && cargo build -p cli`
Expected: PASS + 编译通过

- [ ] **Step 7: Commit**

```bash
git add apps/cli/src/tui/view_model/
git commit -m "feat(tui): OutputBlockView 改为带 block_id/version 的 struct，新增 QueuedSubmission 与 Hash (refs #58)"
```

### Task 1.3：ViewAssembler 产出 block_id/block_version，QueuedUserMessage → QueuedSubmission

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/output.rs`

- [ ] **Step 1: 写失败测试（`output.rs` tests 末尾）**

```rust
#[test]
fn test_assemble_assigns_stable_block_id_and_version_changes_on_content() {
    let mut conv = ConversationModel::default();
    let id = conv.push_user_message_for_test("hello"); // 见下方 helper 说明
    let vm1 = OutputViewAssembler::assemble_from_conversation(&conv, 1);
    let b1 = &vm1.blocks[0];
    assert_eq!(b1.block_id, id);
    let v1 = b1.block_version;

    // 内容不变 → 版本不变
    let vm2 = OutputViewAssembler::assemble_from_conversation(&conv, 2);
    assert_eq!(vm2.blocks[0].block_version, v1);
}

#[test]
fn test_assemble_maps_queued_user_message_to_queued_submission() {
    let mut conv = ConversationModel::default();
    conv.apply_for_test_queue("draft"); // queue_submission
    let vm = OutputViewAssembler::assemble_from_conversation(&conv, 1);
    assert!(vm.blocks.iter().any(|b| matches!(b.kind, OutputBlockKind::QueuedSubmission(_))));
    // 不再复用 UserMessage 承载排队项
    assert!(!vm.blocks.iter().any(|b| matches!(&b.kind, OutputBlockKind::UserMessage(t) if t.text.contains("排队中"))));
}
```

> helper 说明：若 `ConversationModel` 无现成 test 构造，复用现有公有 intent 入口（`apply`/`push_*`）。本 plan 不新增 test-only public API；用既有 `ConversationModel` API 构造（具体调用以现状 `model/conversation/model.rs` 公有方法为准）。

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p cli test_assemble_maps_queued_user_message_to_queued_submission`
Expected: FAIL

- [ ] **Step 3: 实现 —— 在 `output.rs` 新增 version 计算 helper**

```rust
use std::hash::{Hash, Hasher};

fn block_version(kind: &OutputBlockKind) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    kind.hash(&mut h);
    h.finish()
}

fn push_block(blocks: &mut Vec<OutputBlockView>, block_id: String, kind: OutputBlockKind) {
    let block_version = block_version(&kind);
    blocks.push(OutputBlockView { block_id, block_version, kind });
}
```

- [ ] **Step 4: 改造每个映射分支用 `push_block`**

把原先 `blocks.push(OutputBlockView::UserMessage(TextBlockView{...}))` 改为：

```rust
ConversationBlock::UserMessage { id, text } => {
    let view = TextBlockView { key: id.clone(), text: text.clone(), style: SemanticStyle::Normal };
    push_block(&mut blocks, id.clone(), OutputBlockKind::UserMessage(view));
}
```

`QueuedUserMessage` 分支改为 `QueuedSubmission`（去掉「排队中:」前缀，文本本身交给 renderer 加标记）：

```rust
ConversationBlock::QueuedUserMessage { id, text } => {
    let view = TextBlockView { key: id.clone(), text: text.clone(), style: SemanticStyle::Muted };
    push_block(&mut blocks, id.clone(), OutputBlockKind::QueuedSubmission(view));
}
```

`Separator` 用序号生成稳定 id：

```rust
// 进入循环前: let mut sep_seq = 0usize;
ConversationBlock::Separator { .. } => {
    push_block(&mut blocks, format!("sep-{sep_seq}"), OutputBlockKind::Separator);
    sep_seq += 1;
}
```

其余分支（AssistantText/Thinking/ToolCall/System/Error/AgentProgress/ToolResult/OrphanToolResult）同样用各自 `id`（ToolCall 用 `key`）调用 `push_block`，kind 不变。

- [ ] **Step 5: 运行验证通过**

Run: `cargo test -p cli test_assemble_assigns_stable_block_id_and_version_changes_on_content test_assemble_maps_queued_user_message_to_queued_submission`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/tui/view_assembler/output.rs
git commit -m "feat(tui): ViewAssembler 产出 block_id/block_version 并把排队输入映射为 QueuedSubmission (refs #58)"
```

### Task 1.4：block 级缓存 `BlockCache`

**Files:**
- Create: `apps/cli/src/tui/render/output/block_cache.rs`
- Modify: `apps/cli/src/tui/render/output/mod.rs`（加 `pub mod block_cache;`）

- [ ] **Step 1: 写失败测试（`block_cache.rs` 末尾）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::rendered::{RenderedBlock, RenderedLine};

    fn block(id: &str, n: usize) -> RenderedBlock {
        RenderedBlock { block_id: id.into(), lines: vec![RenderedLine::default(); n] }
    }

    #[test]
    fn test_cache_hit_when_key_unchanged() {
        let mut cache = BlockCache::default();
        let mut calls = 0;
        let key = CacheKey { version: 1, width: 80 };
        cache.get_or_render("a", key, |_| { calls += 1; block("a", 2) });
        cache.get_or_render("a", key, |_| { calls += 1; block("a", 2) });
        assert_eq!(calls, 1, "同 key 第二次应命中缓存，不再渲染");
    }

    #[test]
    fn test_cache_miss_when_version_changes() {
        let mut cache = BlockCache::default();
        let mut calls = 0;
        cache.get_or_render("a", CacheKey { version: 1, width: 80 }, |_| { calls += 1; block("a", 1) });
        cache.get_or_render("a", CacheKey { version: 2, width: 80 }, |_| { calls += 1; block("a", 1) });
        assert_eq!(calls, 2, "version 变应重渲染");
    }

    #[test]
    fn test_retain_evicts_absent_blocks() {
        let mut cache = BlockCache::default();
        cache.get_or_render("a", CacheKey { version: 1, width: 80 }, |_| block("a", 1));
        cache.get_or_render("b", CacheKey { version: 1, width: 80 }, |_| block("b", 1));
        cache.retain(&["a".to_string()]);
        assert!(cache.contains("a"));
        assert!(!cache.contains("b"), "ViewModel 中不存在的 block 应被清除防泄漏");
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p cli test_cache_hit_when_key_unchanged`
Expected: FAIL

- [ ] **Step 3: 实现 `block_cache.rs`**

```rust
//! block 级渲染缓存：key=(block_version,width)，命中复用，未命中重渲。
use std::collections::HashMap;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheKey {
    pub version: u64,
    pub width: u16,
}

struct CachedBlock {
    key: CacheKey,
    rendered: RenderedBlock,
}

#[derive(Default)]
pub struct BlockCache {
    map: HashMap<String, CachedBlock>,
}

impl BlockCache {
    /// 命中(key 一致)直接返回缓存 clone；否则调用 `render` 重渲染并缓存。
    pub fn get_or_render(
        &mut self,
        block_id: &str,
        key: CacheKey,
        render: impl FnOnce(&RenderCtx) -> RenderedBlock,
    ) -> RenderedBlock {
        if let Some(c) = self.map.get(block_id) {
            if c.key == key {
                return c.rendered.clone();
            }
        }
        let ctx = RenderCtx { width: key.width };
        let rendered = render(&ctx);
        self.map.insert(block_id.to_string(), CachedBlock { key, rendered: rendered.clone() });
        rendered
    }

    /// 清除不在 `live_ids` 中的缓存条目（防内存泄漏）。
    pub fn retain(&mut self, live_ids: &[String]) {
        self.map.retain(|id, _| live_ids.iter().any(|x| x == id));
    }

    pub fn contains(&self, block_id: &str) -> bool {
        self.map.contains_key(block_id)
    }
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test -p cli block_cache::tests`
Expected: 3 PASS

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/render/output/block_cache.rs apps/cli/src/tui/render/output/mod.rs
git commit -m "feat(tui): 新增 block 级渲染缓存 BlockCache (refs #58)"
```

### Task 1.5：`OutputDocumentRenderer` 骨架（空分发）

**Files:**
- Create: `apps/cli/src/tui/render/output/document_renderer.rs`
- Create: `apps/cli/src/tui/render/output/blocks/mod.rs`
- Modify: `apps/cli/src/tui/render/output/mod.rs`

- [ ] **Step 1: 写失败测试（`document_renderer.rs` 末尾）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::view_model::output::{OutputBlockKind, OutputBlockView, OutputViewModel, TextBlockView};
    use crate::tui::view_model::style::SemanticStyle;

    fn vm_with(kind: OutputBlockKind, id: &str) -> OutputViewModel {
        OutputViewModel {
            blocks: vec![OutputBlockView { block_id: id.into(), block_version: 1, kind }],
            version: 1,
            follow_tail_hint: true,
        }
    }

    #[test]
    fn test_renderer_emits_one_block_per_view() {
        let mut r = OutputDocumentRenderer::default();
        let vm = vm_with(
            OutputBlockKind::SystemNotice(TextBlockView { key: "s".into(), text: "ok".into(), style: SemanticStyle::Muted }),
            "s",
        );
        let doc = r.render(&vm, 80);
        assert_eq!(doc.blocks.len(), 1);
        assert_eq!(doc.blocks[0].block_id, "s");
    }

    #[test]
    fn test_renderer_caches_unchanged_block() {
        let mut r = OutputDocumentRenderer::default();
        let vm = vm_with(
            OutputBlockKind::SystemNotice(TextBlockView { key: "s".into(), text: "ok".into(), style: SemanticStyle::Muted }),
            "s",
        );
        let _ = r.render(&vm, 80);
        let _ = r.render(&vm, 80);
        assert_eq!(r.render_count(), 1, "同 version+width 第二次应命中缓存");
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p cli test_renderer_emits_one_block_per_view`
Expected: FAIL

- [ ] **Step 3: 实现 `blocks/mod.rs`（骨架占位，Phase 3 填具体组件）**

```rust
//! 顶层 block 级渲染组件。每个组件 fn(view, ctx) -> RenderedBlock。
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::view_model::output::OutputBlockKind;

/// Phase 1 占位实现：把 block 的语义文本按 plain 单行输出（无 markdown/theme）。
/// Phase 3 各组件会逐个替换此分发到真实组件。
pub fn render_block(kind: &OutputBlockKind, block_id: &str, _ctx: &RenderCtx) -> RenderedBlock {
    use ratatui::text::Span;
    let text = match kind {
        OutputBlockKind::UserMessage(t)
        | OutputBlockKind::QueuedSubmission(t)
        | OutputBlockKind::AssistantMessage(t)
        | OutputBlockKind::ThinkingMessage(t)
        | OutputBlockKind::DiagnosticNotice(t)
        | OutputBlockKind::SystemNotice(t) => t.text.clone(),
        OutputBlockKind::ToolCall(t) => t.title.clone(),
        OutputBlockKind::Separator => String::new(),
    };
    let lines = text.lines().map(|l| RenderedLine::new(vec![Span::raw(l.to_string())])).collect();
    RenderedBlock { block_id: block_id.to_string(), lines }
}
```

- [ ] **Step 4: 实现 `document_renderer.rs`**

```rust
//! 输出文档渲染器：遍历 ViewModel.blocks，经 block 级缓存产出 RenderedDocument。
use crate::tui::render::output::block_cache::{BlockCache, CacheKey};
use crate::tui::render::output::blocks::render_block;
use crate::tui::render::output::rendered::RenderedDocument;
use crate::tui::view_model::output::OutputViewModel;

#[derive(Default)]
pub struct OutputDocumentRenderer {
    cache: BlockCache,
    #[cfg(test)]
    render_count: std::cell::Cell<usize>,
}

impl OutputDocumentRenderer {
    pub fn render(&mut self, vm: &OutputViewModel, width: u16) -> RenderedDocument {
        let mut blocks = Vec::with_capacity(vm.blocks.len());
        let live: Vec<String> = vm.blocks.iter().map(|b| b.block_id.clone()).collect();
        for view in &vm.blocks {
            let key = CacheKey { version: view.block_version, width };
            let kind = &view.kind;
            let id = view.block_id.clone();
            let id_for_render = id.clone();
            #[cfg(test)]
            let counter = &self.render_count;
            let rendered = self.cache.get_or_render(&id, key, |ctx| {
                #[cfg(test)]
                counter.set(counter.get() + 1);
                render_block(kind, &id_for_render, ctx)
            });
            blocks.push(rendered);
        }
        self.cache.retain(&live);
        RenderedDocument { blocks }
    }

    #[cfg(test)]
    pub fn render_count(&self) -> usize {
        self.render_count.get()
    }
}
```

- [ ] **Step 5: 在 `output/mod.rs` 加模块**

```rust
pub mod blocks;
pub mod document_renderer;
```

- [ ] **Step 6: 运行验证通过**

Run: `cargo test -p cli document_renderer::tests && cargo build -p cli`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add apps/cli/src/tui/render/output/document_renderer.rs apps/cli/src/tui/render/output/blocks/ apps/cli/src/tui/render/output/mod.rs
git commit -m "feat(tui): OutputDocumentRenderer 骨架 + block 占位分发（不接线） (refs #58)"
```

---

## Phase 2：共享原语 primitives（产出 RenderedLine）

把现有产出 ratatui `Line`/`OutputLine`+`SpanPart` 的富渲染逻辑，包装为统一产出 `Vec<RenderedLine>` 的纯函数。先有单测。

### Task 2.1：`SpanPart -> Vec<Span>` 与 `spans -> plain` 转换 helper

**Files:**
- Create: `apps/cli/src/tui/render/output/primitives/mod.rs`
- Create: `apps/cli/src/tui/render/output/primitives/convert.rs`
- Modify: `apps/cli/src/tui/render/output/mod.rs`（加 `pub mod primitives;`）

- [ ] **Step 1: 写失败测试（`convert.rs` 末尾）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;
    use crate::tui::render::output_area::types::SpanPart;

    #[test]
    fn test_spanparts_to_spans_preserves_text_and_color() {
        let parts = vec![SpanPart::plain("ab", Color::Red), SpanPart::plain("c", Color::Blue)];
        let spans = spanparts_to_spans(&parts);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content.as_ref(), "ab");
        assert_eq!(spans[0].style.fg, Some(Color::Red));
    }

    #[test]
    fn test_rendered_line_from_spanparts_sets_plain() {
        let parts = vec![SpanPart::plain("  - ", Color::Red), SpanPart::plain("x", Color::Red)];
        let line = rendered_line_from_spanparts(&parts);
        assert_eq!(line.plain, "  - x");
    }
}
```

- [ ] **Step 2: 验证失败**

Run: `cargo test -p cli test_spanparts_to_spans_preserves_text_and_color`
Expected: FAIL

- [ ] **Step 3: 实现 `convert.rs`**

```rust
//! SpanPart(现有 diff/syntax 着色单元) 与 RenderedLine 互转。
use ratatui::style::Style;
use ratatui::text::Span;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::output_area::types::SpanPart;

pub fn spanparts_to_spans(parts: &[SpanPart]) -> Vec<Span<'static>> {
    parts
        .iter()
        .map(|p| Span::styled(p.text.clone(), Style::default().fg(p.color)))
        .collect()
}

pub fn rendered_line_from_spanparts(parts: &[SpanPart]) -> RenderedLine {
    RenderedLine::new(spanparts_to_spans(parts))
}
```

`primitives/mod.rs`：

```rust
mod convert;
pub mod markdown;
pub mod diff;
pub mod table;
pub use convert::{rendered_line_from_spanparts, spanparts_to_spans};
```

> 注：此 Task 先只建 `convert`；`markdown`/`diff`/`table` 子模块在 2.2/2.3/2.4 创建。本步把 `pub mod markdown/diff/table;` 暂注释，随后续 Task 解开。

- [ ] **Step 4: 验证通过**

Run: `cargo test -p cli convert::tests`
Expected: 2 PASS

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/render/output/primitives/ apps/cli/src/tui/render/output/mod.rs
git commit -m "feat(tui): primitives SpanPart<->RenderedLine 转换 helper (refs #58)"
```

### Task 2.2：markdown 原语 `markdown(text, base_style, width) -> Vec<RenderedLine>`

**Files:**
- Create: `apps/cli/src/tui/render/output/primitives/markdown.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Modifier, Style};

    #[test]
    fn test_markdown_bold_sets_modifier_and_plain_strips_markers() {
        let lines = markdown("a **b** c", Style::default(), 80);
        assert_eq!(lines.len(), 1);
        // 不变式：plain 是去标记后的可见文本
        assert_eq!(lines[0].plain, "a b c");
        // 显示 spans 中 "b" 带粗体
        assert!(lines[0].spans.iter().any(|s| s.content.as_ref() == "b"
            && s.style.add_modifier.contains(Modifier::BOLD)));
    }

    #[test]
    fn test_markdown_wraps_by_width() {
        let lines = markdown("aaaa bbbb", Style::default(), 4);
        assert!(lines.len() >= 2, "超宽应换行");
    }

    #[test]
    fn test_markdown_plain_invariant_matches_spans_visible_text() {
        let lines = markdown("`code` and *em*", Style::default(), 80);
        for l in &lines {
            let visible: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
            // markdown 的 plain 用 strip_inline_formatting，可能与显示可见文本不同，
            // 但显示可见文本必须等于「按显示拼接」本身（自洽）
            assert_eq!(visible, l.spans.iter().map(|s| s.content.as_ref()).collect::<String>());
            assert!(!l.plain.contains('`'));
        }
    }
}
```

- [ ] **Step 2: 验证失败** — Run: `cargo test -p cli test_markdown_bold_sets_modifier_and_plain_strips_markers` → FAIL

- [ ] **Step 3: 实现 `markdown.rs`**

```rust
//! markdown 原语：解析 inline markdown -> 显示 spans，按宽度换行；plain 去标记。
use ratatui::style::Style;
use crate::tui::render::output::markdown as md; // 现有 inline_markdown_lines / strip_inline_formatting
use crate::tui::render::output::rendered::RenderedLine;

pub fn markdown(text: &str, base_style: Style, width: u16) -> Vec<RenderedLine> {
    let lines = md::inline_markdown_lines(text, base_style, width as usize);
    lines
        .into_iter()
        .map(|line| {
            let visible: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            // 对单行的可见文本再 strip 一次，得到逻辑 plain（复制用，无标记）
            let plain = md::strip_inline_formatting(&visible);
            RenderedLine::with_plain(line.spans, plain)
        })
        .collect()
}
```

> 注：`inline_markdown_lines` 已先解析再换行（见现状实现），因此每个 `Line.spans` 已是该屏幕行的显示单元；`visible` 即该行显示文本，对其 strip 得逻辑文本。

- [ ] **Step 4: 在 `primitives/mod.rs` 解开 `pub mod markdown;`**

- [ ] **Step 5: 验证通过** — Run: `cargo test -p cli primitives::markdown::tests` → PASS

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/tui/render/output/primitives/
git commit -m "feat(tui): markdown 原语产出 RenderedLine(spans+plain) (refs #58)"
```

### Task 2.3：diff 原语 `diff(old, new, ext, width) -> Vec<RenderedLine>`（修 #61 行号缩进 + 选区基础）

**Files:**
- Create: `apps/cli/src/tui/render/output/primitives/diff.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_emits_add_remove_with_color_and_plain() {
        let lines = diff("a\nb\n", "a\nc\n", Some("rs"), 80);
        let plains: Vec<&str> = lines.iter().map(|l| l.plain.as_str()).collect();
        assert!(plains.iter().any(|p| p.contains('-') && p.contains('b')), "应含删除行 b");
        assert!(plains.iter().any(|p| p.contains('+') && p.contains('c')), "应含新增行 c");
        // 至少一行带颜色 span（语义色）
        assert!(lines.iter().any(|l| l.spans.iter().any(|s| s.style.fg.is_some())));
    }

    #[test]
    fn test_diff_line_keeps_left_indent_not_flush_left() {
        // 修 #61：行号区不得贴最左，需保留输出区缩进
        let lines = diff("x\n", "y\n", None, 80);
        assert!(lines.iter().all(|l| l.plain.starts_with("  ")), "每行应保留两空格缩进");
    }
}
```

- [ ] **Step 2: 验证失败** — Run: `cargo test -p cli test_diff_emits_add_remove_with_color_and_plain` → FAIL

- [ ] **Step 3: 实现 `diff.rs`**

```rust
//! diff 原语：复用现有 build_diff_lines(产出 OutputLine+SpanPart) 转 RenderedLine。
use crate::tui::render::output::diff::build_diff_lines;
use crate::tui::render::output::primitives::convert::rendered_line_from_spanparts;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::output_area::types::OutputLine;

pub fn diff(old: &str, new: &str, ext: Option<&str>, _width: u16) -> Vec<RenderedLine> {
    let mut out: Vec<OutputLine> = Vec::new();
    build_diff_lines(old, new, ext, &None, &mut out);
    out.into_iter()
        .map(|ol| match ol.spans {
            Some(parts) => rendered_line_from_spanparts(&parts),
            None => RenderedLine::new(vec![ratatui::text::Span::raw(ol.content)]),
        })
        .collect()
}
```

> 注：`build_diff_lines` 现状已含两空格 `INDENT` 前缀（见 `diff.rs:build_delete_line`），#61「贴最左」根因在旧 OutputArea 渲染路径，本管线直接用 SpanPart 即保留缩进；测试断言锁死该不变式防回归。

- [ ] **Step 4: `primitives/mod.rs` 解开 `pub mod diff;`**

- [ ] **Step 5: 验证通过** — Run: `cargo test -p cli primitives::diff::tests` → PASS

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/tui/render/output/primitives/
git commit -m "feat(tui): diff 原语产出 RenderedLine，锁定行号缩进不变式 (refs #58, refs #61)"
```

### Task 2.4：table 原语 `table(lines, base_style, width) -> Vec<RenderedLine>`

**Files:**
- Create: `apps/cli/src/tui/render/output/primitives/table.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    #[test]
    fn test_table_renders_rows_with_aligned_plain() {
        let src = ["| a | bb |", "|---|----|", "| 1 | 2 |"];
        let lines = table(&src, Style::default(), 40);
        assert!(!lines.is_empty());
        // 每行 plain 含列分隔
        assert!(lines.iter().any(|l| l.plain.contains('│') || l.plain.contains('|')));
    }
}
```

- [ ] **Step 2: 验证失败** → FAIL

- [ ] **Step 3: 实现 `table.rs`**

```rust
//! table 原语：复用现有 render_table_block(产出 Vec<Vec<Span>>) 转 RenderedLine。
use ratatui::style::Style;
use crate::tui::render::output::markdown::render_table_block;
use crate::tui::render::output::rendered::RenderedLine;

pub fn table(src_lines: &[&str], base_style: Style, width: u16) -> Vec<RenderedLine> {
    render_table_block(src_lines, base_style, width as usize)
        .into_iter()
        .map(RenderedLine::new)
        .collect()
}
```

- [ ] **Step 4: `primitives/mod.rs` 解开 `pub mod table;`**

- [ ] **Step 5: 验证通过** — Run: `cargo test -p cli primitives::table::tests` → PASS

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/tui/render/output/primitives/
git commit -m "feat(tui): table 原语产出 RenderedLine (refs #58)"
```

---

## Phase 3：逐 block 组件切换

每个 Task 实现一个真实 BlockRenderer 组件并接入 `blocks/render_block` 分发；每切完一个 block 类型即从占位分支移除。每步独立编译+测试。

> **顺序**（spec §114）：Separator/System/Diagnostic → UserMessage → QueuedSubmission → AssistantMessage（恢复 markdown+theme）→ Thinking → ToolCall。

### Task 3.1：Separator / System / Diagnostic 组件

**Files:**
- Create: `apps/cli/src/tui/render/output/blocks/separator.rs`
- Create: `apps/cli/src/tui/render/output/blocks/diagnostic.rs`
- Modify: `apps/cli/src/tui/render/output/blocks/mod.rs`

- [ ] **Step 1: 写失败测试（`diagnostic.rs` 末尾）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::rendered::RenderCtx;
    use crate::tui::view_model::output::TextBlockView;
    use crate::tui::view_model::style::SemanticStyle;

    #[test]
    fn test_diagnostic_error_uses_error_color() {
        let v = TextBlockView { key: "e".into(), text: "boom".into(), style: SemanticStyle::Error };
        let b = render_diagnostic("e", &v, &RenderCtx { width: 80 });
        assert_eq!(b.lines[0].plain, "boom");
        assert_eq!(b.lines[0].spans[0].style.fg, Some(crate::tui::render::theme::ERROR));
    }

    #[test]
    fn test_separator_emits_blank_line() {
        let b = render_separator("sep-0");
        assert_eq!(b.lines.len(), 1);
        assert_eq!(b.lines[0].plain, "");
    }
}
```

- [ ] **Step 2: 验证失败** → FAIL

- [ ] **Step 3: 实现 `separator.rs`**

```rust
use crate::tui::render::output::rendered::{RenderedBlock, RenderedLine};

pub fn render_separator(block_id: &str) -> RenderedBlock {
    RenderedBlock { block_id: block_id.to_string(), lines: vec![RenderedLine::default()] }
}
```

`diagnostic.rs`（System/Diagnostic 共用，按 SemanticStyle 取色，逐行纯文本；不解析 markdown 以隔离 #65/#74 样式泄漏）：

```rust
use ratatui::style::Style;
use ratatui::text::Span;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use crate::tui::view_model::style::SemanticStyle;

pub fn semantic_color(style: SemanticStyle) -> ratatui::style::Color {
    match style {
        SemanticStyle::Normal => theme::TEXT,
        SemanticStyle::Muted => theme::TEXT_MUTED,
        SemanticStyle::Running => theme::TOOL_RUNNING,
        SemanticStyle::Success => theme::SUCCESS,
        SemanticStyle::Error => theme::ERROR,
        SemanticStyle::Warning => theme::WARNING,
        SemanticStyle::Accent => theme::ACCENT,
    }
}

pub fn render_diagnostic(block_id: &str, view: &TextBlockView, _ctx: &RenderCtx) -> RenderedBlock {
    let style = Style::default().fg(semantic_color(view.style));
    let lines = view
        .text
        .lines()
        .map(|l| RenderedLine::new(vec![Span::styled(l.to_string(), style)]))
        .collect();
    RenderedBlock { block_id: block_id.to_string(), lines }
}
```

- [ ] **Step 4: 接入 `blocks/mod.rs` 分发（替换占位的这三类分支）**

```rust
pub mod separator;
pub mod diagnostic;
// ... Phase 3 后续 pub mod 陆续加入

pub fn render_block(kind: &OutputBlockKind, block_id: &str, ctx: &RenderCtx) -> RenderedBlock {
    match kind {
        OutputBlockKind::Separator => separator::render_separator(block_id),
        OutputBlockKind::SystemNotice(t) | OutputBlockKind::DiagnosticNotice(t) =>
            diagnostic::render_diagnostic(block_id, t, ctx),
        // 其余仍走占位（下方保留 fallback 直到 Phase 3 全切完）
        other => render_placeholder(other, block_id, ctx),
    }
}
```

把原占位实现重命名为 `render_placeholder` 保留为 fallback。

- [ ] **Step 5: 验证通过** — Run: `cargo test -p cli blocks::diagnostic::tests blocks::separator` → PASS（`cargo build -p cli` 通过）

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/tui/render/output/blocks/
git commit -m "feat(tui): Separator/System/Diagnostic block 组件 (refs #58)"
```

### Task 3.2：UserMessage 组件（`> ` 前缀 + USER 色）

**Files:**
- Create: `apps/cli/src/tui/render/output/blocks/user_message.rs`
- Modify: `apps/cli/src/tui/render/output/blocks/mod.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::rendered::RenderCtx;
    use crate::tui::view_model::output::TextBlockView;
    use crate::tui::view_model::style::SemanticStyle;

    #[test]
    fn test_user_message_prefixes_gt_and_uses_user_color() {
        let v = TextBlockView { key: "u".into(), text: "hello".into(), style: SemanticStyle::Normal };
        let b = render_user_message("u", &v, &RenderCtx { width: 80 });
        assert_eq!(b.lines[0].plain, "> hello");
        assert_eq!(b.lines[0].spans[0].style.fg, Some(crate::tui::render::theme::USER));
    }

    #[test]
    fn test_user_message_multiline_indents_continuation() {
        let v = TextBlockView { key: "u".into(), text: "a\nb".into(), style: SemanticStyle::Normal };
        let b = render_user_message("u", &v, &RenderCtx { width: 80 });
        assert_eq!(b.lines[0].plain, "> a");
        assert_eq!(b.lines[1].plain, "  b");
    }
}
```

- [ ] **Step 2: 验证失败** → FAIL

- [ ] **Step 3: 实现 `user_message.rs`**

```rust
use ratatui::style::Style;
use ratatui::text::Span;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;

pub fn render_user_message(block_id: &str, view: &TextBlockView, _ctx: &RenderCtx) -> RenderedBlock {
    let style = Style::default().fg(theme::USER);
    let mut lines = Vec::new();
    for (i, l) in view.text.lines().enumerate() {
        let prefix = if i == 0 { "> " } else { "  " };
        lines.push(RenderedLine::new(vec![Span::styled(format!("{prefix}{l}"), style)]));
    }
    if lines.is_empty() {
        lines.push(RenderedLine::new(vec![Span::styled("> ".to_string(), style)]));
    }
    RenderedBlock { block_id: block_id.to_string(), lines }
}
```

- [ ] **Step 4: 接入分发**

`blocks/mod.rs` match 加 `OutputBlockKind::UserMessage(t) => user_message::render_user_message(block_id, t, ctx),`，并从 `render_placeholder` 移除 UserMessage。

- [ ] **Step 5: 验证通过** — Run: `cargo test -p cli blocks::user_message::tests` → PASS

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/tui/render/output/blocks/
git commit -m "feat(tui): UserMessage block 组件 (refs #58)"
```

### Task 3.3：QueuedSubmission 组件（暗色 + 「排队中」标记）

**Files:**
- Create: `apps/cli/src/tui/render/output/blocks/queued_submission.rs`
- Modify: `apps/cli/src/tui/render/output/blocks/mod.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::rendered::RenderCtx;
    use crate::tui::view_model::output::TextBlockView;
    use crate::tui::view_model::style::SemanticStyle;

    #[test]
    fn test_queued_submission_marks_and_dims() {
        let v = TextBlockView { key: "q".into(), text: "draft".into(), style: SemanticStyle::Muted };
        let b = render_queued_submission("q", &v, &RenderCtx { width: 80 });
        assert!(b.lines[0].plain.contains("draft"));
        assert!(b.lines[0].plain.contains("排队中"));
        assert_eq!(b.lines[0].spans[0].style.fg, Some(crate::tui::render::theme::TEXT_DIM));
    }
}
```

- [ ] **Step 2: 验证失败** → FAIL

- [ ] **Step 3: 实现 `queued_submission.rs`**

```rust
use ratatui::style::Style;
use ratatui::text::Span;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;

pub fn render_queued_submission(block_id: &str, view: &TextBlockView, _ctx: &RenderCtx) -> RenderedBlock {
    let style = Style::default().fg(theme::TEXT_DIM);
    let mut lines = Vec::new();
    for (i, l) in view.text.lines().enumerate() {
        let text = if i == 0 { format!("⏳ 排队中: {l}") } else { format!("   {l}") };
        lines.push(RenderedLine::new(vec![Span::styled(text, style)]));
    }
    if lines.is_empty() {
        lines.push(RenderedLine::new(vec![Span::styled("⏳ 排队中: ".to_string(), style)]));
    }
    RenderedBlock { block_id: block_id.to_string(), lines }
}
```

- [ ] **Step 4: 接入分发** —`blocks/mod.rs` 加 `OutputBlockKind::QueuedSubmission(t) => queued_submission::render_queued_submission(block_id, t, ctx),`，从 `render_placeholder` 移除。

- [ ] **Step 5: 验证通过** — Run: `cargo test -p cli blocks::queued_submission::tests` → PASS

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/tui/render/output/blocks/
git commit -m "feat(tui): QueuedSubmission block 组件，排队输入独立样式 (refs #58)"
```

### Task 3.4：AssistantMessage 组件（**恢复 markdown + theme**，含 fence/table/diff）

**Files:**
- Create: `apps/cli/src/tui/render/output/blocks/assistant_message.rs`
- Modify: `apps/cli/src/tui/render/output/blocks/mod.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;
    use crate::tui::render::output::rendered::RenderCtx;
    use crate::tui::view_model::output::TextBlockView;
    use crate::tui::view_model::style::SemanticStyle;

    fn render(text: &str) -> crate::tui::render::output::rendered::RenderedBlock {
        let v = TextBlockView { key: "a".into(), text: text.into(), style: SemanticStyle::Normal };
        render_assistant_message("a", &v, &RenderCtx { width: 80 })
    }

    #[test]
    fn test_assistant_renders_markdown_bold() {
        let b = render("see **this**");
        assert!(b.lines.iter().any(|l| l.spans.iter().any(|s|
            s.content.as_ref() == "this" && s.style.add_modifier.contains(Modifier::BOLD))));
        assert!(b.lines.iter().any(|l| l.plain.contains("see this")));
    }

    #[test]
    fn test_assistant_base_color_is_assistant_theme() {
        let b = render("plain text");
        assert_eq!(b.lines[0].spans[0].style.fg, Some(crate::tui::render::theme::ASSISTANT));
    }

    #[test]
    fn test_assistant_fence_does_not_leak_style_after_close() {
        // 回归 #65：fenced code block 结束后普通行不应继续 code 色
        let b = render("```\ncode\n```\nafter");
        let after = b.lines.last().unwrap();
        assert_eq!(after.plain, "after");
        assert_ne!(after.spans[0].style.fg, Some(crate::tui::render::theme::CODE));
    }
}
```

- [ ] **Step 2: 验证失败** → FAIL

- [ ] **Step 3: 实现 `assistant_message.rs`**

按行扫描，识别 fenced code block / table / 普通 markdown 三态，每段调对应 primitive，state 在 block 内维护、block 结束即销毁（天然隔离 #65/#74 跨 block 泄漏）：

```rust
use ratatui::style::Style;
use ratatui::text::Span;
use crate::tui::render::output::primitives::{markdown::markdown, table::table};
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::output::markdown::{is_table_row, is_table_separator};
use crate::tui::render::{syntax, theme};

pub fn render_assistant_message(block_id: &str, view: &super::TextBlockViewRef, ctx: &RenderCtx) -> RenderedBlock {
    let base = Style::default().fg(theme::ASSISTANT);
    let mut lines: Vec<RenderedLine> = Vec::new();
    let src: Vec<&str> = view.text.lines().collect();
    let mut i = 0;
    let mut in_fence = false;
    let mut fence_lang: Option<String> = None;
    while i < src.len() {
        let line = src[i];
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if in_fence {
                in_fence = false;
                fence_lang = None;
            } else {
                in_fence = true;
                fence_lang = Some(trimmed.trim_start_matches('`').trim().to_string());
            }
            // fence 标记行本身按弱化色单行输出
            lines.push(RenderedLine::new(vec![Span::styled(line.to_string(), Style::default().fg(theme::TEXT_DIM))]));
            i += 1;
            continue;
        }
        if in_fence {
            // 代码行：语法高亮（按 fence_lang）
            let syntax_ref = fence_lang.as_deref().and_then(syntax::language_by_extension);
            if let Some(parts) = syntax::highlight_line(line, syntax_ref.as_ref()) {
                lines.push(crate::tui::render::output::primitives::rendered_line_from_spanparts(&parts));
            } else {
                lines.push(RenderedLine::new(vec![Span::styled(line.to_string(), Style::default().fg(theme::CODE))]));
            }
            i += 1;
            continue;
        }
        // table 检测：当前行是表行且下一行是分隔行
        if is_table_row(line) && i + 1 < src.len() && is_table_separator(src[i + 1]) {
            let mut j = i;
            while j < src.len() && is_table_row(src[j]) { j += 1; }
            let block_src: Vec<&str> = src[i..j].to_vec();
            lines.extend(table(&block_src, base, ctx.width));
            i = j;
            continue;
        }
        // 普通 markdown 行
        lines.extend(markdown(line, base, ctx.width));
        i += 1;
    }
    if lines.is_empty() {
        lines.push(RenderedLine::default());
    }
    RenderedBlock { block_id: block_id.to_string(), lines }
}
```

> 说明：`TextBlockViewRef` 仅示意类型名；实际签名用 `view: &crate::tui::view_model::output::TextBlockView`。请在实现时用真实类型，删除示意别名。

- [ ] **Step 4: 接入分发** — `blocks/mod.rs` 加 `OutputBlockKind::AssistantMessage(t) => assistant_message::render_assistant_message(block_id, t, ctx),` 并从 placeholder 移除。

- [ ] **Step 5: 验证通过** — Run: `cargo test -p cli blocks::assistant_message::tests` → PASS

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/tui/render/output/blocks/
git commit -m "feat(tui): AssistantMessage block 组件，恢复 markdown+theme，隔离 fence 样式 (refs #58, refs #65)"
```

### Task 3.5：Thinking 组件（`💭` 前缀 + THINKING 色）

**Files:**
- Create: `apps/cli/src/tui/render/output/blocks/thinking.rs`
- Modify: `apps/cli/src/tui/render/output/blocks/mod.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::rendered::RenderCtx;
    use crate::tui::view_model::output::TextBlockView;
    use crate::tui::view_model::style::SemanticStyle;

    #[test]
    fn test_thinking_prefixes_bulb_and_thinking_color() {
        let v = TextBlockView { key: "t".into(), text: "ponder".into(), style: SemanticStyle::Muted };
        let b = render_thinking("t", &v, &RenderCtx { width: 80 });
        assert!(b.lines[0].plain.starts_with("💭"));
        assert_eq!(b.lines[0].spans[0].style.fg, Some(crate::tui::render::theme::THINKING));
    }

    #[test]
    fn test_thinking_skips_blank_lines() {
        // 回归 context #10741：thinking 不应渲染空白行
        let v = TextBlockView { key: "t".into(), text: "a\n\n\nb".into(), style: SemanticStyle::Muted };
        let b = render_thinking("t", &v, &RenderCtx { width: 80 });
        assert!(b.lines.iter().all(|l| !l.plain.trim().is_empty()));
    }
}
```

- [ ] **Step 2: 验证失败** → FAIL

- [ ] **Step 3: 实现 `thinking.rs`**

```rust
use ratatui::style::Style;
use ratatui::text::Span;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;

pub fn render_thinking(block_id: &str, view: &TextBlockView, _ctx: &RenderCtx) -> RenderedBlock {
    let style = Style::default().fg(theme::THINKING);
    let mut lines = Vec::new();
    let mut first = true;
    for l in view.text.lines() {
        if l.trim().is_empty() { continue; }
        let prefix = if first { "💭 " } else { "   " };
        lines.push(RenderedLine::new(vec![Span::styled(format!("{prefix}{l}"), style)]));
        first = false;
    }
    if lines.is_empty() {
        lines.push(RenderedLine::new(vec![Span::styled("💭 ".to_string(), style)]));
    }
    RenderedBlock { block_id: block_id.to_string(), lines }
}
```

- [ ] **Step 4: 接入分发** — 加分支，从 placeholder 移除。

- [ ] **Step 5: 验证通过** — Run: `cargo test -p cli blocks::thinking::tests` → PASS

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/tui/render/output/blocks/
git commit -m "feat(tui): Thinking block 组件 (refs #58)"
```

### Task 3.6：ToolCall 组件（复用 `tool_display`，修 #62 标题可见）

**Files:**
- Create: `apps/cli/src/tui/render/output/blocks/tool_call.rs`
- Modify: `apps/cli/src/tui/render/output/blocks/mod.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::rendered::RenderCtx;
    use crate::tui::view_model::output::{ToolCallBlockView, ToolSemanticStatus};
    use crate::tui::view_model::style::SemanticStyle;

    fn tool(status: ToolSemanticStatus) -> ToolCallBlockView {
        ToolCallBlockView {
            key: "t1".into(), chat_id: None, turn_id: None, tool_call_id: Some("t1".into()),
            title: "Grep".into(), icon: "●".into(), semantic_status: status, style: SemanticStyle::Running,
            args_preview: Some("/foo/".into()), summary: None, activity_summary: None,
            result_summary: None, collapsible: false, collapsed: false,
        }
    }

    #[test]
    fn test_tool_call_title_visible_not_background_color() {
        // 回归 #62：running 态 tool name 前景色不得等于背景色
        let b = render_tool_call("t1", &tool(ToolSemanticStatus::Running), &RenderCtx { width: 80 });
        let title_span = b.lines[0].spans.iter().find(|s| s.content.as_ref().contains("Grep")).unwrap();
        assert_ne!(title_span.style.fg, Some(crate::tui::render::theme::SURFACE));
        assert_ne!(title_span.style.fg, title_span.style.bg);
        assert!(b.lines[0].plain.contains("Grep"));
    }

    #[test]
    fn test_tool_call_success_uses_success_icon_color() {
        let b = render_tool_call("t1", &tool(ToolSemanticStatus::Success), &RenderCtx { width: 80 });
        assert!(b.lines[0].plain.contains("Grep"));
    }
}
```

- [ ] **Step 2: 验证失败** → FAIL

- [ ] **Step 3: 实现 `tool_call.rs`**

复用现有 `render/output/tool_display` 的格式化逻辑生成行与 span（具体函数以现状 `tool_display` 公有 API 为准），将其产出转为 `RenderedLine`。核心要求：tool name span 显式用 `theme::TEXT` 或对应语义强调色（**NEVER** 用背景色），锁死 #62。骨架：

```rust
use ratatui::style::Style;
use ratatui::text::Span;
use crate::tui::render::output::blocks::diagnostic::semantic_color;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::ToolCallBlockView;

pub fn render_tool_call(block_id: &str, view: &ToolCallBlockView, ctx: &RenderCtx) -> RenderedBlock {
    let icon_color = semantic_color(view.style);
    // header: <icon> <Title> <args_preview>
    let mut header: Vec<Span<'static>> = vec![
        Span::styled(format!("{} ", view.icon), Style::default().fg(icon_color)),
        Span::styled(view.title.clone(), Style::default().fg(theme::TEXT)), // 标题恒可见
    ];
    if let Some(args) = &view.args_preview {
        header.push(Span::styled(format!(" {args}"), Style::default().fg(theme::TEXT_MUTED)));
    }
    let mut lines = vec![RenderedLine::new(header)];
    // summary/activity/result 细节行（缩进，弱化色），按现有 tool_display 规则展开
    for detail in [&view.summary, &view.activity_summary, &view.result_summary].into_iter().flatten() {
        for l in detail.lines() {
            lines.push(RenderedLine::new(vec![Span::styled(format!("  {l}"), Style::default().fg(theme::TEXT_MUTED))]));
        }
    }
    let _ = ctx;
    RenderedBlock { block_id: block_id.to_string(), lines }
}
```

> 执行时优先复用 `tool_display` 现成格式化（DRY）；若其耦合旧 OutputLine，则在本组件内调用其纯格式化部分，仅替换最终产出类型为 RenderedLine。

- [ ] **Step 4: 接入分发** — 加分支；此时 `render_placeholder` 应已无任何 kind 走它，**删除 `render_placeholder`**，match 改为穷尽（无 `other =>` fallback）。

- [ ] **Step 5: 验证通过** — Run: `cargo test -p cli blocks::tool_call::tests && cargo build -p cli` → PASS

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/tui/render/output/blocks/
git commit -m "feat(tui): ToolCall block 组件，锁定标题可见，移除占位 fallback (refs #58, refs #62)"
```

---

## Phase 4：OutputArea 换血 + 选区叠加 + plain 复制

把 OutputArea 行容器从 `VecDeque<OutputLine>` 换为 `RenderedDocument`，render 直接画 spans，接入唯一选区叠加与 plain 复制。

### Task 4.1：选区叠加唯一路径 `apply_selection_overlay`

**Files:**
- Create: `apps/cli/src/tui/render/output/selection_overlay.rs`
- Modify: `apps/cli/src/tui/render/output/mod.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;
    use ratatui::text::Span;
    use crate::tui::render::output::rendered::RenderedLine;
    use crate::tui::render::theme;

    fn line() -> RenderedLine {
        RenderedLine::new(vec![Span::styled("hello", ratatui::style::Style::default().fg(Color::Red))])
    }

    #[test]
    fn test_overlay_none_returns_original_spans() {
        let spans = apply_selection_overlay(&line(), None);
        assert_eq!(spans[0].style.fg, Some(Color::Red));
        assert!(spans.iter().all(|s| s.style.bg.is_none()));
    }

    #[test]
    fn test_overlay_sets_bg_keeps_fg() {
        // 选中 "ell"（字符偏移 1..4）
        let spans = apply_selection_overlay(&line(), Some(SelRange { start: 1, end: 4 }));
        let visible: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(visible, "hello");
        // 选区内字符 bg=SELECTION_BG 且 fg 保留 Red
        let selected: Vec<_> = spans.iter().filter(|s| s.style.bg == Some(theme::SELECTION_BG)).collect();
        assert!(!selected.is_empty());
        assert!(selected.iter().all(|s| s.style.fg == Some(Color::Red)), "保留原前景色（修 #61）");
    }

    #[test]
    fn test_overlay_cjk_offset_by_char_not_byte() {
        let l = RenderedLine::new(vec![Span::raw("你好世界")]);
        let spans = apply_selection_overlay(&l, Some(SelRange { start: 1, end: 3 })); // 选中"好世"
        let sel: String = spans.iter().filter(|s| s.style.bg.is_some()).map(|s| s.content.as_ref()).collect();
        assert_eq!(sel, "好世", "按字符而非字节偏移（修 #48/#51）");
    }
}
```

- [ ] **Step 2: 验证失败** → FAIL

- [ ] **Step 3: 实现 `selection_overlay.rs`**（移植现状 `apply_selection_to_line` 的 split 逻辑，改为基于单行 plain 字符偏移）

```rust
//! 选区高亮唯一上色路径：只设 bg，保留原 fg，按字符边界 split span。
use ratatui::style::Style;
use ratatui::text::Span;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::theme;

/// 单行内的选区范围（基于该行 plain 的字符偏移，半开区间 [start, end)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelRange {
    pub start: usize,
    pub end: usize,
}

pub fn apply_selection_overlay(line: &RenderedLine, sel: Option<SelRange>) -> Vec<Span<'static>> {
    let Some(SelRange { start, end }) = sel else {
        return line.spans.clone();
    };
    if start >= end {
        return line.spans.clone();
    }
    let mut out = Vec::new();
    let mut global = 0usize; // 已遍历字符数
    for span in &line.spans {
        let mut buf = String::new();
        let mut cur_selected: Option<bool> = None;
        for ch in span.content.chars() {
            let selected = global >= start && global < end;
            if cur_selected != Some(selected) {
                if !buf.is_empty() {
                    out.push(make_span(std::mem::take(&mut buf), span.style, cur_selected.unwrap_or(false)));
                }
                cur_selected = Some(selected);
            }
            buf.push(ch);
            global += 1;
        }
        if !buf.is_empty() {
            out.push(make_span(buf, span.style, cur_selected.unwrap_or(false)));
        }
    }
    out
}

fn make_span(text: String, base: Style, selected: bool) -> Span<'static> {
    let style = if selected { base.bg(theme::SELECTION_BG) } else { base };
    Span::styled(text, style)
}
```

`output/mod.rs` 加 `pub mod selection_overlay;`。

- [ ] **Step 4: 验证通过** — Run: `cargo test -p cli selection_overlay::tests` → PASS

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/render/output/selection_overlay.rs apps/cli/src/tui/render/output/mod.rs
git commit -m "feat(tui): 统一选区叠加 apply_selection_overlay，保留前景色按字符偏移 (refs #58, refs #61, refs #48)"
```

### Task 4.2：OutputArea 持有 RenderedDocument（与旧字段并存，先接 render 输入）

**Files:**
- Modify: `apps/cli/src/tui/render/output_area/mod.rs`
- Modify: `apps/cli/src/tui/adapter/output_widget.rs`

- [ ] **Step 1: 写失败测试（`output_area/mod.rs` 或新 test 文件）**

```rust
#[test]
fn test_output_area_set_document_replaces_content() {
    use crate::tui::render::output::rendered::{RenderedBlock, RenderedDocument, RenderedLine};
    let mut area = OutputArea::new();
    let doc = RenderedDocument { blocks: vec![RenderedBlock {
        block_id: "a".into(), lines: vec![RenderedLine::new(vec![ratatui::text::Span::raw("x")])],
    }]};
    area.set_document(doc);
    assert_eq!(area.document().total_lines(), 1);
}
```

- [ ] **Step 2: 验证失败** → FAIL

- [ ] **Step 3: 实现** — `OutputArea` 新增字段 `document: RenderedDocument` 与 `document_renderer: OutputDocumentRenderer`（保留旧 `lines` 字段不动，本步只加不删）：

```rust
// struct OutputArea { ... 现有字段 ...
//   pub document: crate::tui::render::output::rendered::RenderedDocument,
//   pub document_renderer: crate::tui::render::output::document_renderer::OutputDocumentRenderer, }
pub fn set_document(&mut self, doc: crate::tui::render::output::rendered::RenderedDocument) {
    self.document = doc;
}
pub fn document(&self) -> &crate::tui::render::output::rendered::RenderedDocument {
    &self.document
}
```

`adapter/output_widget.rs` 新增直接从 ViewModel 渲染到 document 的入口（暂与旧 `replace_lines_from_view_model` 并存）：

```rust
pub(crate) fn render_document_from_view_model(area: &mut OutputArea, vm: &OutputViewModel, width: u16) {
    let doc = area.document_renderer.render(vm, width);
    area.set_document(doc);
}
```

- [ ] **Step 4: 验证通过** — Run: `cargo test -p cli test_output_area_set_document_replaces_content` → PASS

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/render/output_area/mod.rs apps/cli/src/tui/adapter/output_widget.rs
git commit -m "feat(tui): OutputArea 持有 RenderedDocument 与渲染器（与旧路径并存） (refs #58)"
```

### Task 4.3：render 改画 RenderedDocument（spans + 选区叠加），切换调用点

**Files:**
- Modify: `apps/cli/src/tui/render/output_area/render.rs`
- Modify: `apps/cli/src/tui/render/output_area/scroll.rs`
- Modify: 调用 `replace_lines_from_view_model` 的上游（`app/` 渲染刷新处，以 grep 定位）

- [ ] **Step 1: 写失败测试**（viewport 选取 + 选区叠加端到端）

```rust
#[test]
fn test_render_document_paints_spans_and_overlays_selection() {
    use crate::tui::render::output::rendered::{RenderedBlock, RenderedDocument, RenderedLine};
    use ratatui::{buffer::Buffer, layout::Rect, text::Span};
    let mut area = OutputArea::new();
    area.set_document(RenderedDocument { blocks: vec![RenderedBlock {
        block_id: "a".into(),
        lines: vec![RenderedLine::new(vec![Span::raw("hello")])],
    }]});
    // 设选区覆盖 "hel"
    area.set_selection_for_test((0, sdk::CharIdx::new(0)), (0, sdk::CharIdx::new(3)));
    let area_rect = Rect::new(0, 0, 10, 3);
    let mut buf = Buffer::empty(area_rect);
    let mut cache = crate::tui::view_state::cache::ViewRenderCache::default();
    area.render_with_cache(area_rect, &mut buf, &mut cache);
    // 第 0 行前 3 格背景应为 SELECTION_BG
    assert_eq!(buf.get(0, 0).bg, crate::tui::render::theme::SELECTION_BG);
}
```

> 若无 `set_selection_for_test`，用现有设置 selection_start/end 的公有/测试入口；CharIdx 构造见 `sdk::CharIdx::new`。

- [ ] **Step 2: 验证失败** → FAIL

- [ ] **Step 3: 实现** — 重写 `render_with_cache` 的内容来源：
  - 用 `self.document.iter_lines()` 展平为屏幕行序列（每个 RenderedLine 已是一屏幕行，无需再按 `\n` split）。
  - viewport：按 `scroll_offset`/`auto_scroll` 选 `[start,end)`（复用现有 `visible_range` 逻辑，输入改为 `document().total_lines()`）。
  - 每行：`apply_selection_overlay(line, sel_for_line(idx))` → `Line::from(spans)` → 画。
  - `sel_for_line`：把 `selection_start/end` 的 `(行号, CharIdx)` 换算为当前行的 `SelRange`（当前行在选区内时 start/end 按字符 clamp）。
  - 追加 spinner / 排队？排队现在是 block，已在 document 内（QueuedSubmission），不再单独 append。spinner 仍由 `build_spinner_line` 追加在最末。
  - `screen_line_map` 重建为 `Vec<(doc_line_idx, CharIdx_start, CharIdx_end)>`，复制时用。

  关键替换（伪代码，实现时落到真实字段）：

```rust
let total = self.document.total_lines();
let (start, end) = self.visible_range_for(total, visible_lines);
let lines: Vec<&RenderedLine> = self.document.iter_lines().collect();
let mut display: Vec<Line<'static>> = Vec::new();
for idx in start..end {
    let rl = lines[idx];
    let sel = self.sel_range_for_line(idx);
    display.push(Line::from(apply_selection_overlay(rl, sel)));
}
// spinner / task_status 追加 ...
```

- [ ] **Step 4: 切换上游调用** — grep `replace_lines_from_view_model` 调用点，改为 `render_document_from_view_model(area, vm, width)`。

```bash
grep -rn "replace_lines_from_view_model" apps/cli/src/
```

- [ ] **Step 5: 验证通过** — Run: `cargo test -p cli test_render_document_paints_spans_and_overlays_selection && cargo test -p cli` → PASS（全包）

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/tui/render/output_area/ apps/cli/src/tui/app/
git commit -m "feat(tui): OutputArea 渲染改用 RenderedDocument + 选区叠加，切换上游调用 (refs #58, refs #80)"
```

### Task 4.4：复制取 plain 字符切片

**Files:**
- Modify: `apps/cli/src/tui/render/output_area/selection_render.rs`（或复制逻辑所在文件）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn test_copy_selection_returns_plain_chars_across_lines() {
    use crate::tui::render::output::rendered::{RenderedBlock, RenderedDocument, RenderedLine};
    use ratatui::text::Span;
    let mut area = OutputArea::new();
    area.set_document(RenderedDocument { blocks: vec![RenderedBlock {
        block_id: "a".into(),
        lines: vec![
            RenderedLine::with_plain(vec![Span::raw("**bold**")], "bold".into()),
            RenderedLine::new(vec![Span::raw("世界")]),
        ],
    }]});
    area.set_selection_for_test((0, sdk::CharIdx::new(0)), (1, sdk::CharIdx::new(2)));
    let copied = area.selected_text();
    assert_eq!(copied, "bold\n世界", "复制取 plain（无 markdown 标记，CJK 按字符）");
}
```

- [ ] **Step 2: 验证失败** → FAIL

- [ ] **Step 3: 实现 `selected_text()`** — 遍历选区覆盖的 document 行，取每行 `plain` 的字符切片（首行从 start_col、末行到 end_col），行间 `\n` 连接。用 `plain.chars().skip(a).take(b-a).collect()`。

- [ ] **Step 4: 验证通过** — Run: `cargo test -p cli test_copy_selection_returns_plain_chars_across_lines` → PASS

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/render/output_area/
git commit -m "feat(tui): 选区复制取 plain 字符切片 (refs #58, refs #51, refs #60)"
```

### Task 4.5：MAX_LINES 改 block 级裁剪

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/output.rs`（或 OutputDocumentRenderer 产出后裁剪）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn test_document_drops_oldest_block_when_over_max_lines() {
    // 构造超过 MAX_LINES 的多 block 文档，断言最旧整个 block 被丢弃，无半截行
    // （具体在 OutputDocumentRenderer::render 后对 RenderedDocument 裁剪）
}
```

- [ ] **Step 2: 验证失败** → FAIL

- [ ] **Step 3: 实现** — 在 `OutputDocumentRenderer::render` 末尾，对 `RenderedDocument` 做 block 级裁剪：从最旧 block 累加行数，超过 `MAX_LINES` 时整块丢弃，保留最新若干完整 block。

- [ ] **Step 4: 验证通过 + Commit**

```bash
git add apps/cli/src/tui/render/output/document_renderer.rs
git commit -m "feat(tui): MAX_LINES 改为 block 级裁剪，消除陈旧行下标 (refs #58, refs #71)"
```

---

## Phase 5：删除旧路径（真正删除，不留死代码）

每步删除后 `cargo build` 必须无 `dead_code`/`unused` 警告。

### Task 5.1：删除拍平桥与纯文本路径

**Files:**
- Delete: `apps/cli/src/tui/adapter/output_widget.rs` 中 `line_to_plain_text` + `replace_lines_from_view_model`
- Delete: `apps/cli/src/tui/render/output_view_model.rs`

- [ ] **Step 1: grep 确认无残留调用**

```bash
grep -rn "replace_lines_from_view_model\|line_to_plain_text\|output_view_model_lines" apps/cli/src/
```
Expected: 仅定义处（即将删）

- [ ] **Step 2: 删除文件/函数，移除 mod 声明**

`adapter/mod.rs` 移除 `output_widget`（若整文件只剩这两函数则删文件）；`render/mod.rs` 移除 `pub mod output_view_model;`。

- [ ] **Step 3: 编译验证无警告**

Run: `cargo build -p cli 2>&1 | grep -E "warning: (unused|dead_code)"`
Expected: 空

- [ ] **Step 4: Commit**

```bash
git rm apps/cli/src/tui/render/output_view_model.rs
git add -A
git commit -m "refactor(tui): 删除拍平桥 output_widget 与纯文本 output_view_model 路径 (refs #58)"
```

### Task 5.2：删除 OutputLine / LineStyle / 旧行级渲染链（cache/line/block/span）

> 现状核对发现：`render/output/{line,block,span}.rs` 与 `cache.rs` 构成一条没接线的旧行级渲染链（`OutputLine → line::render_range → RenderedCache`），其中 `block.rs`/`span.rs` 已整体 `#[allow(dead_code)]`，`line.rs` 仅被 `cache.rs` 调用。删 `cache.rs` 后三者孤立，必须一并删除，否则残留死代码会被 Task 6.1 门禁拦截。

**Files:**
- Modify: `apps/cli/src/tui/render/output_area/mod.rs`（删 `lines: VecDeque<OutputLine>` 及相关方法 `push_line`/`do_rerender` 等）
- Delete: `apps/cli/src/tui/render/output_area/types.rs` 的 `OutputLine`/`LineStyle`
- Delete: `apps/cli/src/tui/render/output/cache.rs` 旧 `RenderedLine`/`RenderedCache`、`view_state/cache.rs` 的 `OutputRenderCacheState`
- Delete: `apps/cli/src/tui/render/output/line.rs`、`apps/cli/src/tui/render/output/block.rs`、`apps/cli/src/tui/render/output/span.rs`（旧行级渲染链，删 cache.rs 后孤立）
- Delete: `apps/cli/src/tui/render/output_area/streaming.rs` 的 `do_rerender` `<think>` 路径
- Modify: `apps/cli/src/tui/render/output/mod.rs`（移除 `pub mod line/block/span/cache;`）

- [ ] **Step 1: grep 依赖**

```bash
grep -rn "OutputLine\b\|LineStyle\|RenderedCache\|OutputRenderCacheState\|do_rerender\|push_line\|render_range\|collect_table_ranges\|scan_code_blocks\|slice_spans" apps/cli/src/
```

- [ ] **Step 2: 逐个删除并修复编译** — 把仍引用旧类型的代码迁到 RenderedDocument 路径（streaming 文本改为更新 ConversationModel 的 active block，由 ViewModel 驱动重渲）。

- [ ] **Step 2b: 删除旧行级渲染链文件** — `git rm` 掉 `line.rs`/`block.rs`/`span.rs`，并从 `render/output/mod.rs` 移除其 `pub mod` 声明；其测试（`render/output/block_tests.rs`/`selection_tests.rs` 中依赖 `render_range`/`slice_spans` 的用例）一并删除或迁移到新组件测试。

```bash
grep -rln "render_range\|slice_spans\|scan_code_blocks\|CodeBlockInfo" apps/cli/src/ # 确认仅测试/自身引用
git rm apps/cli/src/tui/render/output/line.rs apps/cli/src/tui/render/output/block.rs apps/cli/src/tui/render/output/span.rs
```

- [ ] **Step 3: 提升新 `RenderedLine` 到 `output/mod.rs` 顶层 re-export**（此时旧同名类型已删，无冲突）

```rust
pub use rendered::{RenderCtx, RenderedBlock, RenderedDocument, RenderedLine};
```

- [ ] **Step 4: 编译无警告 + 全测**

Run: `cargo build -p cli 2>&1 | grep -E "warning: (unused|dead_code)"; cargo test -p cli`
Expected: 无警告；全 PASS

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor(tui): 删除 OutputLine/LineStyle/旧行级渲染链(cache/line/block/span)/streaming think 扫描，提升 RenderedLine (refs #58, refs #71)"
```

### Task 5.3：删除 legacy 排队机制

**Files:**
- Delete: `apps/cli/src/tui/render/output_area/queued.rs`
- Modify: `apps/cli/src/tui/render/output_area/mod.rs`（删 `queued_messages`/`queued_line_count`）
- Modify: `apps/cli/src/tui/render/output_area/content.rs`（删 `push_user_message` 中 queued 跟踪）、`render/output/status_line.rs`（删 append 排队行）

- [ ] **Step 1: grep**

```bash
grep -rn "build_queued_message_lines\|queued_messages\|queued_line_count" apps/cli/src/
```

- [ ] **Step 2: 删除并修复编译** — 排队显示现完全由 `QueuedSubmission` block 驱动。

- [ ] **Step 3: 编译无警告 + 测试**

Run: `cargo test -p cli && cargo build -p cli 2>&1 | grep -E "warning"`
Expected: PASS；无警告

- [ ] **Step 4: Commit**

```bash
git rm apps/cli/src/tui/render/output_area/queued.rs
git add -A
git commit -m "refactor(tui): 删除 legacy 排队机制，排队显示单一真相归 QueuedSubmission block (refs #58)"
```

---

## Phase 6：收紧 guard + 删除验证门禁

### Task 6.1：删除验证门禁 grep（硬要求）

**Files:**
- 验证脚本：临时命令（不入库）

- [ ] **Step 1: 全仓零命中验证**

```bash
grep -rn "OutputLine\b\|LineStyle\|replace_lines_from_view_model\|line_to_plain_text\|output_view_model_lines\|build_queued_message_lines\|queued_messages\|queued_line_count\|RenderedCache\|OutputRenderCacheState\|render_range\|collect_table_ranges\|scan_code_blocks\|slice_spans" apps/cli/src/ \
  --include=*.rs | grep -v "//"
```
Expected: 0 命中

- [ ] **Step 2: 确认删除的文件不存在**

```bash
for f in adapter/output_widget.rs render/output_view_model.rs render/output_area/queued.rs \
         render/output/line.rs render/output/block.rs render/output/span.rs render/output/cache.rs; do
  test ! -f "apps/cli/src/tui/$f" || { echo "✗ 仍存在: $f"; exit 1; }
done
echo "OK: 旧文件已删除"
```
Expected: `OK: 旧文件已删除`

- [ ] **Step 3: 复用原语后必须移除其 `#[allow(dead_code)]`，且 output 渲染层无死代码残留**

现状 `render/output/diff.rs`、`render/syntax.rs`、`render/output/tool_display/results.rs`、`tool_display/mod.rs` 整体/大量 `#[allow(dead_code)]`（即「富渲染引擎被绕过」的实锤）。Phase 2/3 经 `primitives` 包装、ToolCall 组件复用后，它们已被真正调用，**必须删除对应 `allow(dead_code)`**，否则掩盖回归。

```bash
grep -rn "allow(dead_code)" apps/cli/src/tui/render/output/ apps/cli/src/tui/render/output_area/
```
Expected: 0 命中（被本重构激活/删除的模块；若有残留必须有充分书面理由）。逐项核对 `diff.rs`/`syntax.rs`/`tool_display/results.rs`/`tool_display/mod.rs` 的 `allow(dead_code)` 已随复用移除。

### Task 6.2：render isolation guard

**Files:**
- Create/Modify: `.agents/hooks/check-render-isolation.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`（接入）

- [ ] **Step 1: 写 guard 脚本** — 扫 `apps/cli/src/tui/render/output/`：
  - 禁止 `use crate::tui::model::`（render 不得引用 Model 可变类型）
  - 禁止 `std::fs`/`std::process`/`tokio`（render 不得 IO）
  - 禁止 `apply_selection_overlay` 之外的文件直接 `.bg(theme::SELECTION_BG)`（确保选区叠加唯一上色路径，防 #61 回归）

```bash
#!/usr/bin/env bash
set -euo pipefail
ROOT="apps/cli/src/tui/render/output"
fail=0
if grep -rn "use crate::tui::model::" "$ROOT" --include=*.rs; then
  echo "✗ render/output 不得引用 model"; fail=1
fi
if grep -rn "std::fs::\|std::process::\|tokio::" "$ROOT" --include=*.rs | grep -v "test"; then
  echo "✗ render/output 不得做 IO"; fail=1
fi
# 选区上色唯一路径
if grep -rln "SELECTION_BG" "$ROOT" --include=*.rs | grep -v "selection_overlay.rs"; then
  echo "✗ 只有 selection_overlay.rs 可使用 SELECTION_BG"; fail=1
fi
exit $fail
```

- [ ] **Step 2: 接入 `check-architecture-guards.sh`** — 追加调用 `check-render-isolation.sh`。

- [ ] **Step 3: 运行 guard**

Run: `.agents/hooks/check-architecture-guards.sh`
Expected: 全通过

- [ ] **Step 4: Commit**

```bash
git add .agents/hooks/
git commit -m "feat(guard): render isolation guard，锁定选区上色唯一路径 (refs #58)"
```

### Task 6.3：回归测试补全 + 全量验证 + 文档归档准备

**Files:**
- Modify: 各组件测试文件（补关联 bug 回归）
- Modify: `docs/bug/active.md`、`docs/feature/active.md`

- [ ] **Step 1: 补回归测试** — 针对 #61/#62/#65/#71/#74/#80 各补一条端到端断言（部分已在 Phase 2-4 覆盖，此处补齐缺口，如 #74：System 样式 block 后 Assistant block 不继承暗色）。

```rust
#[test]
fn test_system_block_does_not_leak_color_to_next_assistant_block() {
    // 两个独立 block：SystemNotice + AssistantMessage，断言 assistant 行 fg=ASSISTANT
}
```

- [ ] **Step 2: 全量验证门禁**

Run:
```bash
cargo build -p cli && cargo test -p cli && cargo clippy -p cli -- -D warnings && .agents/hooks/check-architecture-guards.sh
```
Expected: 全绿

- [ ] **Step 3: 更新 bug/feature 状态** — `docs/bug/active.md` 把 #61/#62/#65/#71/#74/#80 状态改「待确认」；`docs/feature/active.md` #58 改「待确认」。

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "test(tui): 渲染管线重构关联 bug 回归 + 状态更新 (refs #58, refs #61, refs #62, refs #65, refs #71, refs #74, refs #80)"
```

---

## Self-Review

**1. Spec 覆盖核对**

| spec 章节 | 对应 Task |
|---|---|
| 架构与数据流（§37-56） | Phase 1（类型+ViewModel+Renderer）、Phase 4（OutputArea） |
| 组件系统（§58-74） | Phase 1.5 骨架 + Phase 3 全部组件 + `blocks/` 布局 |
| 用户回显与 input queue（§76-85） | Task 1.3（映射）+ 3.2/3.3（组件）+ 5.3（删 legacy） |
| 缓存与流式（§87-98） | Task 1.4（BlockCache）+ 4.5（裁剪）+ 5.2（删旧缓存/streaming） |
| 选中/复制（§100-106） | Task 4.1（overlay）+ 4.4（plain 复制） |
| 增量迁移 6 步（§108-117） | Phase 1→6 一一对应 |
| 删除验证门禁（§119-126） | Phase 5 + Task 6.1 |
| 测试（§128-135） | 各 Task TDD + Task 6.3 回归 |
| 涉及路径（§137-143） | File Structure 段 |

**2. Placeholder 扫描** — 已消除：每个改代码步骤含实际代码块；`render_assistant_message` 中 `TextBlockViewRef` 为示意，已在 Step 3 注明用真实类型 `TextBlockView`，实现时删别名。Task 4.3/4.5 含伪代码骨架但给出明确字段与算法，执行时落地。

**3. 类型一致性** — `RenderedLine`(spans/plain)、`RenderedBlock`(block_id/lines)、`RenderedDocument`(blocks)、`RenderCtx`(width)、`OutputBlockView`(block_id/block_version/kind)、`OutputBlockKind`(8 变体)、`CacheKey`(version/width)、`SelRange`(start/end) 全程命名一致。组件函数统一签名 `render_xxx(block_id: &str, view: &TextBlockView|&ToolCallBlockView, ctx: &RenderCtx) -> RenderedBlock`（separator 无 view/ctx）。

**已知执行风险（执行者注意）**
- Task 3.4 AssistantMessage 的 fence/table 扫描与现状 markdown 多行渲染存在重叠，执行时优先复用 `render/output/markdown` 现有多行入口（若有），避免重造；本 plan 给的是按行状态机的保底实现。
- Task 4.3 是最大改动点，`render_with_cache` 现状逻辑复杂（spinner/screen_map/scrollbar/tool dots），需逐段迁移，建议拆成更细的子提交。
- streaming（Task 5.2 Step 2）从 `<think>` 扫描迁到 ViewModel-driven，需确认 ConversationModel 的 active assistant/thinking block 在流式追加时持续 bump version——这依赖现状 streaming 写入 ConversationModel 的路径，执行前先 grep 确认 `active_text_block_id`/`active_thinking_block_id` 的更新点。
