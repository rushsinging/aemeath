# TUI 输出区渲染管线统一重构设计

**日期**：2026-05-29
**关联**：feature #53 / #55 / #57（TUI Model/View 迁移与目录收口）、bug #61 / #51 / #48 / #60 / #71；spec `2026-05-27-tui-model-view-architecture.md`

## 背景与问题

#57 完成后，渲染已物理收进 `render/`，但输出区（spinner 上方内容）的渲染管线仍是有损的：

```
ConversationModel.blocks
 → view_assembler/output.rs  OutputViewModel{ blocks, SemanticStyle }
 → render/output_view_model.rs  output_view_model_lines  ← 只切行 + 粗语义色，【不解析 markdown】
 → adapter/output_widget.rs  replace_lines_from_view_model  ← 【line_to_plain_text 拍平成纯文本，丢所有 span/style；强制 LineStyle::Normal】
 → output_area.push_line(OutputLine{ content: 纯文本, style: Normal })
 → render/output_area/render.rs  render_with_cache
```

成熟的富渲染引擎 `render/output/{markdown,line,block,span,syntax,diff}` 被完全绕过。

**两个 bug 的同一根因**（`adapter/output_widget.rs`）：
1. `line_to_plain_text` 拼接所有 span 成纯字符串 → 丢失语义色/主题色（**没 theme 颜色**）。
2. 写死 `LineStyle::Normal` → output_area 的 markdown 富渲染按 LineStyle 判定，Normal 不触发（**纯文本无 markdown**）。

外加上游 `output_view_model_lines` 本身不渲染 markdown：两处叠加，存在三套表示（ViewModel blocks / ViewModel-lines 纯文本 / OutputLine+markdown）互相拍平。

## 目标

统一为**单一 ViewModel → Render 管线**，恢复 markdown + theme，并消除有损桥与双表示。范围限**输出区**。

## 非目标（Out of Scope）

- **用户输入编辑区（input area widget）渲染 / 选中**：指底部用户敲字的输入框，非输出区里的用户消息回显。经核实，input 侧（`render/input/input_area/{render,selection}.rs`）不引用 `OutputLine` / `output_view_model` / output 的 markdown·syntax 原语，是独立管线；其"单一真相/选中"由 **feature #56** 跟踪，本设计不动 input area。仅共享 util（如 `string_idx::CharIdx`）各自复用，不耦合。（注：输出区里的**用户消息回显**与 **input queue 显示**在本设计范围内，见专节。）
- markdown 元素级组件化（段落/代码/表格做成子组件）——本设计只做顶层 block 组件化，markdown 内部用共享原语函数。
- 状态栏 / dialog / completion popup 渲染。

## 架构与数据流

```
ConversationModel.blocks
 → OutputViewAssembler → OutputViewModel{ blocks: Vec<OutputBlockView>, version }
      每 OutputBlockView 带 block_id + block_version（脏判定）+ 语义数据
 → OutputDocumentRenderer（render/output/）
      逐 block：查 block 级缓存 → 命中复用，否则交对应 BlockRenderer 组件渲染
 → RenderedDocument{ lines: Vec<RenderedLine> }
      RenderedLine{ spans: Vec<Span>（显示，含 markdown/语法/theme 色）, plain: String（逻辑纯文本，选中/复制用）}
 → OutputArea（显示容器：viewport / scroll / selection 叠加 / spinner）
      不持有业务文本，只持 RenderedDocument + 视图状态
```

**职责边界**（对齐 spec 分层）：
- ViewAssembler：Model → ViewModel（语义，无渲染）。
- OutputDocumentRenderer + BlockRenderer 组件：ViewModel → RenderedDocument（markdown/语法/theme/diff 都在此层）。
- OutputArea：RenderedDocument → 屏幕（layout/scroll/selection），不碰业务、不解析 markdown。

**数据表示变化**：废弃 `OutputLine{ content, style }`，换 `RenderedLine{ spans, plain }`（显示与逻辑文本分离，是富渲染 + 可复制选中的基础）。

## 组件系统（顶层 block 级）

```rust
struct RenderCtx<'a> { width: u16, theme: &'a Theme }   // 不含 selection：渲染产物与选区解耦
struct RenderedBlock { lines: Vec<RenderedLine> }
trait BlockRenderer {
    fn render(&self, block: &OutputBlockView, ctx: &RenderCtx) -> RenderedBlock;
}
```

- `OutputDocumentRenderer` 按 `OutputBlockView` 变体分发到组件。
- 组件清单（一一对应现有 block kind）：`UserMessageRenderer`、`QueuedSubmissionRenderer`、`AssistantMessageRenderer`、`ThinkingRenderer`、`ToolCallRenderer`、`Diagnostic/SystemRenderer`、`SeparatorRenderer`。
- `AssistantMessageRenderer` 调**共享 markdown 原语**（`render/output/{markdown,syntax,diff,table}` 改造为 `fn(text, ctx) -> Vec<RenderedLine>` 纯函数）；markdown 元素不做子组件。
- `ToolCallRenderer` 复用现有 `tool_display` 格式化逻辑。
- 选区不进 `RenderCtx`：渲染只依赖 (block 数据, width, theme)，缓存 key 干净，选区变化不触发重渲染。

**模块布局**：`render/output/blocks/`（每组件一文件，天然满足 ≤400 行）、`render/output/primitives/`（markdown/syntax/diff/table 共享原语）、`OutputDocumentRenderer` 在 `render/output/mod.rs`。

## 用户输入回显与 input queue（输出区）

输出区同时显示用户已发送消息和排队中输入，二者都纳入本管线：

- **用户输入回显**：`ConversationBlock::UserMessage` → `OutputBlockView::UserMessage` → `UserMessageRenderer`。已覆盖。
- **input queue（排队中输入）**：新增独立 block 与样式，避免与已发送消息混淆：
  - ViewModel 新增 `OutputBlockView::QueuedSubmission`；`OutputViewAssembler` 把 `ConversationBlock::QueuedUserMessage` 映射为 `QueuedSubmission`（**不再复用 UserMessage**）。
  - `QueuedSubmissionRenderer` 以暗色 + 「排队中」标记渲染，视觉区分于已发送用户消息。
  - agent 取用该排队项后，ConversationModel 将其转为正式 `UserMessage` block（已有 `QueuedUserMessage` 的 retain/clear 生命周期，model.rs:296/310），ViewModel 随之从 QueuedSubmission 变为 UserMessage。
- **删除 legacy 双表示**：`render/output_area/queued.rs`（`build_queued_message_lines`）、OutputArea 的 `queued_messages`、`queued_line_count` 及 streaming 重渲时保留排队行的逻辑全部删除——排队显示只由 ConversationModel block 经 ViewModel 驱动，单一真相。

## 缓存与流式

```
cache: HashMap<BlockId, CachedBlock>
CachedBlock{ key: CacheKey, rendered: RenderedBlock }
CacheKey = (block_version, width, theme_version)
```

- 遍历 ViewModel.blocks：key 一致 → 复用（零渲染）；否则重渲染并更新缓存；ViewModel 中已不存在的 block_id → 清除缓存（防泄漏）。
- `OutputViewAssembler` 组装时，block 内容（text/状态/tool result）变就 bump 该 block version；未变不动。**streaming 时只有正在追加的 block version 在变 → 只它重渲染**，其余命中缓存。
- width/theme 变 → 全 key 失效 → 全量重渲染一次（低频，可接受）。
- **与 #71 的关系**：旧行级 `rendered_cache`（按行下标、render_start/end 越界 panic）被 block 级缓存取代——key 是 block_id 非行下标，裁剪/增长不产生陈旧下标，从结构上消除该类越界。`MAX_LINES` 裁剪改为"丢弃最旧整个 block"。

## 选中 / 复制

- **显示与逻辑分离**：`RenderedLine{ spans, plain }`，不变式 `plain == spans 可见文本拼接`（组件产出时保证 + 单测断言）。
- **选区模型**：存 `(行号, plain 内字符偏移)` 起止，基于**字符**（非字节），CJK 宽字符按 1 单位；复制时用 `string_idx::CharIdx` 映射回 `plain` 字节切片（避免 #48 CJK 偏移错）。
- **屏幕坐标 → 选区**：OutputArea 维护"屏幕行 → (RenderedLine 索引 + 起始可见列)"映射，再把可见列按宽字符列宽换算成 `plain` 字符偏移。
- **高亮叠加（修 #61）**：统一 `apply_selection_overlay(line, sel_range) -> Vec<Span>`——只对选区内字符设 `bg(selection)`，**保留原 span 前景色**，必要时按选区边界 split span。所有 block 类型共用，杜绝绕过选区叠加的旁路（#61/#62 同族）。
- **复制**：取选区各行 `plain` 的字符切片拼接（行间 `\n`），与显示无关 → 复制内容永远干净逻辑文本（修 #51/#60）。

## 增量迁移步骤（方案 A：逐 block 切换）

每步独立编译 + 测试通过，旧路径在对应 block 全切后才删：

1. **建新表示与骨架（不接线）**：`RenderedLine`/`RenderedBlock`/`RenderCtx`/`BlockRenderer`/`OutputDocumentRenderer`（空分发）/block 级缓存，与现状并存。
2. **共享原语就位**：`render/output/{markdown,syntax,diff,table}` 改造/包成 `fn(text, ctx) -> Vec<RenderedLine>`（产 spans+plain），先有单测。
3. **逐 block 组件切换**（每步切完即删该类旧渲染）：`Separator/System/Diagnostic` → `UserMessage` → `AssistantMessage`（**这步恢复 markdown+theme**）→ `Thinking` → `ToolCall`。
4. **OutputArea 换血**：行容器 `Vec<OutputLine>` → `RenderedDocument`；render 直接画 spans；接入选区叠加 + plain 复制。
5. **删除旧路径**：`adapter/output_widget.rs` 拍平桥、`render/output_view_model.rs` 纯文本路径、`OutputLine`、旧行级 `rendered_cache`。
6. **收紧 guard**：补 render isolation guard（spec §843）——`render/output` 组件不得引用 Model 可变类型、不得做 IO；确保选区叠加是唯一上色路径（防 #61 回归）。

## 删除验证门禁（硬要求）

迁移**必须真正删除旧代码，不留死代码**。验证方式：

- 每步切换后，grep 确认该 block 的旧渲染分支已移除，无 `#[allow(dead_code)]` 掩盖的残留。
- 最终步后，全仓 grep **必须为零命中**：`OutputLine`、`replace_lines_from_view_model`、`output_view_model_lines`、`render/output_view_model.rs`、旧 `rendered_cache` 类型与其 `LineStyle`-based markdown 判定、以及 legacy 排队机制 `build_queued_message_lines`/`queued_messages`/`queued_line_count`。
- `cargo build` 无 `dead_code`/`unused` 警告（针对被替换模块）；`adapter/output_widget.rs`、`render/output_view_model.rs` 文件应被删除而非留空。
- 顶层目录白名单 guard（#57）与新 render isolation guard 一并通过。

## 测试

- **组件级**（纯函数，最高价值）：每 BlockRenderer 断言产出 `spans` 样式与 `plain` 文本；覆盖正常/空/超长换行。
- **markdown 原语**：代码块语法色、表格对齐、unified diff 行号+加减色、CJK 宽字符——golden 断言 spans 与 plain。
- **不变式**：每行 `plain == spans 可见文本拼接`。
- **缓存**：同 key 命中复用（spy 计数渲染器未被调用）；block_version 变只重渲该 block；block 删除后缓存清除；width/theme 变全失效。
- **选区**：屏幕坐标→字符偏移（含 CJK 列宽）、跨行复制正确、叠加只改背景不动前景（断言 span.fg 不变）、空选区。
- **回归**：#61、#51/#48/#60、#71，以及本次两 bug（markdown + theme 恢复）。

## 涉及路径

- 新增：`render/output/blocks/*`、`render/output/primitives/*`、`render/output/mod.rs`（OutputDocumentRenderer）、`render/output/rendered.rs`（RenderedLine/RenderedBlock/RenderedDocument）。
- 改造：`view_assembler/output.rs`（block_id/version）、`render/output_area/`（OutputArea 显示容器 + 选区叠加）、`render/output/{markdown,syntax,diff,table}`（改纯函数原语）。
- 删除：`adapter/output_widget.rs`、`render/output_view_model.rs`、`OutputLine` 及旧行级 `rendered_cache`、`render/output_area/queued.rs` 及 OutputArea 的 `queued_messages`/`queued_line_count`。
- ViewModel：`view_model/output.rs` 新增 `OutputBlockView::QueuedSubmission`；`view_assembler/output.rs` 把 `QueuedUserMessage` 映射到它。
- guard：新增 render isolation guard，接入 `.agents/hooks/check-architecture-guards.sh`。
