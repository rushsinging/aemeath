# Issue #1670 TUI 渲染控制字符归一化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 收敛共享 display 控制字符归一化入口，根因修复 TUI 渲染吞 tab / 零宽控制字符问题（含 ESC/C1 显示注入阻断），并迁移 tool result 既有 `expand_tabs` 消除重复。

**Architecture:** 单一策略函数 `normalize_display_control_chars`（`view_model/display_text.rs`，纯字符串、无 ratatui 依赖，对 model/render/view_assembler 三方均合法）+ 四道防线：① `render_self` 分发入口归一化 `TextBlockView.text`（源头，先于宽度计算与 markdown/links 解析）；② `wrap_spans_with_prefix` 入口归一化 spans（宽度敏感路径）；③ `RenderedLine` 构造（`new`/`with_plain`/`with_plain_and_links`）归一化（一切直产行的最终防线，保持 `plain == spans` 拼接不变式）；④ tool result assembler 层迁移共享函数并删除本地 `expand_tabs`（DRY）。

**Tech Stack:** Rust / ratatui / unicode-width；TDD（先红后绿）；cargo fmt / clippy / test 门禁。

**Worktree:** `~/.agents/worktrees/aemeath/fix-1670-display-control-chars`（基于 origin/main `5de41d12`）

**策略约定（单一真相，与 issue #1670 一致）：**

| 输入 | 处理 |
|---|---|
| `\t` | 展开为 4 空格（沿用 #196 tool result 既有约定） |
| `\n` | 保留 |
| 其余控制字符（C0/C1/DEL，含 `ESC`、C1-CSI、`NUL`、`BEL`、`DEL`） | 替换为 `U+FFFD`（阻断 ANSI 显示注入 + 消除零宽吞字） |
| 普通字符（含 CJK） | 原样 |
| `width()==Some(0)` 零宽格式字符（ZWSP/ZWJ/组合符，非 Cc 类） | 原样保留 |

**依赖合法性（已核对 `docs/design/03-engineering/01-architecture-guards.md` §12）：**
- `view_model/` 守卫仅禁 `crate::tui::model` / `ratatui` → 纯字符串函数放 `view_model` ✅
- `view_assembler/` 守卫禁 `ratatui` 等副作用，不禁 `view_model` / `render` ✅
- `render/` 对 `view_model` 依赖为既有方向 ✅

---

### Task 1: 共享归一化函数 `normalize_display_control_chars`

**Files:**
- Create: `apps/cli/src/tui/view_model/display_text.rs`
- Modify: `apps/cli/src/tui/view_model.rs`（注册 `pub mod display_text;`）

- [ ] **Step 1.1: 确认 view_model.rs 当前模块清单**

Run: `head -12 apps/cli/src/tui/view_model.rs`
Expected: 现有 `pub mod conversation;` … `pub mod style;` 等声明（后续在其 alphabetical 位置插入）。

- [ ] **Step 1.2: 新建失败测试 + stub**

创建 `apps/cli/src/tui/view_model/display_text.rs`：

```rust
//! 显示文本控制字符归一化策略（单一真相，issue #1670）。
//!
//! 终端渲染链路对控制字符的处理约定：
//! - `\t` 展开为 4 空格（沿用 tool result 既有约定，#196）；
//! - `\n` 保留（多行文本由渲染组件按行拆分）；
//! - 其余控制字符（C0/C1/DEL，含 ESC 与 C1-CSI）替换为 `U+FFFD`，
//!   阻断 ANSI 显示注入并消除零宽吞字。

/// 把文本中的控制字符归一化为可安全显示的形态。
pub fn normalize_display_control_chars(text: &str) -> String {
    let _ = text;
    unimplemented!("issue #1670: not implemented")
}

#[cfg(test)]
mod tests {
    use super::normalize_display_control_chars;

    #[test]
    fn test_tab_expands_to_four_spaces() {
        assert_eq!(normalize_display_control_chars("a\tb"), "a    b");
    }

    #[test]
    fn test_multiple_tabs_expand_independently() {
        assert_eq!(
            normalize_display_control_chars("wanaka_session\t9892\t78"),
            "wanaka_session    9892    78"
        );
    }

    #[test]
    fn test_newline_preserved() {
        assert_eq!(normalize_display_control_chars("a\nb"), "a\nb");
    }

    #[test]
    fn test_escape_char_replaced_with_replacement_char() {
        assert_eq!(
            normalize_display_control_chars("a\u{1b}[31m"),
            "a\u{fffd}[31m"
        );
    }

    #[test]
    fn test_c1_csi_replaced() {
        assert_eq!(normalize_display_control_chars("a\u{9b}0m"), "a\u{fffd}0m");
    }

    #[test]
    fn test_bell_and_del_replaced() {
        assert_eq!(
            normalize_display_control_chars("a\u{7}b\u{7f}"),
            "a\u{fffd}b\u{fffd}"
        );
    }

    #[test]
    fn test_plain_text_unchanged() {
        assert_eq!(normalize_display_control_chars("你好 hello 2026"), "你好 hello 2026");
    }

    #[test]
    fn test_zero_width_format_chars_preserved() {
        assert_eq!(normalize_display_control_chars("a\u{200b}b"), "a\u{200b}b");
    }
}
```

在 `apps/cli/src/tui/view_model.rs` 模块声明区（按 alphabetical，`pub mod dialog;` 之后、`pub mod input;` 之前）插入：

```rust
pub mod display_text;
```

- [ ] **Step 1.3: 运行测试确认失败（红）**

Run: `cargo test -p cli display_text`
Expected: FAIL — 9 个测试全部 `not implemented` panic（编译通过、断言阶段失败）。

- [ ] **Step 1.4: 实现最小通过代码**

替换 `normalize_display_control_chars` 函数体：

```rust
pub fn normalize_display_control_chars(text: &str) -> String {
    if !text.chars().any(char::is_control) {
        return text.to_string();
    }
    let mut normalized = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\t' => normalized.push_str("    "),
            '\n' => normalized.push('\n'),
            other if other.is_control() => normalized.push('\u{fffd}'),
            other => normalized.push(other),
        }
    }
    normalized
}
```

（`char::is_control()` 覆盖 Cc 类 = C0 + C1 + DEL；`\t`/`\n` 先于守卫分支匹配，`\r` 落入 `is_control` 守卫 → `U+FFFD`。）

- [ ] **Step 1.5: 运行测试确认通过（绿）**

Run: `cargo test -p cli display_text`
Expected: PASS — `8 passed`。

- [ ] **Step 1.6: Commit**

```bash
git add apps/cli/src/tui/view_model/display_text.rs apps/cli/src/tui/view_model.rs
git commit -m "feat(tui): #1670 共享显示文本控制字符归一化函数"
```

---

### Task 2: 渲染层复现测试（红）+ 三道渲染防线实现（绿）

**Files:**
- Modify: `apps/cli/src/tui/render/output/block_component.rs`（分发入口归一化，防线①）
- Modify: `apps/cli/src/tui/render/output/primitives/wrap.rs`（wrap 入口归一化，防线②）
- Modify: `apps/cli/src/tui/render/output/rendered.rs`（`RenderedLine` 构造归一化 + 共享 span helper，防线③）
- Test（同文件内 `mod tests`）: `wrap.rs` / `rendered.rs` / `blocks/user_message.rs` / `blocks/assistant_message.rs`
- Test: `apps/cli/src/tui/render/output/document_renderer/tests.rs`（L4 场景 ×2）

- [ ] **Step 2.1: 写 7 个测试（5 组）失败复现测试（红）**

**(a) `primitives/wrap.rs` tests 模块追加：**

```rust
    #[test]
    fn test_wrap_entry_normalizes_tab_before_width_calc() {
        // issue #1670：tab 必须在宽度计算前展开，否则以 0 宽参与断行导致溢出/吞字。
        let lines = wrap_spans_with_prefix(vec![Span::raw("a\tb")], 80, None, WrapMode::Word);
        assert_eq!(lines[0].plain, "a    b");
    }
```

**(b) `rendered.rs` tests 模块追加：**

```rust
    #[test]
    fn test_rendered_line_new_normalizes_control_chars() {
        let line = RenderedLine::new(vec![Span::raw("a\u{1b}b")]);
        assert_eq!(line.plain, "a\u{fffd}b");
        assert_eq!(line.spans[0].content.as_ref(), "a\u{fffd}b");
    }

    #[test]
    fn test_with_plain_normalizes_spans_and_plain_symmetrically() {
        // 不变式：plain == spans 可见文本拼接 —— 两侧必须同函数归一化。
        let line = RenderedLine::with_plain(vec![Span::raw("a\tb")], "a\tb".to_string());
        assert_eq!(line.plain, "a    b");
        assert_eq!(line.spans[0].content.as_ref(), "a    b");
        assert_eq!(
            line.plain,
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        );
    }
```

（若 rendered.rs 现有 tests 模块无 `Span` import，补 `use ratatui::text::Span;`。）

**(c) `blocks/user_message.rs` tests 模块追加：**

```rust
    #[test]
    fn test_user_message_expands_tab_to_four_spaces() {
        // issue #1670 复现：TSV 回显 tab 被吞成一整行。
        let view = TextBlockView {
            key: "u".into(),
            text: "wanaka_session\t9892\t78".into(),
            style: SemanticStyle::Normal,
        };
        let block = render_user_message("u", &view, &RenderCtx::for_width(80));
        assert_eq!(block.lines[0].plain, "wanaka_session    9892    78");
    }
```

**(d) `blocks/assistant_message.rs` tests 模块追加：**

```rust
    #[test]
    fn test_assistant_code_block_expands_tab() {
        // issue #1670 复现：fenced code block 内 tab 被吞。
        let block = render("```\nwanaka_session\t9892\n```");
        let code_line = block
            .lines
            .iter()
            .find(|line| line.plain.contains("wanaka_session"))
            .expect("应渲染出代码行");
        assert!(
            !code_line.plain.contains('\t'),
            "代码行不应残留 tab，实际: {:?}",
            code_line.plain
        );
        assert!(code_line.plain.contains("wanaka_session    9892"));
    }
```

（复用该文件既有 `fn render(text: &str) -> RenderedBlock` 测试 helper。）

**(e) `document_renderer/tests.rs` 追加 L4 场景 ×2**（构造模式对齐同文件 `test_renderer_adds_user_message_card_spacers`，消息行位于 `lines[2]`）：

```rust
#[test]
fn test_user_message_tab_renders_as_visible_spaces_scene() {
    // issue #1670 场景：TSV 用户消息回显列对齐可见。
    let kind = OutputBlockKind::UserMessage(TextBlockView {
        key: "u".into(),
        text: "wanaka_session\t9892\t78".into(),
        style: SemanticStyle::Normal,
    });
    let user = BlockNode {
        block_id: "u".into(),
        block_version: kind.cache_version(),
        kind,
        children: Vec::new(),
    };
    let vm = vm_with_roots(vec![user]);
    let mut renderer = OutputDocumentRenderer::default();
    let doc = renderer.render_tree(&vm, 80);
    let lines = &doc.blocks[0].lines;

    assert_eq!(lines[2].plain, "wanaka_session    9892    78");
    assert!(!lines[2].plain.contains('\t'));
}

#[test]
fn test_assistant_code_block_tab_renders_as_visible_spaces_scene() {
    // issue #1670 场景：assistant 代码块内 tab 可见为空白。
    let kind = OutputBlockKind::AssistantMessage(TextBlockView {
        key: "a".into(),
        text: "```\nwanaka_session\t9892\n```".into(),
        style: SemanticStyle::Normal,
    });
    let assistant = BlockNode {
        block_id: "a".into(),
        block_version: kind.cache_version(),
        kind,
        children: Vec::new(),
    };
    let vm = vm_with_roots(vec![assistant]);
    let mut renderer = OutputDocumentRenderer::default();
    let doc = renderer.render_tree(&vm, 80);

    let code_line = doc.blocks[0]
        .lines
        .iter()
        .find(|line| line.plain.contains("wanaka_session"))
        .expect("应渲染出代码行");
    assert!(
        !code_line.plain.contains('\t'),
        "代码行不应残留 tab，实际: {:?}",
        code_line.plain
    );
    assert!(code_line.plain.contains("wanaka_session    9892"));
}
```

- [ ] **Step 2.2: 运行测试确认失败（红）**

Run: `cargo test -p cli -- test_wrap_entry_normalizes_tab_before_width_calc test_rendered_line_new_normalizes_control_chars test_with_plain_normalizes_spans_and_plain_symmetrically test_user_message_expands_tab_to_four_spaces test_assistant_code_block_expands_tab test_user_message_tab_renders_as_visible_spaces_scene test_assistant_code_block_tab_renders_as_visible_spaces_scene`

Expected: FAIL — 上述测试断言 tab 已展开 / ESC 已替换，当前实现输出原样 `\t` / `\u{1b}`。（若个别测试因语法/依赖编译失败，先修正编译错误再跑，仍须断言失败。）

- [ ] **Step 2.3: 实现防线③ — `rendered.rs` 归一化**

文件头部 use 区追加：

```rust
use crate::tui::view_model::display_text::normalize_display_control_chars;
```

在 `impl RenderedLine` 之前（模块级）新增共享 span helper：

```rust
/// 把 spans 中的控制字符归一化（issue #1670）；含控制字符的 span 被替换为归一化副本，
/// 干净 span 保持原对象。wrap 入口与 RenderedLine 构造共用（DRY）。
pub(crate) fn normalize_span_texts(spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    spans
        .into_iter()
        .map(|span| {
            let normalized = normalize_display_control_chars(span.content.as_ref());
            if normalized == span.content.as_ref() {
                span
            } else {
                Span::styled(normalized, span.style)
            }
        })
        .collect()
}
```

修改 `RenderedLine::new`（现 L83-97）：

```rust
    pub fn new(spans: Vec<Span<'static>>) -> Self {
        let spans = normalize_span_texts(spans);
        let plain = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        Self {
            spans,
            plain,
            style: Style::default(),
            gutter_cols: 0,
            fill_style: None,
            links: Vec::new(),
            animation: None,
        }
    }
```

修改 `RenderedLine::with_plain`（现 L120-131）：

```rust
    pub fn with_plain(spans: Vec<Span<'static>>, plain: String) -> Self {
        let spans = normalize_span_texts(spans);
        let plain = normalize_display_control_chars(&plain);
        Self {
            spans,
            plain,
            style: Style::default(),
            gutter_cols: 0,
            fill_style: None,
            links: Vec::new(),
            animation: None,
        }
    }
```

修改 `RenderedLine::with_plain_and_links`（现 L133-154）：函数体开头同样加两行归一化：

```rust
    pub fn with_plain_and_links(
        spans: Vec<Span<'static>>,
        plain: String,
        links: Vec<LinkSpan>,
    ) -> Self {
        let spans = normalize_span_texts(spans);
        let plain = normalize_display_control_chars(&plain);
        Self {
            spans,
            plain,
            style: Style::default(),
            gutter_cols: 0,
            fill_style: None,
            links,
            animation: None,
        }
    }
```

> 说明：`links` 偏移基于原始文本字符位。防线①（`render_self` 源头归一化）保证进入 markdown 解析的文本已干净，此处归一化对 link 行为 no-op，不会造成偏移错位；此处理只作最终防线。

`from_plain` 是 `#[cfg(test)]` 专用构造，函数体开头加 `let plain = normalize_display_control_chars(&plain);`（保证测试构造与生产语义一致；`Span::raw(plain.clone())` 位于其后自然继承归一化结果）。注意调整顺序：先归一化 plain，再 `Span::raw(plain.clone())`：

```rust
    #[cfg(test)]
    pub fn from_plain(text: impl Into<String>) -> Self {
        let plain = normalize_display_control_chars(&text.into());
        Self {
            spans: vec![Span::raw(plain.clone())],
            plain,
            style: Style::default(),
            gutter_cols: 0,
            fill_style: None,
            links: Vec::new(),
            animation: None,
        }
    }
```

- [ ] **Step 2.4: 实现防线② — `wrap.rs` 入口归一化**

`wrap.rs` 顶部 use 改为：

```rust
use crate::tui::render::output::rendered::{normalize_span_texts, RenderedLine};
```

`wrap_spans_with_prefix`（现 L21-35）函数体第一行插入：

```rust
pub fn wrap_spans_with_prefix(
    spans: Vec<Span<'static>>,
    max_width: usize,
    continuation_prefix: Option<Span<'static>>,
    mode: WrapMode,
) -> Vec<RenderedLine> {
    // issue #1670：宽度敏感处理前先归一化控制字符，保证 tab 不以 0 宽参与断行计算。
    let spans = normalize_span_texts(spans);
    if max_width == 0 {
        return vec![RenderedLine::new(spans)];
    }
    // …原有 match mode 分支不变（原参数 spans 改用上方归一化后的绑定）
```

（`wrap_spans_to_rendered_lines` 调本函数，自动覆盖。）

- [ ] **Step 2.5: 实现防线① — `block_component.rs` 分发入口归一化**

文件顶部 use 追加：

```rust
use crate::tui::view_model::display_text::normalize_display_control_chars;
```

`impl BlockComponent for OutputBlockKind` 的 `render_self` 中，5 个 `TextBlockView` variant 改经 `normalized_text_block_view`；其余 variant（ToolCall/ToolResult/AskUserBatch/HookNotice）不动：

```rust
/// 渲染前归一化视图文本副本（issue #1670 源头防线：先于宽度计算与
/// markdown/links 解析，保证 links 偏移基于已归一化文本对齐）。
fn normalized_text_block_view(view: &TextBlockView) -> TextBlockView {
    TextBlockView {
        key: view.key.clone(),
        text: normalize_display_control_chars(&view.text),
        style: view.style,
    }
}

impl BlockComponent for OutputBlockKind {
    fn render_self(&self, block_id: &str, ctx: &RenderCtx) -> RenderedBlock {
        match self {
            OutputBlockKind::AssistantMessage(text) => {
                blocks::assistant_message::render_assistant_message(
                    block_id,
                    &normalized_text_block_view(text),
                    ctx,
                )
            }
            OutputBlockKind::ToolCall(tool) => {
                blocks::tool_call::render_tool_call(block_id, tool, ctx)
            }
            OutputBlockKind::ToolResult(result) => {
                blocks::tool_result::render_tool_result(block_id, result, ctx)
            }
            OutputBlockKind::ThinkingMessage(text) => {
                blocks::thinking::render_thinking(block_id, &normalized_text_block_view(text), ctx)
            }
            OutputBlockKind::UserMessage(text) => {
                blocks::user_message::render_user_message(
                    block_id,
                    &normalized_text_block_view(text),
                    ctx,
                )
            }
            OutputBlockKind::AskUserBatch(ask) => {
                blocks::ask_user::render_ask_user_batch(block_id, ask, ctx)
            }
            OutputBlockKind::HookNotice(notice) => {
                blocks::hook_notice::render_hook_notice(block_id, notice, ctx)
            }
            OutputBlockKind::SystemNotice(text) | OutputBlockKind::DiagnosticNotice(text) => {
                blocks::diagnostic::render_diagnostic(
                    block_id,
                    &normalized_text_block_view(text),
                    ctx,
                )
            }
        }
    }
}
```

（`SemanticStyle` 需为 `Copy`；若非 `Copy` 则 `style: view.style.clone()`，以编译器为准。）

- [ ] **Step 2.6: 运行测试确认通过（绿）+ 渲染层全量回归**

Run: `cargo test -p cli -- tui::render`
Expected: PASS — 6 组新测试绿，且 render 子树既有测试零回归（若有既有断言依赖原始控制字符的失败项，逐个核对：预期仅 `plain` 含控制字符的旧行为断言，需按新策略更新并在 commit message 记录）。

- [ ] **Step 2.7: Commit**

```bash
git add apps/cli/src/tui/render/output/block_component.rs \
        apps/cli/src/tui/render/output/primitives/wrap.rs \
        apps/cli/src/tui/render/output/rendered.rs \
        apps/cli/src/tui/render/output/blocks/user_message.rs \
        apps/cli/src/tui/render/output/blocks/assistant_message.rs \
        apps/cli/src/tui/render/output/document_renderer/tests.rs
git commit -m "fix(tui): #1670 渲染层三道防线归一化控制字符（分发/wrap/RenderedLine）"
```

---

### Task 3: tool result 路径迁移共享函数，删除本地 `expand_tabs`

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/output_tool_view.rs`（`display_text_for_tool_result` 3 处调用 + 删 `fn expand_tabs` + 新增测试模块）

- [ ] **Step 3.1: 写迁移守护测试（红——当前 `expand_tabs` 行为相同，预期直接绿，作为迁移等价性守护）**

`output_tool_view.rs` 文件末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::display_text_for_tool_result;

    #[test]
    fn test_tool_result_display_text_expands_tab_via_shared_normalize() {
        let content = serde_json::json!({ "display": "col1\tcol2" });
        let text = display_text_for_tool_result(Some("Bash"), "fallback", &content);
        assert_eq!(text, "col1    col2");
    }

    #[test]
    fn test_tool_result_display_text_replaces_escape_via_shared_normalize() {
        // 共享策略增强：ESC 阻断（原 expand_tabs 会原样保留）。
        let content = serde_json::json!({ "display": "a\u{1b}[31m" });
        let text = display_text_for_tool_result(Some("Bash"), "fallback", &content);
        assert_eq!(text, "a\u{fffd}[31m");
    }
}
```

Run: `cargo test -p cli -- view_assembler::output_tool_view`
Expected: 第 1 个 PASS（等价性）、第 2 个 **FAIL**（原 `expand_tabs` 原样保留 ESC）→ 迁移的红驱动。

- [ ] **Step 3.2: 迁移实现**

`output_tool_view.rs` 顶部 use 追加：

```rust
use crate::tui::view_model::display_text::normalize_display_control_chars;
```

三处调用替换（并更新 156-158 行的 #196 注释）：

```rust
    // issue #196/#1670：tool result 文本进入 TUI 渲染前走共享控制字符归一化
    // （\t → 4 空格，\n 保留，其余控制字符 → U+FFFD），策略单一真相见
    // view_model::display_text。
```

- L172: `return expand_tabs(&format!("{message}\n当前分支：{branch}"));`
  → `return normalize_display_control_chars(&format!("{message}\n当前分支：{branch}"));`
- L174: `(Some(message), None) => return expand_tabs(message).to_string(),`
  → `(Some(message), None) => return normalize_display_control_chars(message),`
- L187: `expand_tabs(&text).to_string()`
  → `normalize_display_control_chars(&text)`

删除 `fn expand_tabs`（L189-202，含其 doc 注释，共 14 行）。

- [ ] **Step 3.3: 运行测试确认通过（绿）**

Run: `cargo test -p cli`
Expected: PASS — `output_tool_view` 2 个测试绿，cli lib 全量零回归。

Run: `grep -rn "expand_tabs" apps/cli/src/`
Expected: 零命中（死代码清净，`specs/3.14.7`）。

- [ ] **Step 3.4: Commit**

```bash
git add apps/cli/src/tui/view_assembler/output_tool_view.rs
git commit -m "refactor(tui): #1670 tool result 迁移共享控制字符归一化，删除重复 expand_tabs"
```

---

### Task 4: 全量验证与门禁

- [ ] **Step 4.1: 格式化（MUST 由工具执行，NEVER 手动调整格式）**

Run: `cargo fmt --all && git diff --stat`
Expected: 仅格式化产生的机械变更（若无 diff 亦正常）。

- [ ] **Step 4.2: Clippy 全量**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 零 warning 通过。

- [ ] **Step 4.3: Workspace 全量测试**

Run: `cargo test --workspace`
Expected: 全部通过（含场景测试与既有跨层测试）。

- [ ] **Step 4.4: 架构守卫预跑（pre-push 同套，提前暴露阻断）**

Run: `bash .agents/hooks/check-architecture-guards.sh`
Expected: 全部通过；若阻断，按 `specs/3.14.9` 止血并先修复阻断根因再继续。

- [ ] **Step 4.5: 死代码/废弃路径检查（specs/3.14.7）**

Run: `grep -rn "expand_tabs\|sanitize_for_display" apps/cli/src/`
Expected: `expand_tabs` 仅 1 处测试注释历史提及（`output_tool_view_tests.rs`，非代码调用）；`sanitize_for_display` 零命中。

- [ ] **Step 4.6: 更新 issue 状态为「修复中」**

Run:
```bash
gh issue comment 1670 --repo rushsinging/aemeath --body "状态：修复中。根因修复方案已实施：共享 normalize_display_control_chars + 渲染三道防线（render_self 分发 / wrap 入口 / RenderedLine 构造）+ tool path 迁移删除重复 expand_tabs。复现测试 6 组先行红后绿。"
```

- [ ] **Step 4.7: Commit（如有格式化变更）**

```bash
git add -A && git commit -m "chore(tui): #1670 cargo fmt 机械格式化" --allow-empty
```

---

### Task 5: 文档门禁与 specs 提案（需用户确认）

- [ ] **Step 5.1: 核对 Target 文档（issue 文档与代码双向校验门禁 · 开发前项）**

Target 文档：`specs/3.3-tui-cli.md`（TUI 分片）。当前不符合点：分片无「显示文本控制字符策略」章节，新策略无文档真相源。
`docs/design/**` 无 TUI 渲染控制字符相关章节，无矛盾项。

- [ ] **Step 5.2: 向用户提案新增 specs/3.3 小节（Constitution #3：新增规则 MUST 先征得用户同意）**

提案内容（待用户同意后写入 `specs/3.3-tui-cli.md`，位于 3.3.8 Markdown 间距策略之后新增 3.3.9）：

```markdown
## 3.3.9 显示文本控制字符策略

- 所有进入输出区渲染的文本 **MUST** 经 `view_model::display_text::normalize_display_control_chars` 归一化（单一真相）：`\t` → 4 空格、`\n` 保留、其余控制字符（C0/C1/DEL，含 ESC/C1-CSI）→ `U+FFFD`。
- 渲染防线 **MUST** 覆盖 `render_self` 分发入口、`wrap_spans_with_prefix` 入口、`RenderedLine` 构造（`new`/`with_plain`/`with_plain_and_links`）三层；新增消息块类型 **NEVER** 绕过 `render_self` 分发自行渲染原始文本。
- 零宽格式字符（ZWSP/ZWJ/组合符，非 Cc 类）**保留**。
- tool result display 文本 **MUST** 复用同一函数，**NEVER** 再实现局部 tab 展开。
```

- 用户同意 → 写入文件、勾掉 issue 门禁「开发前/开发中」两 check、commit：
  `git commit -m "docs(specs): #1670 补充 TUI 显示文本控制字符策略"`
- 用户拒绝/暂缓 → 不改 specs，在 PR Test plan 记录「specs/3.3 策略章节为开放项，经确认延期」（对应门禁「完成前」check 的可验证理由路径）。

---

### Task 6: Push 与 PR

- [ ] **Step 6.1: 同步最新主分支（specs/3.14.6 MUST）**

Run: `git pull origin main`
Expected: Already up to date 或产生合并提交（有冲突则逐项解决，**NEVER 丢弃任一侧测试**，Constitution #12）。

- [ ] **Step 6.2: Push（pre-push 自动跑 17 个架构守卫 + workspace 单测）**

Run: `git push -u origin fix/1670-display-control-chars`
Expected: 守卫与单测全过；若被 `--no-verify` 绕过（仅在用户明确要求时），PR Test plan MUST 披露并补跑。阻断时按 `specs/3.14.9` 处理并报告。

- [ ] **Step 6.3: 创建 PR（body-file 方式，PR 模板四段齐全）**

写 `/tmp/pr-1670.md`：

```markdown
## Summary

- 根因修复 TUI 渲染吞 tab / 零宽控制字符（issue #1670）：新增共享 `normalize_display_control_chars` 单一策略（`\t`→4 空格、`\n` 保留、其余 C0/C1/DEL→`U+FFFD` 阻断 ANSI 显示注入）。
- 三道渲染防线：`render_self` 分发入口（源头，先于宽度计算与 markdown links 解析）、`wrap_spans_with_prefix` 入口、`RenderedLine` 构造（保持 `plain == spans` 不变式）。
- tool result 迁移共享函数并删除本地重复 `expand_tabs`（#196 遗留的单点修复）。

## Refs

Closes #1670

## Breaking change

无协议/存储破坏；显示语义变化：非 tab 控制字符由「零宽吞字/原样直通」变为 `U+FFFD`，tool result 中 ESC 等由原样保留变为 `U+FFFD`（issue 要求的增强）。

## Test plan

- [x] TDD：复现测试 6 组先行（wrap / RenderedLine / user message / assistant fenced / L4 场景 ×2）先红后绿
- [x] `cargo test --workspace` 全过
- [x] `cargo clippy --workspace --all-targets -- -D warnings` 零警告
- [x] `cargo fmt --all` 工具格式化
- [x] `bash .agents/hooks/check-architecture-guards.sh` 守卫通过
- [x] 死代码检查：`expand_tabs` 全仓零命中（specs/3.14.7）
- [x] 文档核对：`specs/3.3-tui-cli.md` 策略章节 —（已写入 / 开放项经确认延期，见 issue 门禁记录）
```

Run:
```bash
gh pr create --repo rushsinging/aemeath \
  --base main --head fix/1670-display-control-chars \
  --title "fix(tui): 根因修复渲染吞 tab 与控制字符归一化（#1670）" \
  --body-file /tmp/pr-1670.md
```

- [ ] **Step 6.4: 报告并等待 review**

向用户报告 PR URL、issue 门禁清单闭合情况；**NEVER 自行合并**（specs/3.14.6，合并需当前会话对具体 PR 的明确授权）。

---

## Self-Review 记录

1. **Spec（issue #1670）覆盖**：现象（复现测试 Task2）→ 根因（防线注释与 specs 提案）→ 修复五条策略（Task1 表格逐条对应）→ 验证（Task2/3/4 命令）→ 涉及路径（全部出现）✅；文档门禁三 check（Task5）✅。
2. **占位符扫描**：无 TBD/TODO；所有代码步骤给出完整代码；所有命令给出期望输出 ✅。
3. **类型一致性**：`normalize_display_control_chars(&str) -> String` 在 Task1 定义，Task2/3 引用一致；`normalize_span_texts(Vec<Span<'static>>) -> Vec<Span<'static>>` 定义于 rendered.rs、wrap.rs 引用一致；`normalized_text_block_view(&TextBlockView) -> TextBlockView` 定义与 5 个调用点一致 ✅。
