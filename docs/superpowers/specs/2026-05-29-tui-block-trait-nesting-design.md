# TUI Block 抽象 trait 化 + 真正渲染树（嵌套规则）设计（Feature #60）

**日期**：2026-05-29（2026-05-29 二次审查后修订，详见 §0）
**关联**：feature #58（输出区渲染管线统一，本设计的直接前置，已落地待确认）、#59（单源迁移 roadmap，正交）；bug #65（与之相关，main 已结构性隔离，本设计仅进一步加固，**不认领修复**）

## §0 现状核对修订说明（2026-05-29）

本 spec 初稿假设 tool result 仍被压平成字符串、#65/#76 未修。对照当前 main 实测后修正如下，以本节为准：

1. **tool result 已富渲染（非压平）**：`tool_call.rs::format_result_lines` 已走共享原语 `render_fenced_markdown`（`primitives/fenced.rs`）；Edit diff 已走 `blocks/edit_diff.rs::render_edit_diff` + `primitives/unified_diff.rs`，带行号/语义色/语法高亮。当前形态是**内联进 tool_call 同一块的 lines**，不是子块。
2. **#65 已被 main 结构性隔离**（状态：待确认）：`render_fenced_markdown` 的 fence 状态机是函数局部、随调用销毁，天然不跨块泄漏。→ 本设计**不再声称"从根修复 #65"**，子块化只是进一步把 result 隔离成独立缓存单元。
3. **#76 与本设计无关**：其 main 根因是 #58 域的 legacy 直写 vs ViewModel 全量替换双路径 + ToolCall index 丢失，归 #58/#76 处理。→ 本设计**完全摘除 #76**。
4. **缩进现状进 plain**：`render_fenced_markdown` 的 `indent` 参数注释明写"加在每行最前且保证 plain 与可见 spans 一致"——即现状缩进**进** plain。本设计决策为"缩进/gutter **不**进 plain"，需改该原语契约（见 §6）。

**因此 #60 的真实价值定位**（非修 bug）：① gutter 行首标志槽（main 完全没有的新能力）；② BlockComponent trait 抽象（代码组织）；③ result 由"内联行"升为"子块树"（结构性收益：独立缓存粒度、gutter 按 depth 对齐、组合更干净）。

## 背景与定位

#58 已把输出区渲染统一为 `ConversationModel → OutputViewAssembler → OutputViewModel → OutputDocumentRenderer → RenderedDocument → OutputArea` 管线，恢复 markdown+theme，建立 block 级缓存与 `RenderedLine{spans, plain}` 双轨。

#58 有两处明确留白，本设计接续完成：

1. **trait 化**：#58 §68 写明 "BlockRenderer trait 为概念描述（可不落地为 trait）"，当前是自由函数 `render_block(kind, id, ctx)` + match 分发。本设计升为正式 `BlockComponent` trait，承载模板 + 类型层不变式。
2. **嵌套**：#58 非目标写明 "只做顶层 block 组件化"，tool result 当前虽已富渲染（§0.1），但仍以"内联行"形式拍进 tool_call 同一块。本设计引入**真正的渲染树**：block 可含子 block，子块复用现有富渲染原语（`render_edit_diff`/`render_fenced_markdown`）作为独立组件。

**本质前提**：TUI 终端只有 cell 行网格。"树/嵌套/缩进"均为**组装与渲染期的逻辑外壳**，最终仍 DFS 展平成扁平 `Vec<RenderedLine>` 交 ratatui 画。本设计不改变"行渲染"本质，只为行加结构。

## 目标

- `BlockComponent` trait 统一模板：每个 block 组件输入/输出/缓存键/不变式由 trait 约束。
- ViewModel 升为树：`BlockNode{ block_id, block_version, kind, children }`。
- 渲染器递归走树、逐层缩进、DFS 展平。
- 显式**嵌套规则表**：定义合法父子组合、最大深度，构造期校验。
- **gutter 行首标志槽**：统一缩进 + 状态 marker（main 无此能力）。
- （非目标，明确不认领：#65 已由 main 结构性隔离，本设计仅进一步加固；#76 属 #58 域，与本设计无关——见 §0）

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
- 组合期对每行前置 **gutter**（`[depth 缩进] + [marker 列]`，详见 §6.5）；**只施加到 spans，不进 plain**（见 §6）。

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
  - **⚠️ 与 main 现状冲突，需改原语契约**：现状 `primitives/fenced.rs::render_fenced_markdown(indent, ...)` 把 `indent` 拼进每行**且进 plain**（其注释明写"保证 plain 与可见 spans 一致"）。本决策要求 indent 不进 plain，故需改造：
    - 方案：**把 indent 参数从 `render_fenced_markdown` 移除**（原语只产 depth=0、无缩进的纯内容行，spans/plain 均不含缩进）；缩进/gutter 统一由渲染器组合期施加到 spans（§3/§6.5）。
    - 连带影响 assistant 路径（也调该原语，现传 `""` 空缩进）——因传空，行为不变，但其 `indent` 入参一并删除（DRY）。
    - tool_call 内联调用点（`format_result_lines` 传 `INDENT`）随 result 改子块后由 gutter 机制接管，原 `INDENT` 参数移除。
    - 该原语契约变更是本设计相对初稿的**新增工作量**，列入迁移步骤。
- MAX_LINES 裁剪：改为按"最旧的 **root 子树**整组"丢弃（现 `trim_blocks_to_max_lines` 按单 RenderedBlock）。需让 trim 识别 root 边界（RenderedBlock 标注其所属 root，或按 root 分组裁剪）。

### §6.5 行首标志槽 gutter（决策：仅首行 marker + 后续纯空白，静态）

把"状态标志 + 缩进"统一为每个 block 行首的**固定宽度槽位（gutter）**：

- **gutter 组成**：`[depth 缩进] + [marker 列]`。`marker 列`固定宽度 `GUTTER_WIDTH`（建议 2：字形 + 空格），所有 block 共用同一宽度 → 内容左边缘对齐；嵌套块按 depth 叠加缩进后再接 marker 列。
- **marker 来源（按 block kind）**：
  - 有状态的块（ToolCall）：按状态映射字形 ●运行 / ✓成功 / ✗失败 / –取消 / ?孤儿（复用现 `map_tool_status`）。
  - 无状态的块（UserMessage / AssistantMessage / Thinking / Diagnostic 等）：用固定 kind 字形（初版建议：UserMessage `>`、其余可空 gutter，具体字形 review 时定）。
  - 全部**静态**：marker 只随 block 状态变；状态已纳入 `block_version`，故无需动画帧、无缓存失效问题（不引入 #59 S1 的 tick）。
- **仅首行画 marker**：block 第一行 gutter 显示 marker 字形；**后续行 gutter 为等宽纯空白**（无竖线续接）。
- **施加时机 = 组合期（与缩进同源）**：gutter（缩进 + marker）在渲染器组合/展平期前置到每行，**不进缓存的 `render_self` 内容、不进 `plain`**（与 §5 §6 一致）。组件 `render_self` 永远产"无 gutter 的纯内容行"；marker 字形由渲染器在组合期现读 node 的 kind/status 决定。
- **与现状关系**：现 `tool_call.rs` 把 `● `/`✓ ` 直接写进首行 spans——迁移后改为不在组件内写 marker，统一由 gutter 机制注入，使 UserMessage 等也获得一致的对齐 gutter。
- **plain 一致性**：marker 与缩进均为显示装饰，不进 plain → 复制内容不含 marker / 前导空格（§6 列偏移补偿需把 `GUTTER_WIDTH + depth 缩进` 一并算作显示偏移）。

### §7 assembler 改造

- `OutputViewAssembler::assemble_from_conversation` 产 `Vec<BlockNode>`。
- ToolCall：把当前**内联进 tool_call lines 的 result 渲染**（§0.1）改为 children `BlockNode`——**复用现有原语作为子组件，不新建渲染逻辑**：
  - Edit/Write 的 diff → `Diff` 子块（内部调 `render_edit_diff`）；
  - 其余文本结果（含 fenced/表格/markdown）→ result 子块（内部调 `render_fenced_markdown`）；
  - 纯成功/错误摘要 → `Diagnostic/SystemNotice` 子块。
- 相应从 `tool_call.rs::render_tool_call` 移除内联 result 渲染分支（`render_edit_diff`/`format_result_lines` 调用），父块只渲 header + args detail。
- 保留 `tool_result_is_embedded` 决定 result 作子节点而非顶层块。
- 删除 `semantic_version`（由 `cache_version` 取代）；`ToolCallBlockView.result_summary` 字段去留：result 改子块后父块不再用它渲染，但 assembler 仍需读 result 文本来建子块——保留字段作为子块数据来源，仅移除父块的字符串渲染路径。

### §8 与 #65 / #76 的关系（不认领修复）

- **#65**（fence 跨块泄漏）：**main 已结构性隔离**（`render_fenced_markdown` 局部状态机，状态：待确认）。本设计把 result 拆为独立子块后，进一步把其 fence 渲染封进独立缓存单元——**加固而非修复**。回归测试保留"result 含 fence 后续块不变色"断言，但不在 bug 追踪上认领 #65 为本设计修复。
- **#76**（thinking 后 grep 扁平 + 滚动失效）：根因在 #58 域（双路径 + index 丢失），**与本设计无关，完全摘除**，不纳入本设计测试与追踪。

### §9 测试

- **trait impl**：每组件 `render_self` 的 spans 样式 + plain 文本；`cache_version` 同输入稳定、异输入不同。
- **建树**：assembler 产出正确父子结构；非法父子被拒（降级/panic）；深度超限处理。
- **渲染器**：递归逐层缩进正确；DFS 展平顺序；子块 version 变只重渲子块（spy 渲染器计数）；retain 删全树消失 block。
- **选区/复制**：缩进不进 plain（复制无前导空格）；屏幕列→plain 偏移补偿缩进宽度（含 CJK）；叠加只改 bg 保留 fg（#61 回归）。
- **原语契约变更**：`render_fenced_markdown` 去 indent 后，产出行 spans/plain 均不含缩进；assistant 路径行为不变（原传空缩进）。
- **回归**：#65 加固断言（result 含 fence 后续块不变色）；#58 既有选区/缓存测试全过。**不含 #76**（已摘除）。

### §10 guard

- 新增/扩展架构 guard：
  - 组件 `render_self` 内禁止施加 gutter（缩进 + marker 唯一在渲染器组合期；组件不写 marker 字形、不加前导缩进）。
  - 禁止绕过 `BlockComponent` 直接拼 block 行。
  - 嵌套合法性 + 深度由 assembler 校验路径覆盖（guard 检查校验函数被调用 / 无旁路建树）。
- 接入 `.agents/hooks/check-architecture-guards.sh`。

## 增量迁移步骤（每步独立编译 + 测试通过）

1. **trait 落地**：定义 `BlockComponent`，各 view impl（`render_self` 复用现有 `render_xxx` 函数体），`render_block` 分发改 trait。扁平结构与行为不变。
2. **引入树**：ViewModel 加 `children`（暂空），渲染器改递归 `render_node`（depth=0）。行为不变。
3. **原语去 indent 契约变更**：`render_fenced_markdown` 移除 `indent` 参数（产无缩进行，spans/plain 均不含缩进）；assistant 路径同步（原传空缩进，行为不变）；tool_call 内联调用点暂保留缩进由调用方拼（过渡，第 5 步随子块化移除）。先有原语单测。
4. **gutter 收口**：渲染器组合期统一注入 gutter（depth 缩进 + marker 列，§6.5）；组件 `render_self` 去掉自写 marker/缩进；plain 解耦 + 列偏移补偿；各 block kind 的 marker 字形映射。这步让 UserMessage 等获得一致 gutter。
5. **嵌套规则表 + 校验 + guard**：`allowed_child`、`MAX_BLOCK_DEPTH`、assembler 建树校验、guard。
6. **ToolCall result 改子节点**：assembler 把 ToolResult 解析成 Diff/result 子块（复用 `render_edit_diff`/`render_fenced_markdown`）；移除父块内联 result 渲染分支与 `INDENT` 拼接。**结构性加固 #65（非认领修复）**。
7. **缓存与裁剪适配**：`retain` 改全树 DFS；MAX_LINES 按 root 子树裁剪。

## 涉及路径

- 新增：`render/output/block_component.rs`（trait + enum 分发）、嵌套规则模块（`allowed_child`/`MAX_BLOCK_DEPTH`）。
- 改造：`view_model/output.rs`（`OutputBlockView`→`BlockNode` 树）、`view_assembler/output.rs`（产树 + ToolResult 解析子块 + 校验）、`render/output/document_renderer.rs`（递归 + DFS 展平 + retain 全树）、`render/output/blocks/*`（impl trait；`tool_call.rs` 移除内联 result 渲染）、`render/output/primitives/fenced.rs`（移除 `indent` 参数，产无缩进行）、`render/output_area/`（缩进/plain 偏移补偿、root 边界裁剪）。
- 复用（不重写）：`render/output/blocks/edit_diff.rs::render_edit_diff`、`primitives/unified_diff.rs`、`primitives/fenced.rs` 作为子块组件内部实现。
- 删除：`view_assembler` 的 `semantic_version`、`tool_call.rs` 父块的 result 字符串渲染路径。
- guard：新增 block 组件/嵌套 isolation guard，接入架构守卫。
