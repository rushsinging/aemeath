# #1422 TUI 增量共享保留输出视图实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消除长会话每次 conversation revision 都完整遍历并深拷贝 `OutputViewModel` 的行为，使输出更新成本主要取决于新增/变化 root 与当前历史窗口，而不是完整历史字节数。

**Architecture:** `ConversationModel` 发布有界的输出视图变更日志，记录 append、update、remove/reset 与 synthetic placeholder 变化；`RetainedOutputView` 作为 TUI 侧唯一增量装配结果 owner，按 cursor 消费日志并以稳定 root identity/version 复用 `Arc<BlockNode>`。`OutputDocumentRenderer` 消费共享 roots 和增量语义变更，保留轻量累计布局索引，并在物化富渲染前选择窗口；过期 cursor、Resume、reset、reorder 或 workspace 变化使用明确的全量重建降级，不改变业务内容。

**Tech Stack:** Rust 2021、Ratatui、现有 TEA reducer/model、`Arc`、有界 `VecDeque` 变更日志、现有 block/gutted LRU 与场景性能采集器。

---

## 文件结构

- 修改 `apps/cli/src/tui/model/conversation/model.rs`：在聚合根内维护单调 output view sequence 与有界变更日志。
- 新建 `apps/cli/src/tui/model/conversation/output_view_change.rs`：定义无 ViewModel 语义的 append/update/remove/reset 变更协议与 cursor 查询结果。
- 新建 `apps/cli/src/tui/model/conversation/output_view_change_tests.rs`：验证模型 mutation 到增量变更协议的相邻层契约。
- 修改 `apps/cli/src/tui/model/conversation/change.rs`：补足视图更新定位需要的稳定 block/tool identity，避免 `OutputDirty` 触发猜测。
- 修改 `apps/cli/src/tui/model/output_timeline/model.rs`：提供按稳定 identity 的只读定位，收口直接 mutation 后的结构变化通知。
- 修改 `apps/cli/src/tui/view_model/output.rs`：roots/children 使用共享不可变节点，并提供稳定 identity/version。
- 新建 `apps/cli/src/tui/view_assembler/retained_output_view.rs`：实现增量视图 owner、cursor 同步、共享 root cache 与 rebuild 降级。
- 新建 `apps/cli/src/tui/view_assembler/retained_output_view_tests.rs`：验证 append/update/reset/Resume/workspace 与共享 backing。
- 修改 `apps/cli/src/tui/view_assembler/output.rs`：拆为单 root 装配函数，删除生产路径全量 owned clone。
- 修改 `apps/cli/src/tui/view_assembler/output_tool_view.rs`：只为被请求的 tool root 构造 view，禁止构建完整 ToolIndex。
- 修改 `apps/cli/src/tui/app.rs`、`apps/cli/src/tui/app/update.rs`：由 `RetainedOutputView` 替代 `OutputViewCache`，向 renderer 传递共享语义 roots。
- 修改 `apps/cli/src/tui/render/output/document_renderer.rs`：接受共享 roots，增量维护 root layout 顺序/累计行索引，窗口外不物化 block。
- 修改 `apps/cli/src/tui/render/performance.rs`：记录 retained view touched/created/reused roots 与窗口候选数量。
- 修改 `apps/cli/src/tui/app/scenario_tests/frame_performance.rs`：增加 revision 变化、100/1000/5000 roots 与 viewport 候选门禁。
- 修改 `apps/cli/src/tui/view_assembler/output_tests/retained_state_performance_tests.rs`：替换全量 assemble 基线为增量共享 retained-state 验收。
- 修改 `apps/cli/src/tui/effect/session/resume.rs` 及对应场景测试：验证 Resume 触发一次 rebuild，后续增量复用且展示语义等价。
- 修改 `.agents/aemeath.json` 与新增 `.agents/hooks/check-tui-retained-output-view.sh`：禁止生产路径重新引入全量 owned retained output view。
- 修改 `docs/design/03-engineering/04-testing-and-coverage.md`：登记 #1422 的逐层性能与语义证据。

### Task 1：建立模型层输出视图失效协议

- [ ] **Step 1：写失败测试**

在 `output_view_change_tests.rs` 覆盖：连续 append 只发布新增 block identity；流式文本只发布当前 block update；ToolCall result/meta/activity 只发布对应 tool root update；AskUser 原位编辑发布对应 identity；dismiss/reset/Resume 发布 structural reset；日志超出容量返回 rebuild-required。

- [ ] **Step 2：运行测试并确认 RED**

运行：`cargo test -p cli output_view_change --no-fail-fast`

预期：因 `RetainedOutputViewCursor`、`OutputViewChange` 与 `changes_since` 尚不存在而编译失败。

- [ ] **Step 3：实现最小变更协议**

协议必须只携带稳定 identity 与序列号，不携带正文：

```rust
pub enum OutputViewChange {
    Append { item_id: String },
    Update { item_id: String },
    Remove { item_id: String },
    Reset,
    Placeholder,
}

pub enum OutputViewChanges<'a> {
    Delta { next_cursor: u64, changes: &'a [OutputViewChange] },
    RebuildRequired { next_cursor: u64 },
}
```

`ConversationModel::apply` 在 mutation 完成后根据明确 `ConversationChange` 与 timeline 长度/顺序变化发布日志；日志使用固定容量，过期 cursor 明确降级 rebuild。

- [ ] **Step 4：运行模型层测试并确认 GREEN**

运行：`cargo test -p cli output_view_change --no-fail-fast`

预期：全部通过，且现有 conversation model 测试不回归。

### Task 2：建立共享增量 RetainedOutputView owner

- [ ] **Step 1：写失败测试**

在 `retained_output_view_tests.rs` 建立 5000 个历史 block，首次同步后记录前 4999 个 `Arc<BlockNode>` 指针；append 一个 block 后再次同步，断言旧指针全部相同、仅创建一个 root；更新尾部流式 block 时只替换该 root；reset/Resume 后顺序和业务内容与完整 assembler 参考结果相同。

- [ ] **Step 2：运行测试并确认 RED**

运行：`cargo test -p cli retained_output_view --no-fail-fast`

预期：因 `RetainedOutputView` 不存在而编译失败。

- [ ] **Step 3：实现单 root 装配和增量 cache**

`OutputViewAssembler` 提供按 timeline identity 装配单个 root 的私有能力；`RetainedOutputView` 持有：

```rust
pub struct RetainedOutputView {
    cursor: u64,
    workspace_root: Option<String>,
    ordered_roots: Vec<Arc<BlockNode>>,
    root_positions: HashMap<String, usize>,
}
```

append 仅 push；update 仅替换目标 index；remove 调整后缀 index；reset、cursor 过期和 workspace 改变只执行一次完整 rebuild。工具视图按具体 `TimelineToolCallRef` 查找对应 call，禁止每次构造完整 `ToolIndex`。

- [ ] **Step 4：运行保留视图测试并确认 GREEN**

运行：`cargo test -p cli retained_output_view --no-fail-fast`

预期：共享 backing、增量创建计数与完整语义对照全部通过。

### Task 3：接入 App 并退役完整 owned cache

- [ ] **Step 1：写失败场景测试**

扩展 `frame_performance.rs`：冷帧建立 5000 roots；append 一个消息并 draw；断言 retained view touched/created roots 为 1、reused roots 为 5000、assemble 全历史调用为 0；spinner-only 和 resize 的 retained view work 为 0。

- [ ] **Step 2：运行测试并确认 RED**

运行：`cargo test -p cli frame_pipeline --no-fail-fast`

预期：现有实现报告完整 `assemble_source_items=5001`，断言失败。

- [ ] **Step 3：以 RetainedOutputView 替换 OutputViewCache**

`App` 只保留一个增量视图 owner；`refresh_output_document_from_model` 先同步 delta，再把共享 roots 交给 renderer。删除 `take()` 整棵 owned ViewModel 的借用规避路径，frame diagnostics 改报 `semantic_roots/retained_roots/created_roots/reused_roots`。

- [ ] **Step 4：运行 App 场景测试并确认 GREEN**

运行：`cargo test -p cli frame_performance --no-fail-fast`

预期：冷帧一次 rebuild；append/update 与 spinner/resize 满足增量计数。

### Task 4：让 renderer 使用共享 roots 与增量布局顺序

- [ ] **Step 1：写失败 renderer 测试**

增加测试：5000 个已测量 roots 后 append 一个 root，renderer 只为新 root建立 layout metadata，只物化窗口候选；宽度变化只精确重排窗口内 roots；滚动加载旧历史时按需精确化候选；删除/reset 后 stale layout 被清除。

- [ ] **Step 2：运行测试并确认 RED**

运行：`cargo test -p cli document_renderer --no-fail-fast`

预期：现有 renderer 每帧遍历全部 roots 并构造全量 `HashSet`/state Vec，候选访问计数超预算。

- [ ] **Step 3：实现增量 RootLayoutIndex**

保留 `root_id -> RootLayoutEntry`，另维护与 retained view 同序的轻量 root order 与累计行 Fenwick/prefix 索引；仅对增量 append/update/remove 修改索引。窗口选择先用累计行定位 root range，再对候选 stale root 执行精确 render，禁止 materialize 窗口外 block。

- [ ] **Step 4：运行 renderer 测试并确认 GREEN**

运行：`cargo test -p cli document_renderer --no-fail-fast`

预期：窗口候选计数受 line limit/overscan 限制，滚动锚点与 root group 不拆分契约保持通过。

### Task 5：覆盖 Resume 与交互语义

- [ ] **Step 1：写失败 Resume/交互测试**

场景必须比较完整参考装配结果与增量装配结果的 block id、kind、tool parent/child、AskUser 状态及 terminal notice；覆盖 Resume、实时流式更新、tool completion、AskUser dismiss、history scroll、selection/copy/link。

- [ ] **Step 2：运行测试并确认 RED**

运行：`cargo test -p cli session_resumed frame_performance history_window selection link --no-fail-fast`

预期：命令按单个过滤器分别执行；至少新增增量语义对照测试因能力缺失失败。

- [ ] **Step 3：修正结构变化与 anchor 处理**

Resume/reset 只触发一次 rebuild；append/update 保留未变化 root identity；remove/reorder 更新 layout 顺序；workspace_root 变化仅使工具 root 失效；placeholder 独立更新，不污染静态历史。

- [ ] **Step 4：运行场景测试并确认 GREEN**

分别运行：

- `cargo test -p cli session_resumed --no-fail-fast`
- `cargo test -p cli history_window --no-fail-fast`
- `cargo test -p cli selection --no-fail-fast`
- `cargo test -p cli link --no-fail-fast`

预期：全部通过，实时与 Resume 输出语义一致。

### Task 6：性能预算与 retained-memory 门禁

- [ ] **Step 1：新增确定性计数门禁**

100/1000/5000 blocks 的 append/update 必须满足：created roots `<= 1`、retained view touched roots `<= 1`、窗口物化 roots 不随历史规模增长；10/50/100 大型 Edit 只物化当前窗口内 diff；spinner-only retained view/document rebuild 均为 0。

- [ ] **Step 2：运行 debug 门禁**

运行：`cargo test -p cli frame_performance retained_state_performance edit_diff_performance --no-fail-fast`

预期：所有确定性计数断言通过。

- [ ] **Step 3：运行 Release workload 并记录 P50/P95**

运行：

- `cargo test -p cli --release tui_retained_output_view_release_workload -- --ignored --nocapture`
- `cargo test -p cli --release edit_diff_release_workload -- --ignored --nocapture`

验收：5000 roots append/update prepare P95 不超过 100 roots 的 2 倍；窗口候选与 diff/highlighter 数保持常数阶；无新增内容的连续 redraw retained root/node 数不增长。

### Task 7：架构守卫、设计同步与旧路径退役

- [ ] **Step 1：新增失败守卫 fixture**

守卫必须拒绝生产路径调用 `assemble_from_conversation`、`Vec<BlockNode>` owned 全量 cache、从完整 timeline 构造 ToolIndex 后再选 viewport，以及无界 output view journal。

- [ ] **Step 2：运行守卫并确认 RED**

运行：`.agents/hooks/check-tui-retained-output-view.sh`

预期：旧生产路径仍存在，守卫失败。

- [ ] **Step 3：删除旧路径并注册守卫**

删除 `OutputViewCache`、生产用完整 `assemble_from_conversation` 与重复 ToolIndex；保留 test-only 完整参考 assembler 仅用于语义对照。把新守卫登记到 `.agents/aemeath.json`，并在测试覆盖设计中记录 L1 model journal、L2 retained-view/renderer、L4 TUI/Resume 场景证据。

- [ ] **Step 4：运行守卫并确认 GREEN**

运行：

- `.agents/hooks/check-tui-retained-output-view.sh`
- `.agents/hooks/check-architecture-guards.sh --full`
- `cargo run -p xtask -- production-reachability .`

预期：全部通过，无旧全量生产引用。

### Task 8：完整验证与收尾

- [ ] **Step 1：格式与定向测试**

运行：

- `cargo fmt --all -- --check`
- `cargo test -p cli --no-fail-fast`
- `git diff --check`

预期：全部通过。

- [ ] **Step 2：workspace 门禁**

运行：

- `cargo build --workspace`
- `cargo test --workspace --no-fail-fast`
- `cargo clippy --workspace --all-targets -- -D warnings`

预期：全部通过、无 warnings。

- [ ] **Step 3：复查验收与废弃代码**

搜索 `assemble_from_conversation`、`OutputViewCache`、完整 `ToolIndex::build`、owned `Vec<BlockNode>` cache；生产引用必须为零，test-only 参考实现必须有清晰命名和 `cfg(test)`。

- [ ] **Step 4：更新 Issue 状态和证据**

将通过的测试、Release workload、守卫结果和现场复测写入 #1422；未完成项必须说明原因、影响和后续处理，不关闭 Issue。
