# TUI Block 抽象 trait 化 + 真正渲染树（嵌套规则）设计（Feature #60）

**日期**：2026-05-29
**关联**：feature #58（输出区渲染管线统一，本设计的直接前置，已落地待确认）、#59（单源迁移 roadmap，正交）；bug #65 / #76（tool result 压平丢样式，本设计从根修复）

## 背景与定位

#58 已把输出区渲染统一为 `ConversationModel → OutputViewAssembler → OutputViewModel → OutputDocumentRenderer → RenderedDocument → OutputArea` 管线，恢复 markdown+theme，建立 block 级缓存与 `RenderedLine{spans, plain}` 双轨。

但 #58 有两处明确留白，本设计接续完成：

1. **trait 化**：#58 §68 写明 "BlockRenderer trait 为概念描述（可不落地为 trait）"，当前是自由函数 `render_block(kind, id, ctx)` + match 分发。本设计升为正式 `BlockComponent` trait，承载模板 + 类型层不变式。
2. **嵌套**：#58 非目标写明 "只做顶层 block 组件化"，tool result 至今被 fold 进 `ToolCallBlockView.result_summary`（String），丢失 markdown/diff/语法。本设计引入**真正的渲染树**：block 可含子 block，子块由各自组件富渲染。

**本质前提**：TUI 终端只有 cell 行网格。"树/嵌套/缩进"均为**组装与渲染期的逻辑外壳**，最终仍 DFS 展平成扁平 `Vec<RenderedLine>` 交 ratatui 画。本设计不改变"行渲染"本质，只为行加结构。

## 目标

- `BlockComponent` trait 统一模板：每个 block 组件输入/输出/缓存键/不变式由 trait 约束。
- ViewModel 升为树：`BlockNode{ block_id, block_version, kind, children }`。
- 渲染器递归走树、逐层缩进、DFS 展平。
- 显式**嵌套规则表**：定义合法父子组合、最大深度，构造期校验。
- 从根修复 #65（fence 跨块泄漏）、#76（thinking 后 result 扁平）。

## 非目标

- 不改 #58 的选区/复制/缓存机制本身（复用，仅适配树）。
- 不做 markdown 元素级子组件（段落/代码块仍用共享原语函数，沿用 #58）。
- 不动 input area、status bar、spinner/task live tail（spinner/task 属 #59 S1）。
- 不引入运行时 Theme（沿用 #58 编译期 `theme::*` 常量，`RenderCtx` 仅 width）。

## 设计

### §1 BlockComponent trait（模板 + 不变式）

```rust
pub trait BlockComponent {
    /// 自身语义指纹 → 该 block 自身内容的 cache version（不含子）。
    fn cache_version(&self) -> u64;
    /// 仅渲染自身内容（不含子块），产出 depth=0 的行。
    /// 不变式：每行 plain == spans 可见文本拼接（各 impl 单测断言）。
    fn render_self(&self, ctx: &RenderCtx) -> Vec<RenderedLine>;
}
```

- 各 `*BlockView`（`TextBlockView`、`ToolCallBlockView`、`AskUserBlockView`、`Separator` 等）impl 之，`render_self` 复用 #58 现有 `render_xxx` 函数体。
- `OutputBlockKind::component(&self) -> &dyn BlockComponent` 做 enum → trait 分发，取代 `blocks/mod.rs` 的 match 与 assembler 的 `semantic_version` 手算 hash。
- **缩进不在组件内**：组件永远产 depth=0 的行，缩进由渲染器在组合期施加 → 同组件可作根或子、可独立测试、可复用。

### §2 树节点 ViewModel

```rust
pub struct BlockNode {
    pub block_id: BlockId,
    pub block_version: u64,   // = kind.component().cache_version()，仅自身
    pub kind: OutputBlockKind,
    pub children: Vec<BlockNode>,
}
pub struct OutputViewModel {
    pub roots: Vec<BlockNode>,
    pub version: u64,
    pub follow_tail_hint: bool,
}
```

### §3 渲染器递归 + 缩进（DFS 展平）

```text
render_document(roots, ctx):
    for root in roots: render_node(root, ctx, depth=0) → push 到 RenderedDocument.blocks

render_node(node, ctx, depth):
    self_lines = cache.get_or_render(node.block_id, key=(node.block_version, width)) {
        node.kind.component().render_self(ctx)   // 缓存"未缩进"行
    }
    emit RenderedBlock{ block_id: node.block_id, lines: indent(self_lines, depth) }
    for child in node.children:
        render_node(child, ctx, depth + 1)
```

- 产物仍是扁平 `RenderedDocument{ blocks: Vec<RenderedBlock> }`，每个 `BlockNode` → 一个 `RenderedBlock`（保持 block_id 粒度，利于缓存 retain 与 root 边界识别）。
- 缩进 = 每层一个 `INDENT`，累积；**只施加到 spans，不进 plain**（见 §6）。

### §4 嵌套规则表（吸收方案 C）

```rust
fn allowed_child(parent: &OutputBlockKind, child: &OutputBlockKind) -> bool;
const MAX_BLOCK_DEPTH: usize = 3;   // top → tool_call → result-content
```

初版规则：

| 父 | 允许的子 |
|---|---|
| ToolCall | AssistantMessage（result 文本走 markdown）、Diff（Edit/Write diff result）、Diagnostic/SystemNotice（成功/错误摘要）、(未来) Image |
| 其余所有 block | 无子（叶子） |

- AgentProgress 含子 ToolCall（子 agent 工具调用）标记为**后续可选**，初版不做。
- **校验时机**：`OutputViewAssembler` 建树时校验每条父子边与深度。非法组合：release 降级为叶子（丢弃非法子）+ `log::warn`，debug `panic`；深度超限同处理。
- 新增 guard 测试覆盖非法组合与超深。

### §5 缓存策略（决策：不折叠）

- **父 version 不折叠子 version**：每 `BlockNode` 独立缓存自身 `self_lines`，key = `(block_version_自身, width)`，**depth 不进 key**（缩进取出后施加 → 同组件跨 depth 复用缓存）。
- 收益：流式时子块（如 result）追加只重渲子块，父块（tool_call 头）命中缓存不动。
- `BlockCache.retain(live_ids)`：`live_ids` 改为 **DFS 收集全树**的 `block_id`，防子块缓存泄漏。
- 与 #58 block 级缓存一致，仅遍历从扁平改 DFS。沿用 #71 结论：key 是 block_id 非行下标，无越界。

### §6 选区 / plain / MAX_LINES（决策：缩进不进 plain）

- 渲染器 DFS 展平为扁平行序列 → **选区/复制路径复用 #58 不变**（`apply_selection_overlay`、`screen_line_map`、plain 字符切片照用）。
- **缩进只进 spans，不进 plain**：
  - `spans` 含前导缩进空格（屏幕有缩进）；`plain` 不含缩进 → 复制内容是纯逻辑文本，无装饰性前导空格。
  - 适配点：`screen_line_map` 的列偏移与 `apply_selection_overlay` 的 `SelRange` 需把"缩进显示宽度"作为显示偏移补偿，区别于 `plain` 字符偏移。即：屏幕列 → 减去缩进宽度 → 映射到 plain 字符偏移。
- MAX_LINES 裁剪：改为按"最旧的 **root 子树**整组"丢弃（现 `trim_blocks_to_max_lines` 按单 RenderedBlock）。需让 trim 识别 root 边界（RenderedBlock 标注其所属 root，或按 root 分组裁剪）。

### §7 assembler 改造

- `OutputViewAssembler::assemble_from_conversation` 产 `Vec<BlockNode>`。
- ToolCall：不再 fold `result_summary` 字符串；把 `ConversationBlock::ToolResult` 解析成 children `BlockNode`——按 tool 类型选子组件：
  - Edit/Write 的 diff → `Diff` 子块；
  - 文本结果 → `AssistantMessage`(markdown) 子块；
  - 成功/错误摘要 → `Diagnostic/SystemNotice` 子块。
- 保留 `tool_result_is_embedded` 决定 result 作子节点而非顶层块。
- 删除 `semantic_version`（由 `cache_version` 取代）。

### §8 #65 / #76 从根修复

- **#65**（fence 跨块泄漏）：result 走独立 markdown 子块，fence 状态机在子块内闭合，不泄漏到后续块。
- **#76**（thinking 后 grep 扁平 + 滚动条失效）：result 走子块富渲染（带工具头/缩进/折叠），不再扁平原始行；滚动随行序列正常。
- 二者纳入回归测试，完成后做 bug 追踪联动。

### §9 测试

- **trait impl**：每组件 `render_self` 的 spans 样式 + plain 文本；`cache_version` 同输入稳定、异输入不同。
- **建树**：assembler 产出正确父子结构；非法父子被拒（降级/panic）；深度超限处理。
- **渲染器**：递归逐层缩进正确；DFS 展平顺序；子块 version 变只重渲子块（spy 渲染器计数）；retain 删全树消失 block。
- **选区/复制**：缩进不进 plain（复制无前导空格）；屏幕列→plain 偏移补偿缩进宽度（含 CJK）；叠加只改 bg 保留 fg（#61 回归）。
- **回归**：#65、#76，以及 #58 既有选区/缓存测试全过。

### §10 guard

- 新增/扩展架构 guard：
  - 组件 `render_self` 内禁止施加缩进（缩进唯一在渲染器 `indent()`）。
  - 禁止绕过 `BlockComponent` 直接拼 block 行。
  - 嵌套合法性 + 深度由 assembler 校验路径覆盖（guard 检查校验函数被调用 / 无旁路建树）。
- 接入 `.agents/hooks/check-architecture-guards.sh`。

## 增量迁移步骤（每步独立编译 + 测试通过）

1. **trait 落地**：定义 `BlockComponent`，各 view impl（`render_self` 复用现有 `render_xxx` 函数体），`render_block` 分发改 trait。扁平结构与行为不变。
2. **引入树**：ViewModel 加 `children`（暂空），渲染器改递归 `render_node`（depth=0）。行为不变。
3. **嵌套规则表 + 校验 + guard**：`allowed_child`、`MAX_BLOCK_DEPTH`、assembler 建树校验、guard。
4. **ToolCall result 改子节点**：assembler 把 ToolResult 解析成 Diff/Markdown/Diagnostic 子块；删 `result_summary` 字符串路径；缩进施加 + plain 解耦（§6）。**本步修 #65/#76**。
5. **缓存与裁剪适配**：`retain` 改全树 DFS；MAX_LINES 按 root 子树裁剪。

## 涉及路径

- 新增：`render/output/block_component.rs`（trait + enum 分发）、嵌套规则模块（`allowed_child`/`MAX_BLOCK_DEPTH`）。
- 改造：`view_model/output.rs`（`OutputBlockView`→`BlockNode` 树）、`view_assembler/output.rs`（产树 + ToolResult 解析子块 + 校验）、`render/output/document_renderer.rs`（递归 + DFS 展平 + retain 全树）、`render/output/blocks/*`（impl trait）、`render/output_area/`（缩进/plain 偏移补偿、root 边界裁剪）。
- 删除：`view_assembler` 的 `semantic_version`、`ToolCallBlockView.result_summary` 字符串渲染路径。
- guard：新增 block 组件/嵌套 isolation guard，接入架构守卫。
