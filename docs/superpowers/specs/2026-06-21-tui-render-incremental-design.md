# TUI 渲染增量化设计（B 阶段）

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/425
> 前置：A 阶段（SpinnerTick active 门控 + assemble revision memo + resize 标脏）已根治 **idle 卡死**。本 spec 聚焦 **运行中（active/streaming）大会话持续卡顿**。

## 背景与实测定位

A 阶段消除了 idle/完成态的全量重建。但 active（streaming 生成中、tool 运行中）时，每个内容变化（chunk/tool 更新）使 `conversation.revision` 前进，A3 的 assemble memo 失效，触发全量 `assemble` + 全量 `render_tree`。

为避免凭猜测优化，新增了性能基准 `bench_refresh_cost_by_conversation_size`（`output_tests.rs`，`#[ignore]`），合成「每 turn = user + assistant + completed tool」的大会话，release 实测：

| turns | blocks | assemble | render_cold | render_warm | streaming/chunk(assemble+warm) |
|---|---|---|---|---|---|
| 500 | 2000 | 0.94ms | 3.9ms | 1.9ms | 2.8ms |
| 1000 | 4000 | 2.8ms | 9.5ms | 5.6ms | 8.3ms |
| 2000 | 8000 | 9.2ms | 23ms | 19ms | 28ms |
| 4000 | 16000 | **33ms** | 54ms | **49ms** | **82ms** |

### 瓶颈结论（数据 + 代码确认）

1. **render_tree 是主瓶颈**（≈3× assemble）。`render_warm`（`BlockCache` 命中后）几乎不比 `render_cold` 省 —— 因为 `BlockCache::get_or_render` 命中时仍 **`cached.rendered.clone()`**（block_cache.rs:34，clone 所有行），且 `apply_gutter_with_frame` 在缓存外每帧消费+重建所有行。clone+gutter 是 O(总行数)，缓存救不了。
2. **assemble 超线性 O(n²)**：`find_tool_call`（output.rs:442）对每个 tool 线性扫描 `chats→turns→tool_calls`；assemble 对每个 tool 还多次调用此类查找（`tool_result_is_embedded` / `find_tool_name_by_id` / `find_tool_result_block` / `find_tool_view`）。n 个 tool × O(n) 扫描 = O(n²)。
3. **gutter 的 frame 依赖极窄**：`animated_marker_glyph`（gutter.rs:26）证实**只有 `ToolCall` 且 `status==Running` 的首行 marker 随 frame 闪烁（●↔○）**；其余所有 block 的 gutter 完全静态。运行中 tool 通常 0–1 个。

> 现实意义：中等会话（≤2000 block，~3ms/chunk）本就流畅；**万级 block（82ms/chunk）才严重卡**。

## 设计目标

让 active 大会话单帧 refresh 成本 **∝ 变化量**而非会话总量：
- streaming chunk：只重算正在生成的那个 block，其余复用。
- 动画 tick：只重算运行中 tool（0–1 个），其余复用。
- 不改 `output_area` 消费方式与滚动逻辑（draw 已做视口裁剪，无需视口虚拟化重构）。

## 方案

### B1 — assemble 工具查找索引化（O(n²) → O(n)）

**改动**：`OutputViewAssembler::assemble_from_conversation` 开始时，一次性遍历 `conversation.chats` 构建索引：

```rust
// 仅在 assemble 内构建的临时索引，O(n) 建、O(1) 查。
struct ToolIndex<'a> {
    by_id: HashMap<(&'a ChatId, &'a ChatTurnId, &'a ToolCallId), &'a ToolCall>,
}
```

把 `find_tool_call` / `find_tool_view` / `find_tool_name_by_id` / `find_tool_result_block` / `tool_result_is_embedded` 改为接收 `&ToolIndex` 并 O(1) 查找。行为等价（找到的是同一个 `ToolCall`）。

**边界**：`ToolResult` 的 result block 查找（`find_tool_result_block`）若走不同数据路径，同样建索引。key 用借用（`&ChatId` 等）避免 clone id。

**风险**：低。纯算法等价替换，现有 assemble 测试（`output_tests.rs` / `output_unit_tests.rs`）覆盖行为；bench 验证 assemble 提速 + scaling 转线性。

**收益**：assemble 4000turns 33ms → 预计个位数 ms，且去掉超线性。

### B2 — render_tree 增量（带 gutter 的 block 级缓存 + `Rc` 共享）

**核心**：缓存「带 gutter 的最终 `RenderedBlock`」并以 `Rc` 共享，避免命中时 clone 所有行。

#### 数据结构

把行拷贝成本集中到 `Rc`，且**不破坏** `RenderedDocument.blocks: Vec<RenderedBlock>` 与 `output_area` 消费：

- `RenderedBlock.lines: Rc<Vec<RenderedLine>>`（rendered.rs，原 `Vec<RenderedLine>`）。`clone` 一个 `RenderedBlock` 退化为 `Rc::clone`（零行拷贝）—— 现有 `BlockCache::get_or_render` 命中处的 `cached.rendered.clone()`（block_cache.rs:34）**立即受益**。构造点（含 `render_tests.rs` 多处 `RenderedBlock { lines: vec![...] }`）改为 `Rc::new(vec![...])`；只读访问（`.iter()` / `.len()`）不变。
- `OutputDocumentRenderer` 新增 **gutted 缓存**（带 gutter 的最终 block，与现有「无 gutter」`BlockCache` 并列）：

```rust
struct GuttedCacheKey {
    block_version: u64,
    text_width: u16,
    depth: usize,
    marker_frame: Option<u64>, // None=静态 block；Some(blink_frame)=运行中 ToolCall
}
// block_id -> (GuttedCacheKey, RenderedBlock)   // RenderedBlock.lines 内部已是 Rc<Vec<_>>，命中 clone 廉价
```

#### render_node 流程（改）

对每个 node：
1. 计算 `marker_frame`：仅当 `kind==ToolCall(status==Running)` 时为 `Some(animation_frame / TOOL_MARKER_BLINK_DIVISOR)`，否则 `None`。
2. 组 `GuttedCacheKey`。命中（key 相等）→ `Rc::clone`（廉价，零行拷贝），直接 push。
3. 未命中 → 现有路径：`BlockCache` 取 `render_self`（无 gutter）→ `apply_gutter_with_frame` → `wrap_user_message_card_lines` 等 → 包成 `Rc<RenderedBlock>` 存 gutted 缓存 → push。
4. `retain` 清理：按 live block_id 同时清 `BlockCache` 与 gutted 缓存。

> 静态 block：`marker_frame=None`，`block_version`+`text_width` 不变即命中，**跨 frame、跨 revision 复用**（streaming 时旧 block 全部命中）。
> 运行中 ToolCall：`marker_frame` 随 blink 变，每帧重算 —— 但数量 0–1 个，成本可忽略。

#### trim 与 root 分隔空行

- `trim_root_groups_to_max_lines` 仍按 root group 累加行数裁剪，但累加用 `Rc<RenderedBlock>` 的 `lines.len()`（不 clone）。
- depth==0 的「root 前空行」（document_renderer.rs:111）属 gutter 组合产物，纳入缓存的 gutted block（保持现有视觉）。

**风险**：中。触及 `rendered.rs`（`Vec<Rc<RenderedBlock>>`）、`document_renderer.rs`（gutted 缓存 + render_node）、`output_area::render`（消费 `Rc`）。`BlockCache` 命中仍 clone 的问题由 gutted 层 `Rc` 覆盖（gutted 命中走 `Rc`，未命中才回落到 `BlockCache.clone`）。

**收益**：streaming chunk render 从 O(总量)→O(变化量)。万级 block 下，单帧仅重算 1 个生成中 block + 0–1 运行中 tool，render 从 ~49ms → 亚毫秒级。

### 不做（YAGNI）

- **视口虚拟化（原②）**：draw 已 `visible_range` 裁剪；B2 让上游增量即够，不必改 `output_area` 消费 / 滚动 / `total_lines`，避免冷启动 `total_lines` 鸡生蛋与滚动锚定的高风险重构。
- **增量 assemble 复用 BlockNode（原③）**：B1 索引化后 assemble 降为 O(n) 线性，4000turns 个位数 ms；进一步增量复用 node 收益有限、复杂度高，暂不做。
- **冷启动优化**：首次全量不可避免，不在本范围（用户确认聚焦运行中卡顿）。

## 验证

- **bench**：B1 后 assemble 转线性且提速；B2 后「streaming chunk」（仿真：大会话基础上 apply 一个新 AssistantText 再 refresh）单帧成本与会话总量解耦（接近常数）。
- **行为等价**：B1/B2 全程不改渲染输出。现有 `output_tests.rs` / `output_unit_tests.rs` / `render_tests.rs` 全绿；新增 gutted 缓存命中/失效单测（静态 block 跨 frame 命中、运行中 tool 每帧失效、block_version 变失效、text_width 变失效）。
- **门禁**：`cargo test -p cli` + `cargo clippy -p cli` 全绿。

## 与其他 issue 协调

- **#388（TUI typed 组装管线 / 拆 `view_assembler/output.rs`）**：B1 改 `output.rs` 的查找逻辑，同文件域。B1 是行为等价的内部优化，与 #388 的 typed/拆分不冲突；若 #388 先动，B1 在其结构上做；否则 B1 先行，#388 拆分时保留索引。
- **#390（timeline 单一真相）**：B2 改 render 层，不依赖 timeline 结构，与 #390 正交。

## 实施顺序

1. **B1**（低风险、独立、可单独 PR）：索引化 → bench 验证 assemble 转线性。
2. **B2**（核心）：`Rc` 化 RenderedDocument → gutted 缓存 → render_node 增量 → bench 验证 render 解耦。
