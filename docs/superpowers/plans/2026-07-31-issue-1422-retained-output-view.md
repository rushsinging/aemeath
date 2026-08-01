# #1422 统一输出窗口物化实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用轻量历史索引和唯一的“同步索引 → 规划窗口 → 只物化窗口 → 渲染窗口”管线替代完整历史 `BlockNode` 常驻与渲染前全量扫描，使冷启动、Resume、增量更新、resize 和历史滚动的物化成本都受请求窗口约束。

**Architecture:** `ConversationModel` 继续通过有界 `OutputViewJournal` 发布稳定 identity 变更；`RetainedOutputView` 收窄为 journal cursor、workspace identity 与 `OutputWindowIndex` 的 owner，不再持有完整历史 `OutputViewModel`。`OutputWindowIndex` 只保存顺序、稳定 identity、可渲染性和布局摘要；统一窗口入口先定位候选项，再通过 `OutputViewAssembler::assemble_item` 物化候选 root，使用精确行数收敛窗口，最后把只含当前窗口 roots 的 `OutputViewModel` 交给 renderer。冷启动和 Resume 只重建同一种轻量索引，不拥有特殊渲染分支。

**Tech Stack:** Rust 2021、Ratatui、现有 TEA model/reducer、`OutputViewJournal`、有界 block/gutted LRU、稳定 `RenderedLineAnchor`、Cargo 测试与架构守卫。

---

## 目标文件结构

- 新建 `apps/cli/src/tui/view_assembler/output_window_index.rs`：拥有轻量历史顺序、位置、估算/精确布局摘要与窗口范围选择；不得持有 `BlockNode`、消息正文、ToolResult payload 或 rendered lines。
- 新建 `apps/cli/src/tui/view_assembler/output_window_index_tests.rs`：覆盖 journal 增量、窗口规划、精确布局收敛和 retained-state 上限。
- 修改 `apps/cli/src/tui/view_assembler/retained_output_view.rs`：只维护 cursor、workspace identity 和 `OutputWindowIndex`，提供唯一 `materialize_window` 生产入口。
- 修改 `apps/cli/src/tui/view_assembler/retained_output_view_tests.rs`：从“完整历史 root 复用”迁移为“窗口外不物化、窗口内按 identity/version 复用”。
- 修改 `apps/cli/src/tui/view_assembler/output.rs`：保留单 item 与 placeholder 装配，退役完整历史装配实现。
- 修改 `apps/cli/src/tui/view_assembler/output_tool_lookup.rs`、`apps/cli/src/tui/model/conversation/model.rs`：提供按 `(chat_id, turn_id, tool_call_id)` 定位的唯一工具查找路径，避免窗口候选逐项扫描完整 chats。
- 修改 `apps/cli/src/tui/view_model/output.rs`：将 `OutputViewModel` 收窄为单次窗口物化结果，携带源总行数与折叠行数。
- 修改 `apps/cli/src/tui/render/output/document_renderer.rs`：只渲染已选择的窗口 roots，不再从完整 roots 计算窗口或清理全历史 cache identity。
- 修改 `apps/cli/src/tui/render/output/document_renderer/tests.rs`：验证窗口模型渲染、block cache 复用和稳定 anchor，不再测试 renderer 内部全量窗口选择。
- 修改 `apps/cli/src/tui/app/update.rs`：调用唯一窗口入口并消费窗口结果；冷启动、Resume、增量、resize、滚动均走相同顺序。
- 修改 `apps/cli/src/tui/app/scenario_tests/history_window.rs`、`resume_sdk_delivery.rs`、`frame_performance.rs`：覆盖用户场景与确定性工作量门禁。
- 修改 `apps/cli/src/tui/view_assembler/output_tests/retained_state_performance_tests.rs`、`edit_diff_performance_tests.rs`：基准改为索引同步、窗口物化和窗口渲染。
- 修改 `.agents/hooks/check-tui-retained-output-view.sh`、`.agents/hooks/check-tui-retained-output-view-tests.sh`：保护统一窗口入口并拒绝退役路径回归。
- 修改 `docs/design/03-engineering/04-testing-and-coverage.md`：登记索引、物化、renderer、App 与 Resume 的逐层证据。

## 完成定义与强制退役清单

以下项目是同一交付的一部分，**不得**以兼容、后续优化或测试便利为由延期：

1. `RetainedOutputView` 不再持有完整历史 `OutputViewModel` 或 `Vec<Arc<BlockNode>>`。
2. 删除生产 `OutputViewAssembler::assemble_shared_roots`。
3. 删除 `retained_item_ids`、`root_item_ids` 和 `positions` 这套重复顺序/位置状态。
4. 删除 renderer 每帧构造完整 `semantic_root_ids`、`root_layout_states` 和全历史 `collect_semantic_block_ids` 的路径。
5. renderer 不再调用 `select_root_window`、`select_root_range` 从完整 roots 选窗口；窗口选择只有 `OutputWindowIndex` 一个实现。
6. `App::refresh_output_document_from_model` 不再“先同步完整 ViewModel，再创建 `OutputRenderWindow`”。
7. 全部测试迁移后删除无生产语义的 `render_model_document`、`render_tree`、`render_tree_with_animation_frame` 兼容入口；如果某个入口仍被保留，必须证明它调用同一窗口生产入口且有非重复职责。
8. `assemble_from_conversation` 不得保留一套与生产单 item 装配重复的完整 match；语义测试改用生产窗口入口。迁移完成后删除该符号。
9. 仅服务旧完整参考装配的 `ToolIndex` 及其测试必须删除；工具查找只保留生产定位接口。
10. 更新守卫，旧守卫不得继续要求 `.sync(&self.model.conversation, ...)` 这一退役 API。
11. `rg` 搜索所有退役符号生产引用为零，`cargo build -p cli` 不产生只被测试引用的死代码警告。

---

### Task 1：先固化旧路径退役守卫

**Files:**
- Modify: `.agents/hooks/check-tui-retained-output-view.sh`
- Modify: `.agents/hooks/check-tui-retained-output-view-tests.sh`

- [ ] **Step 1：添加当前实现必然失败的守卫 fixture**

守卫 fixture 分别加入以下违规片段，并逐个断言 guard 返回非零：

```rust
struct RetainedOutputView {
    view_model: OutputViewModel,
}

fn rebuild() {
    OutputViewAssembler::assemble_shared_roots(conversation, None);
}

fn render_all(view_model: &OutputViewModel) {
    let semantic_root_ids = view_model.roots.iter().collect::<Vec<_>>();
}
```

同时把合法 fixture 改为唯一窗口入口：

```rust
let result = self
    .output_view
    .retained
    .materialize_window(&self.model.conversation, workspace_root, request);
```

- [ ] **Step 2：运行守卫测试并确认 RED**

Run: `.agents/hooks/check-tui-retained-output-view-tests.sh`

Expected: FAIL，因为现有 guard 尚不能拒绝完整历史 owner、完整 roots 扫描和退役同步 API。

- [ ] **Step 3：实现最小守卫规则**

守卫必须检查：

- `retained_output_view.rs` 不出现 `view_model: OutputViewModel`、`assemble_shared_roots`、`retained_item_ids`；
- `app/update.rs` 必须调用 `materialize_window`；
- `document_renderer.rs` 不出现生产用 `semantic_root_ids`、`root_layout_states` 或从完整 roots 选择窗口的调用；
- journal 仍显式有界；
- 已退役的 `OutputViewCache` 与宽泛 `projection` 命名不得回归。

- [ ] **Step 4：运行 guard fixture 至 GREEN，并确认真实源码仍 RED**

Run:

```bash
.agents/hooks/check-tui-retained-output-view-tests.sh
.agents/hooks/check-tui-retained-output-view.sh
```

Expected: fixture tests PASS；真实源码 guard FAIL，准确指出尚未退役的生产路径。

- [ ] **Step 5：提交守卫 RED 证据**

```bash
git add .agents/hooks/check-tui-retained-output-view.sh .agents/hooks/check-tui-retained-output-view-tests.sh
git commit -m "test(tui): #1422 固化输出窗口退役门禁"
```

### Task 2：建立不持有正文的 `OutputWindowIndex`

**Files:**
- Create: `apps/cli/src/tui/view_assembler/output_window_index.rs`
- Create: `apps/cli/src/tui/view_assembler/output_window_index_tests.rs`
- Modify: `apps/cli/src/tui/view_assembler.rs`
- Read contract: `apps/cli/src/tui/model/output_timeline/item.rs`
- Read contract: `apps/cli/src/tui/model/conversation/output_view_change.rs`

- [ ] **Step 1：写轻量索引 RED 测试**

测试建立 5,000 个不同长度的 timeline items，并断言索引公开测试快照只包含以下字段：

```rust
struct OutputWindowEntrySnapshot {
    item_id: String,
    estimated_lines: usize,
    exact_lines: Option<usize>,
}
```

同时断言 `Append` 增加一个条目、`Update` 清除目标精确行数、`Remove` 删除目标、`Placeholder` 更新合成条目、`Reset` 产生与 timeline 顺序一致的新索引。

- [ ] **Step 2：运行测试并确认 RED**

Run: `cargo test -p cli output_window_index --no-fail-fast`

Expected: FAIL，`OutputWindowIndex` 尚不存在。

- [ ] **Step 3：实现轻量条目与位置索引**

生产类型固定为单一职责：

```rust
pub(crate) struct OutputWindowEntry {
    item_id: String,
    estimated_lines: usize,
    exact_layout: Option<ExactRootLayout>,
}

pub(crate) struct ExactRootLayout {
    width: u16,
    spacing_fingerprint: u64,
    block_version: u64,
    line_count: usize,
}

pub(crate) struct OutputWindowIndex {
    entries: Vec<OutputWindowEntry>,
    positions: HashMap<String, usize>,
}
```

`OutputWindowEntry` 不得保存 timeline item、`BlockNode`、正文、ToolResult payload 或 rendered lines。估算只使用 item 类型与已有轻量统计，不 clone 文本。

- [ ] **Step 4：实现 journal 增量**

提供：

```rust
fn rebuild_from_timeline(&mut self, conversation: &ConversationModel);
fn apply_change(&mut self, change: &OutputViewChange, conversation: &ConversationModel);
fn record_exact_layout(&mut self, item_id: &str, layout: ExactRootLayout);
```

`Append`/`Update`/`Remove` 的 touched entry 数必须与变化数一致；`Reset` 和 cursor 过期允许扫描 timeline，但只建立轻量条目。

- [ ] **Step 5：运行索引测试至 GREEN**

Run: `cargo test -p cli output_window_index --no-fail-fast`

Expected: PASS；5,000 项索引中 retained `BlockNode` 数为零。

- [ ] **Step 6：提交轻量索引**

```bash
git add apps/cli/src/tui/view_assembler.rs apps/cli/src/tui/view_assembler/output_window_index.rs apps/cli/src/tui/view_assembler/output_window_index_tests.rs
git commit -m "perf(tui): #1422 建立轻量输出窗口索引"
```

### Task 3：在索引内实现唯一窗口规划

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/output_window_index.rs`
- Modify: `apps/cli/src/tui/view_assembler/output_window_index_tests.rs`
- Read contract: `apps/cli/src/tui/render/output/document_renderer.rs`

- [ ] **Step 1：写窗口选择 RED 测试**

覆盖：

- `line_limit == 0` 返回空窗口；
- 从尾部按完整 root group 覆盖 `line_limit`；
- `tail_offset` 跳过新端完整 roots；
- 单个 root 超过 `line_limit` 时仍完整选择；
- offset 超过总行数时稳定返回最旧可定位范围；
- 估算窗口内 root 被精确化后，范围可向前或向后收敛；
- 5,000 项的窗口选择不构造 `BlockNode`。

期望返回：

```rust
pub(crate) struct OutputWindowSelection {
    pub item_range: Range<usize>,
    pub source_total_lines: usize,
    pub folded_earlier_lines: usize,
}
```

- [ ] **Step 2：运行测试并确认 RED**

Run: `cargo test -p cli output_window_index --no-fail-fast`

Expected: FAIL，索引尚无 `select_window`。

- [ ] **Step 3：实现基于摘要的选择与收敛**

提供：

```rust
fn select_window(&self, request: OutputRenderWindow) -> OutputWindowSelection;
```

初次选择使用精确行数或估算行数；窗口候选精确渲染后调用 `record_exact_layout`，重复选择直至范围与源总行数稳定。禁止 renderer 保留第二套选择算法。

- [ ] **Step 4：运行索引测试至 GREEN**

Run: `cargo test -p cli output_window_index --no-fail-fast`

Expected: PASS，边界 root 不拆分，精确布局收敛稳定。

- [ ] **Step 5：提交窗口规划器**

```bash
git add apps/cli/src/tui/view_assembler/output_window_index.rs apps/cli/src/tui/view_assembler/output_window_index_tests.rs
git commit -m "perf(tui): #1422 统一输出窗口规划"
```

### Task 4：建立唯一且有界的工具调用定位

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/model.rs`
- Modify: corresponding `apps/cli/src/tui/model/conversation/*_tests.rs`
- Modify: `apps/cli/src/tui/view_assembler/output_tool_lookup.rs`
- Modify: `apps/cli/src/tui/view_assembler/output_tool_view.rs`

- [ ] **Step 1：写工具定位 RED 契约测试**

建立多个 chat、turn 和同名 tool ID，断言 `(chat_id, turn_id, tool_call_id)` 精确定位唯一 call；缺失引用返回 `None`；重复窗口物化不扫描无关 chat/turn。测试计数器必须位于 `cfg(test)`，不得扩大生产 API。

- [ ] **Step 2：运行定向测试并确认 RED**

Run: `cargo test -p cli tool_lookup --no-fail-fast`

Expected: FAIL，当前 `ConversationToolLookup::call` 逐层线性扫描。

- [ ] **Step 3：实现单一查找端口**

由 `ConversationModel` 暴露只读定位方法，或维护与 mutation 同步的轻量位置索引；`ConversationToolLookup` 只委托该入口。不得复制 `ToolCall` 或 payload，不得建立每次窗口调用的完整 `ToolIndex`。

- [ ] **Step 4：运行模型与 assembler 相邻测试至 GREEN**

Run:

```bash
cargo test -p cli tool_lookup --no-fail-fast
cargo test -p cli output_tool_view --no-fail-fast
```

Expected: PASS；同 ID 跨 turn 不冲突，查找不扫描无关历史。

- [ ] **Step 5：提交工具定位**

```bash
git add apps/cli/src/tui/model/conversation apps/cli/src/tui/view_assembler/output_tool_lookup.rs apps/cli/src/tui/view_assembler/output_tool_view.rs
git commit -m "perf(tui): #1422 收口窗口工具定位"
```

### Task 5：把 `RetainedOutputView` 收窄为索引 owner 和窗口物化器

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/retained_output_view.rs`
- Modify: `apps/cli/src/tui/view_assembler/retained_output_view_tests.rs`
- Modify: `apps/cli/src/tui/view_assembler/output.rs`
- Modify: `apps/cli/src/tui/view_model/output.rs`

- [ ] **Step 1：写统一窗口物化 RED 测试**

覆盖冷启动、Resume/reset、append、stream update、remove、placeholder、workspace 变化：

- 冷启动 5,000 items 只创建请求窗口内 `BlockNode`；
- Resume/reset 只重建索引，再物化同一窗口；
- append/update 只使目标摘要失效；
- 窗口外 item 的 assembler 访问计数为零；
- 同一窗口未变化 root 使用缓存 backing；
- 窗口移动后旧 root 可由有界 cache 淘汰，retained roots 不随总历史无限增长。

- [ ] **Step 2：运行测试并确认 RED**

Run: `cargo test -p cli retained_output_view --no-fail-fast`

Expected: FAIL，当前 `RetainedOutputView::rebuild` 会完整调用 `assemble_shared_roots`。

- [ ] **Step 3：定义窗口请求与结果**

唯一生产入口：

```rust
pub(crate) fn materialize_window(
    &mut self,
    conversation: &ConversationModel,
    workspace_root: Option<&Path>,
    width: u16,
    spacing: MarkdownSpacingPolicy,
    request: OutputRenderWindow,
) -> MaterializedOutputWindow;
```

结果包含只属于当前窗口的模型：

```rust
pub(crate) struct MaterializedOutputWindow {
    pub view_model: OutputViewModel,
    pub stats: RetainedOutputViewStats,
}
```

`OutputViewModel` 增加 `source_total_lines` 与 `folded_earlier_lines`，其 `roots` 明确定义为当前窗口 roots。

- [ ] **Step 4：实现统一同步、规划和物化循环**

`materialize_window` 严格按以下顺序：

1. 消费 journal 或重建轻量索引；
2. `select_window` 得到候选 range；
3. 仅对 range 内 items 调用 `assemble_item` / `assemble_placeholder`；
4. 获取候选精确行数并回写索引；
5. 若范围变化则重复 2–4，直到稳定；
6. 返回窗口 `OutputViewModel`。

设置有限收敛上限；若摘要持续变化，记录 warning 并使用最后一个完整 root 窗口，不回退到全历史物化。

- [ ] **Step 5：运行 retained-view 测试至 GREEN**

Run: `cargo test -p cli retained_output_view --no-fail-fast`

Expected: PASS；冷启动、Resume 和 warm update 使用同一入口，5,000 items 不产生 5,000 roots。

- [ ] **Step 6：提交窗口物化器**

```bash
git add apps/cli/src/tui/view_assembler/retained_output_view.rs apps/cli/src/tui/view_assembler/retained_output_view_tests.rs apps/cli/src/tui/view_assembler/output.rs apps/cli/src/tui/view_model/output.rs
git commit -m "perf(tui): #1422 只物化当前输出窗口"
```

### Task 6：让 renderer 只负责窗口渲染

**Files:**
- Modify: `apps/cli/src/tui/render/output/document_renderer.rs`
- Modify: `apps/cli/src/tui/render/output/document_renderer/tests.rs`

- [ ] **Step 1：写窗口 renderer RED 测试**

输入仅含当前窗口 roots 的 `OutputViewModel`，断言：

- renderer 按输入顺序输出全部窗口 root groups；
- `folded_earlier_lines > 0` 时只添加一次折叠提示；
- `source_total_lines` 原样传播；
- 同一窗口 block cache 命中；
- resize 只重渲染输入窗口；
- renderer 不需要完整历史 identity 才能清理 cache。

- [ ] **Step 2：运行 renderer 测试并确认 RED**

Run: `cargo test -p cli document_renderer --no-fail-fast`

Expected: FAIL，当前 renderer 仍构造完整 root layout state 并自行选择窗口。

- [ ] **Step 3：删除 renderer 内窗口规划职责**

`render_model_window` 改为直接渲染 `view_model.roots`，结果的总行数元数据来自窗口模型。删除生产 `SelectedRootWindow`、`RootLayoutEntry`、`RootLayoutState`、`select_root_window`、`select_root_range` 及对应完整 roots 遍历。

缓存清理由有界 LRU 和显式变化 identity 负责，不得为了清理 cache 收集完整历史 block IDs。

- [ ] **Step 4：运行 renderer 测试至 GREEN**

Run: `cargo test -p cli document_renderer --no-fail-fast`

Expected: PASS；renderer 工作量只与窗口 roots 数量相关。

- [ ] **Step 5：提交 renderer 收窄**

```bash
git add apps/cli/src/tui/render/output/document_renderer.rs apps/cli/src/tui/render/output/document_renderer/tests.rs
git commit -m "refactor(tui): #1422 收窄输出渲染器到当前窗口"
```

### Task 7：接入 App 的唯一窗口管线

**Files:**
- Modify: `apps/cli/src/tui/app.rs`
- Modify: `apps/cli/src/tui/app/update.rs`
- Modify: `apps/cli/src/tui/render/performance.rs`
- Modify: `apps/cli/src/tui/app/scenario_tests/frame_performance.rs`

- [ ] **Step 1：写 App 管线 RED 场景测试**

用 5,000 roots 覆盖：

- 首次 refresh 只物化尾部窗口；
- append/update 只触及目标索引和当前窗口；
- spinner-only 不同步索引、不物化静态 roots；
- resize 走同一个 `materialize_window`，不建立冷启动或 resize 特殊 assembler；
- 向上滚动只物化新窗口；
- `source_total_lines` 和稳定 anchor 正确更新。

- [ ] **Step 2：运行场景测试并确认 RED**

Run: `cargo test -p cli frame_performance --no-fail-fast`

Expected: FAIL，当前 App 先 `sync` 完整 ViewModel，再把窗口交给 renderer。

- [ ] **Step 3：改造 `refresh_output_document_from_model`**

在读取宽度、spacing 和 `OutputRenderWindow` 后立即调用 `materialize_window`；renderer 只接收返回的窗口模型。日志字段明确区分：

- indexed items；
- selected items；
- materialized roots；
- reused roots；
- source total lines；
- rendered window lines。

不得出现冷启动专用渲染分支。

- [ ] **Step 4：运行 App 场景测试至 GREEN**

Run: `cargo test -p cli frame_performance --no-fail-fast`

Expected: PASS；5,000 roots 的首次和 warm refresh 都受窗口约束。

- [ ] **Step 5：提交 App 接入**

```bash
git add apps/cli/src/tui/app.rs apps/cli/src/tui/app/update.rs apps/cli/src/tui/render/performance.rs apps/cli/src/tui/app/scenario_tests/frame_performance.rs
git commit -m "perf(tui): #1422 接入统一输出窗口管线"
```

### Task 8：验证实时、Resume 与交互语义

**Files:**
- Modify: `apps/cli/src/tui/app/scenario_tests/history_window.rs`
- Modify: `apps/cli/src/tui/app/scenario_tests/resume_sdk_delivery.rs`
- Modify: relevant selection/copy/link scenario files under `apps/cli/src/tui/app/scenario_tests/`
- Modify: `apps/cli/src/tui/view_assembler/retained_output_view_tests.rs`

- [ ] **Step 1：写实时与 Resume 等价 RED 测试**

同一会话事实分别通过实时 intent 和 Resume SDK delivery 建模，比较窗口结果中的 block ID、kind、ToolCall/ToolResult parent-child、AskUser、placeholder 和 terminal notice。每个终态同时断言冲突终态不存在。

- [ ] **Step 2：写历史交互 RED 测试**

覆盖键盘和鼠标向上加载、向下返回最新、resize、selection、copy、link。断言窗口切换后稳定 line anchor 能重新定位；窗口外内容不会常驻为 `BlockNode`。

- [ ] **Step 3：分别运行新增测试并确认 RED**

Run:

```bash
cargo test -p cli resume_sdk_delivery --no-fail-fast
cargo test -p cli history_window --no-fail-fast
cargo test -p cli selection --no-fail-fast
cargo test -p cli link --no-fail-fast
```

Expected: 至少新增的窗口物化/等价断言失败。

- [ ] **Step 4：修正索引失效和 anchor 处理**

只修改统一窗口管线：Reset/Resume 重建轻量索引，workspace/width/spacing 使相关精确布局失效，remove/reorder 更新索引顺序，placeholder 独立更新。禁止添加场景专用完整物化分支。

- [ ] **Step 5：运行全部交互场景至 GREEN**

重复 Step 3 命令。

Expected: 全部 PASS；实时与 Resume 业务语义一致，选择/复制/链接行为不回归。

- [ ] **Step 6：提交语义修正**

```bash
git add apps/cli/src/tui/app/scenario_tests apps/cli/src/tui/view_assembler/retained_output_view_tests.rs
git commit -m "test(tui): #1422 覆盖输出窗口实时与恢复语义"
```

### Task 9：退役所有重复和旁路代码

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/retained_output_view.rs`
- Modify: `apps/cli/src/tui/view_assembler/output.rs`
- Modify: `apps/cli/src/tui/view_assembler/output_unit_tests.rs`
- Modify: `apps/cli/src/tui/view_assembler/output_task_tests.rs`
- Modify: `apps/cli/src/tui/view_assembler/output_tests/*.rs`
- Modify: `apps/cli/src/tui/render/output/document_renderer.rs`
- Modify: `apps/cli/src/tui/render/output/document_renderer/tests.rs`
- Modify: `apps/cli/src/tui/adapter/output_widget.rs`
- Modify: `.agents/hooks/check-tui-retained-output-view.sh`
- Modify: `.agents/hooks/check-tui-retained-output-view-tests.sh`

- [ ] **Step 1：把 assembler 测试迁移到生产窗口入口**

将所有 `assemble_from_conversation` 调用改为构造轻量索引并请求足以覆盖 fixture 的窗口，或者直接测试 `assemble_item`。测试必须继续覆盖 ToolResult、Task、AskUser、Hook 和 Agent 展示语义。

- [ ] **Step 2：把 renderer 测试迁移到唯一窗口入口**

将 `render_model_document`、`render_tree`、`render_tree_with_animation_frame` 的调用迁移到 `render_model_window`；小 fixture 使用显式有限窗口，不以 `usize::MAX` 模拟第二套生产语义。

- [ ] **Step 3：删除完整历史装配与重复状态**

删除：

- `OutputViewAssembler::assemble_shared_roots`；
- `OutputViewAssembler::assemble_from_conversation`；
- `ToolIndex` 及仅验证它的测试；
- `retained_item_ids`；
- `RetainedOutputView.view_model`、`root_item_ids`、`positions`；
- renderer 的完整 roots layout/window 选择类型和函数；
- 无调用的 renderer 兼容入口；
- 旧 `.sync(...)` API 和要求它存在的 guard 文本。

- [ ] **Step 4：运行零引用搜索**

Run:

```bash
rg -n 'assemble_shared_roots|assemble_from_conversation|retained_item_ids|root_item_ids|render_model_document|render_tree_with_animation_frame|semantic_root_ids|root_layout_states|OutputViewCache' apps/cli/src/tui .agents/hooks
```

Expected: 无生产或测试引用；只允许 guard 的“禁止模式”fixture/字符串命中，并逐条人工确认。

- [ ] **Step 5：运行非测试构建和守卫**

Run:

```bash
cargo build -p cli
.agents/hooks/check-tui-retained-output-view-tests.sh
.agents/hooks/check-tui-retained-output-view.sh
```

Expected: 全部 PASS，无 dead code warning，真实 guard 由 RED 转为 GREEN。

- [ ] **Step 6：提交退役清理**

```bash
git add apps/cli/src/tui .agents/hooks/check-tui-retained-output-view.sh .agents/hooks/check-tui-retained-output-view-tests.sh
git commit -m "refactor(tui): #1422 退役完整历史输出路径"
```

### Task 10：建立确定性性能与内存门禁

**Files:**
- Modify: `apps/cli/src/tui/app/scenario_tests/frame_performance.rs`
- Modify: `apps/cli/src/tui/view_assembler/output_tests/retained_state_performance_tests.rs`
- Modify: `apps/cli/src/tui/view_assembler/output_tests/edit_diff_performance_tests.rs`
- Modify: `apps/cli/src/tui/render/performance.rs`

- [ ] **Step 1：写 100/1000/5000 block 计数门禁**

每个规模断言：

- 冷启动 materialized roots 受 `line_limit` 约束；
- append/update touched index entries `<= 1`；
- 窗口外 assembler visits 为零；
- spinner-only index sync、materialization、static document render 均为零；
- retained `BlockNode` 与 rendered cache 数不随历史总 roots 无界增长。

- [ ] **Step 2：写 10/50/100 大型 Edit 门禁**

断言只解析和渲染当前窗口内 diff；窗口外 Edit 不进入 highlighter 或 diff parser；resize 只重新处理窗口候选。

- [ ] **Step 3：运行 debug 计数门禁**

Run:

```bash
cargo test -p cli frame_performance --no-fail-fast
cargo test -p cli retained_state_performance --no-fail-fast
cargo test -p cli edit_diff_performance --no-fail-fast
```

Expected: 全部 PASS；工作量上限由确定性计数而非墙钟 sleep 证明。

- [ ] **Step 4：运行 Release workload**

Run:

```bash
cargo test -p cli --release tui_retained_output_view_release_workload -- --ignored --nocapture
cargo test -p cli --release edit_diff_release_workload -- --ignored --nocapture
```

Expected: 记录 100/1000/5000 的 P50/P95、selected/materialized roots 与 retained cache；5,000 历史的窗口物化数量不得超过相同窗口下 100 历史的常数倍。

- [ ] **Step 5：提交性能门禁**

```bash
git add apps/cli/src/tui/app/scenario_tests/frame_performance.rs apps/cli/src/tui/view_assembler/output_tests/retained_state_performance_tests.rs apps/cli/src/tui/view_assembler/output_tests/edit_diff_performance_tests.rs apps/cli/src/tui/render/performance.rs
git commit -m "test(tui): #1422 固化输出窗口性能门禁"
```

### Task 11：同步测试设计与完整架构门禁

**Files:**
- Modify: `docs/design/03-engineering/04-testing-and-coverage.md`
- Modify: `docs/design/03-engineering/01-architecture-guards.md`
- Modify: `.agents/hooks/check-architecture-guards.sh` only if registration changes

- [ ] **Step 1：登记逐层证据**

文档登记：

- L1：`OutputWindowIndex` journal 增量与窗口范围；
- L2：窗口物化、工具定位、renderer 相邻契约；
- L4：冷启动、Resume、滚动、resize、selection/copy/link；
- 性能：确定性访问/物化/cache 计数和 ignored Release workload；
- 退役：guard fixture、零引用搜索、production reachability。

- [ ] **Step 2：更新架构守卫说明**

守卫说明明确禁止完整历史 `BlockNode` owner、渲染前完整 roots 扫描和第二套窗口选择算法；不要引用 Issue/PR 编号。

- [ ] **Step 3：运行文档与架构门禁**

Run:

```bash
.agents/hooks/check-architecture-guards.sh --full
cargo run -p xtask -- production-reachability .
git diff --check
```

Expected: 全部 PASS。

- [ ] **Step 4：提交设计同步**

```bash
git add docs/design/03-engineering/04-testing-and-coverage.md docs/design/03-engineering/01-architecture-guards.md .agents/hooks/check-architecture-guards.sh
git commit -m "docs(testing): 同步统一输出窗口门禁"
```

### Task 12：完整验证、现场验收与 PR 更新

**Files:**
- Verify only: whole workspace
- External update: GitHub Issue #1422 and PR #1465 after all checks pass

- [ ] **Step 1：格式、CLI 构建与定向测试**

Run:

```bash
cargo fmt --all -- --check
cargo build -p cli
cargo test -p cli output_window_index --no-fail-fast
cargo test -p cli retained_output_view --no-fail-fast
cargo test -p cli document_renderer --no-fail-fast
cargo test -p cli frame_performance --no-fail-fast
cargo test -p cli history_window --no-fail-fast
cargo test -p cli resume_sdk_delivery --no-fail-fast
git diff --check
```

Expected: 全部 PASS。

- [ ] **Step 2：CLI 与 workspace 门禁**

Run:

```bash
cargo test -p cli --no-fail-fast
cargo build --workspace
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: 全部 PASS，无 warnings。

- [ ] **Step 3：守卫和退役复核**

Run:

```bash
.agents/hooks/check-tui-retained-output-view-tests.sh
.agents/hooks/check-tui-retained-output-view.sh
.agents/hooks/check-architecture-guards.sh --full
cargo run -p xtask -- production-reachability .
rg -n 'assemble_shared_roots|assemble_from_conversation|retained_item_ids|root_item_ids|render_model_document|render_tree_with_animation_frame|semantic_root_ids|root_layout_states|OutputViewCache' apps/cli/src/tui .agents/hooks
```

Expected: 所有 guard PASS；搜索只命中 guard 的禁止模式 fixture/字符串，逐条列入验收记录；不存在生产或测试旧路径引用。

- [ ] **Step 4：绑定明确运行身份进行现场复测**

记录并核验：

- PID；
- binary 绝对路径；
- binary 对应 Git SHA；
- worktree/branch；
- Session ID；
- `AEMEATH_AGENTS_DIR`；
- 日志文件和采样时间窗口。

使用固定大 Session 复测冷启动、尾部增量、向上滚动和 resize，记录 RSS/physical footprint、prepare/draw P50/P95、indexed items、selected/materialized roots 和 retained cache。

- [ ] **Step 5：核对 Issue 全部验收清单**

逐项核对 #1422 body。任何未完成项必须在 Issue/PR 中记录可验证原因、影响和后续处理；存在无理由未完成项时不得宣称完成或更新 PR 为可合并。

- [ ] **Step 6：更新 Issue 与 PR 证据**

把完整命令结果、Release workload、现场复测和退役零引用证据写入 #1422 与 PR #1465。保留 `Closes #1422`，但不自行关闭 Issue、不自行合并 PR。
