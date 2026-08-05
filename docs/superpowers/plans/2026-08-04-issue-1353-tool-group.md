# #1353 TUI 连续相关工具分组实施计划

> 对应设计：`docs/superpowers/specs/2026-08-04-issue-1353-tool-group-design.md`
> 对应 Issue：https://github.com/rushsinging/aemeath/issues/1353
> 执行方式：TDD；每项任务单一、可验证；生产逻辑必须在对应失败测试之后修改

## 0. 范围与执行约束

本计划只修改 TUI 展示投影、历史窗口索引、ViewModel 与 renderer。不得修改 Runtime、SDK Published Language、ConversationModel 事实结构、OutputTimeline 持久化语义或 Session schema。

执行前必须：

1. 基于最新 `origin/main` 创建实现 worktree；设计分支中的两个文档提交需先进入实现分支。
2. 重新读取 `specs/3.2-rust-coding.md`、`specs/3.3-tui-cli.md`、`specs/3.13-bug-feature-tracking.md`、`specs/3.14-workflow.md`。
3. 读取 #1353 的 milestone、labels、body 和全部 checklist；无 milestone 时停止并提醒用户。
4. 更新 Issue：L1/L2/L3 的预勾选恢复为未完成，并加入设计文档链接及“开发前文档—代码差异”。
5. 每次提交前运行 `cargo fmt --all -- --check` 与本任务最窄测试。

## 1. 文件结构

### 新增文件

- `apps/cli/src/tui/view_assembler/tool_group.rs`
  - 唯一工具分类清单；
  - 统一 planner 输入、`DisplayUnitPlan` 与纯分段算法；
  - ToolGroup 稳定 ID 规则；
  - 不依赖 Render、ratatui 或可变 ViewState。

- `apps/cli/src/tui/view_assembler/tool_group_tests.rs`
  - 分类、切断、单项/多项、透明 ToolResult、稳定 ID 的 L1 测试。

- `apps/cli/src/tui/render/output/blocks/tool_group.rs`
  - 只渲染 ToolGroup 轻量标题；不渲染成员、不汇总状态、不处理动画。

- `apps/cli/src/tui/app/scenario_tests/tool_group.rs`
  - TestBackend 用户可见场景和实时状态更新覆盖。

### 修改文件

- `apps/cli/src/tui/view_assembler.rs`
  - 注册 `tool_group` 模块及测试。

- `apps/cli/src/tui/view_model/output.rs`
  - 新增 `ToolGroupKind`、`ToolGroupBlockView` 和 `OutputBlockKind::ToolGroup`。

- `apps/cli/src/tui/view_model.rs`
  - 导出 ToolGroup ViewModel 类型。

- `apps/cli/src/tui/view_model/nesting.rs`
  - 合法化 `ToolGroup → ToolCall → ToolResult`，更新最大逻辑深度和测试。

- `apps/cli/src/tui/view_assembler/output.rs`
  - 把已有单 item 物化能力收窄为可复用成员物化；
  - 新增按 `DisplayUnitPlan` 物化 ToolGroup 的入口；
  - 保持 ToolCall/ToolResult 原有 block ID 和 payload。

- `apps/cli/src/tui/view_assembler/resumed_history.rs`
  - 将已加载 history item 映射为共享 planner 输入；
  - 匹配 ToolResult 作为透明关联项，orphan 作为切断项；
  - 不复制工具清单。

- `apps/cli/src/tui/view_assembler/output_window_index.rs`
  - 索引 entry 从 item identity 改为展示单元 identity；
  - 仍维持 prefix-lines 与尾部二分选择复杂度；
  - ToolGroup 是窗口原子。

- `apps/cli/src/tui/view_assembler/output_window_index_tests.rs`
  - 展示单元原子性、超长组、增量变化与性能读取数测试。

- `apps/cli/src/tui/view_assembler/retained_output_view.rs`
  - 统一构建 live/resume 展示计划；
  - 应用邻接感知失效；
  - 按展示单元物化和缓存；
  - display history invalidate 时确定性重建。

- `apps/cli/src/tui/view_assembler/retained_output_view_tests.rs`
  - 单项升级为组、追加、切断、成员复用、resume placeholder/加载/淘汰/重载测试。

- `apps/cli/src/tui/render/output/blocks.rs`
  - 注册 ToolGroup renderer。

- `apps/cli/src/tui/render/output/block_component.rs`
  - 为 `OutputBlockKind::ToolGroup` 分发标题组件。

- `apps/cli/src/tui/render/output/gutter.rs`
  - 为 ToolGroup 定义无运行态动画的轻量 marker/空 marker 语义；保持窄屏策略。

- `apps/cli/src/tui/render/output/document_renderer.rs`
  - DFS 时分别传递逻辑深度与视觉深度；
  - ToolGroup 成员视觉深度不增加；
  - root breathing space 只在 ToolGroup 根前插入一次；
  - gutted cache key 使用视觉深度。

- `apps/cli/src/tui/app/scenario_tests.rs`
  - 注册 tool_group 场景。

- `docs/design/02-modules/tui/01-architecture-and-dataflow.md`
  - 补充展示单元规划和 live/resume 共享规则。

- `docs/design/02-modules/tui/04-view-layer.md`
  - 更新 block 类型数量、ToolGroup 树、nesting、窗口和缓存契约。

- `docs/design/03-engineering/03-migration-governance.md`
  - 仅在实施中产生明确延期或 Current→Target 残差时修改；无残差则不改。

## 2. 任务清单

### 任务 1：修正 Issue 开发门禁

**操作**

1. 使用 `gh issue view 1353 --repo rushsinging/aemeath --json ...` 核对 milestone、labels、body、comments。
2. 保存原 body，精确把 L1/L2/L3 的 `[x]` 改回 `[ ]`，其他 checklist 不动。
3. 在 body 增加：
   - Target 文档：`docs/design/02-modules/tui/01-architecture-and-dataflow.md`、`04-view-layer.md`；
   - 设计规格路径；
   - 当前代码缺少 ToolGroup、窗口仍按 item 索引、resume 使用惰性 step placeholder；
   - 本 Issue 修复项与明确非目标。
4. 重新读取 body，确认只有预期变化。

**验证**

```bash
gh issue view 1353 --repo rushsinging/aemeath --json milestone,body,updatedAt
```

**完成条件**：milestone 存在；L1–L3 未预勾选；开发前差异清单可追溯。

---

### 任务 2：为工具分类写失败测试

**新增**：`view_assembler/tool_group_tests.rs`
**修改**：`view_assembler.rs`

**测试先行**

添加测试锁定：

- Read/Glob/Grep → Explore；
- Bash → Run；
- Write/Edit → Write；
- 当前 9 个显式 Task 工具 → Tasks；
- `TaskList`、`TaskFutureTool`、`taskCreate`、未知工具 → 不分类；
- 用户可见标题使用 `Explore`，不使用 `Explor`。

运行并确认因模块/类型不存在而失败：

```bash
cargo test -p cli --bin aemeath tool_group_classification -- --nocapture
```

**完成条件**：失败原因只指向尚未实现的 ToolGroup 分类能力。

---

### 任务 3：实现唯一显式分类清单

**新增**：`view_assembler/tool_group.rs`

实现：

- `ToolGroupKind` 使用 ViewModel-owned 类型；
- `classify_tool_name(&str) -> Option<ToolGroupKind>`；
- 显式匹配 9 个 Task 工具，不使用 `starts_with("Task")`；
- `ToolGroupKind::title()` 或等价单一标题来源；
- 文件中只承担分类与后续 planner 共享类型，不访问 Model。

**验证**

```bash
cargo test -p cli --bin aemeath tool_group_classification -- --nocapture
cargo fmt --all -- --check
```

**提交**

```text
feat(tui): #1353 定义工具分组分类
```

---

### 任务 4：为纯分段规划器写失败测试

**修改**：`view_assembler/tool_group_tests.rs`

定义测试输入 builder，不构造完整 ConversationModel。覆盖：

- 单个候选输出 `Single`；
- 两个/三个同类候选输出 ToolGroup；
- 类别变化拆组；
- Text/Thinking/User/System/Hook/Error/AskUser/unknown tool/orphan result 切断；
- 匹配 ToolResult 是透明关联项；
- 成员 Success/Error/Cancelled 不改变分段；
- 追加第二项时 group ID 基于首成员；
- 追加第三项时 group ID 不变；
- live/resume 来源差异不参与 group ID。

**验证**

```bash
cargo test -p cli --bin aemeath tool_group_planner -- --nocapture
```

**完成条件**：测试因 planner 未实现而失败，测试数据没有复制生产算法。

---

### 任务 5：实现共享 DisplayUnitPlan

**修改**：`view_assembler/tool_group.rs`

实现最小纯算法：

- 输入：带稳定 item ID、来源边界、候选 kind/ToolCall ID、透明/切断语义的切片；
- 输出：`Single` 或 `ToolGroup { id, kind, member_ids }`；
- 同一 resume step 通过 boundary token 限制分组；live 当前序列使用统一 boundary；
- 透明 ToolResult 归属于成员但不成为 root；
- 单项区段退化为 Single；
- group ID 为 kind + boundary identity + first ToolCall stable identity；
- 不包含状态、成员数量或位置索引。

**验证**

```bash
cargo test -p cli --bin aemeath tool_group_planner -- --nocapture
cargo fmt --all -- --check
```

**提交**

```text
feat(tui): #1353 建立展示单元分段规划器
```

---

### 任务 6：为 ToolGroup ViewModel 和 nesting 写失败测试

**修改**：

- `view_model/output.rs`
- `view_model/nesting.rs`
- `view_model.rs`

先只添加测试，覆盖：

- ToolGroup View 数据只包含 stable key、kind、title/style；
- ToolGroup 可包含 ToolCall；
- ToolCall 仍可包含 ToolResult/既有 notice 子块；
- ToolGroup 不能含文本、ToolResult 或另一个 ToolGroup；
- ToolCall 之外的节点不能直接包含 ToolCall；
- 最大逻辑层级容纳 group→call→result，超深非法。

**验证**

```bash
cargo test -p cli --bin aemeath nesting -- --nocapture
```

---

### 任务 7：实现 ToolGroup ViewModel 契约

**修改**：

- `view_model/output.rs`
- `view_model/nesting.rs`
- `view_model.rs`

实现新 enum variant 和合法嵌套，不引入 ratatui 类型。更新所有 `OutputBlockKind` 穷尽匹配的编译错误点，但 renderer 分支在此任务只允许返回明确待实现测试桩时不可提交；更优先让后续 renderer 任务与本任务在同一可编译提交中完成最小标题组件。

**验证**

```bash
cargo test -p cli --bin aemeath nesting -- --nocapture
cargo check -p cli --bin aemeath
```

**提交**

```text
feat(tui): #1353 扩展 ToolGroup 视图树契约
```

---

### 任务 8：为 live timeline adapter 写失败测试

**修改**：`view_assembler/tool_group_tests.rs`

用真实 `ConversationModel`/`OutputTimelineItem` 与测试 ToolCall builder 覆盖：

- adapter 读取 ToolCall 名称并生成候选；
- embedded ToolResult 生成透明关联；
- orphan ToolResult 生成切断项；
- 查不到 ToolCall 时不吞 item，生成切断/诊断输入；
- 其他 timeline variant 映射为切断项；
- 适用 runtime context 不影响分类。

**验证**

```bash
cargo test -p cli --bin aemeath live_tool_group_adapter -- --nocapture
```

---

### 任务 9：实现 live adapter 与单元物化入口

**修改**：

- `view_assembler/tool_group.rs`
- `view_assembler/output.rs`

实现：

- `OutputTimelineItem` → shared planner input；
- 将现有 `assemble_item` 中 ToolCall 的构造抽为单成员物化函数；
- 按 `DisplayUnitPlan` 物化 Single 或 ToolGroup；
- ToolGroup children 复用原 ToolCall 节点和 ToolResult 子节点；
- 非法/缺失成员降级为独立根并记录 warning，不丢 item；
- 日志 target 和字段遵守 `specs/3.15-logging.md`，不输出结果正文。

**验证**

```bash
cargo test -p cli --bin aemeath live_tool_group_adapter -- --nocapture
cargo test -p cli --bin aemeath output_view_assembler -- --nocapture
```

**提交**

```text
feat(tui): #1353 物化实时工具分组树
```

---

### 任务 10：为 Resume adapter 写失败测试

**修改**：

- `view_assembler/tool_group_tests.rs`
- 必要时在 `view_assembler/resumed_history.rs` 的同级测试模块增加 fixture

覆盖：

- StepPlaceholder 输出不可分组的 Single/加载边界；
- 已加载 step 的 ToolUse 映射候选；
- provider tool ID 匹配的 ToolResult 是透明关联，即使位于后续 user message；
- orphan ToolResult 切断；
- 相邻 step 的同类工具不分为同组；
- Task 工具复用唯一显式清单；
- adapter 不修改 conversation revision。

**验证**

```bash
cargo test -p cli --bin aemeath resume_tool_group_adapter -- --nocapture
```

---

### 任务 11：实现 Resume adapter

**修改**：

- `view_assembler/resumed_history.rs`
- `view_assembler/tool_group.rs`

实现：

- 用 step identity 作为 boundary；
- 从 `LocalResumeContentBlock::ToolUse` 读取真实工具名和 stable tool ID；
- 使用现有 ToolResult 查找契约产生透明关联，不按相邻位置猜测；
- placeholder 保持独立；
- 不在 resume 模块复制分类 match；
- 提供按展示计划物化 history group 的入口。

**验证**

```bash
cargo test -p cli --bin aemeath resume_tool_group_adapter -- --nocapture
cargo test -p cli --bin aemeath resumed_history -- --nocapture
```

**提交**

```text
feat(tui): #1353 统一恢复历史工具分组
```

---

### 任务 12：为展示单元窗口索引写失败测试

**修改**：`view_assembler/output_window_index_tests.rs`

先把测试术语从 item 改为 display unit，覆盖：

- ToolGroup 一个 entry 对应多个成员；
- line limit 不从 group 中间截断；
- 单组超过 limit 时仍完整选择；
- source total/folded lines 按 unit 计算；
- exact line update 只失效对应 unit；
- 100k unit 尾部选择仍维持现有次线性读取断言；
- reset/append/update/remove 使用 unit ID。

**验证**

```bash
cargo test -p cli --bin aemeath output_window_index -- --nocapture
```

---

### 任务 13：把 OutputWindowIndex 提升为展示单元索引

**修改**：`view_assembler/output_window_index.rs`

执行受控重命名：

- `item_id` → `unit_id`；
- `item_range` → `unit_range`；
- change payload 同步；
- 保持 prefix-lines 和二分窗口选择算法；
- 保持 exact layout width 失效规则；
- 不在 index 内加入 ToolGroup 业务分类。

**验证**

```bash
cargo test -p cli --bin aemeath output_window_index -- --nocapture
cargo check -p cli --bin aemeath
```

**提交**

```text
refactor(tui): #1353 统一展示单元窗口索引
```

---

### 任务 14：为 RetainedOutputView 邻接失效写失败测试

**修改**：`view_assembler/retained_output_view_tests.rs`

覆盖 live：

- 首个 Read 是独立根；
- 追加 Grep 后升级为 Explore 组；
- 追加 Glob 后组 ID 和前两个成员 Arc identity 保持；
- 更新第二个成员只替换第二个成员，标题/第一成员复用；
- 追加 Text 切断，旧组不变；
- Remove/rename 使前后边界正确重算；
- 未分类 TaskFutureTool 不进入 Tasks 组；
- indexed_items 语义改为 indexed_units。

覆盖 resume：

- placeholder 不分组；
- `apply_window` 后组装同 step 工具；
- 不跨 step；
- step 淘汰恢复 placeholder 并清理 group live set；
- 重新加载恢复相同 group ID；
- materialize 不前进 ConversationModel revision。

**验证**

```bash
cargo test -p cli --bin aemeath retained_output_view -- --nocapture
```

---

### 任务 15：实现分组感知 RetainedOutputView

**修改**：`view_assembler/retained_output_view.rs`

实现策略：

1. 从 display history 与 live timeline 按最终展示顺序生成 planner inputs。
2. 构建 `DisplayUnitPlan` 列表并据此同步窗口 index。
3. 对 delta 变化：先标记变更 item 的前一/当前/后一分段边界；无法证明局部安全时 `rebuild_all`。
4. root cache key 改为 unit ID；ToolGroup wrapper 重建时从成员缓存复用 `Arc<BlockNode>` 所需结构。若当前 `BlockNode.children` 不是 Arc，需在本任务评估：
   - 优先只重建轻量 wrapper，成员内容通过 block renderer cache 复用；
   - 不为追求 Arc identity 扩大公共树类型重构，测试断言改为稳定 block ID 与 renderer cache 命中。
5. display history invalidate 始终重建 planner/index，但不写 ConversationModel。
6. missing history request 仍根据选中 placeholder 的原始 item IDs 生成，不能提交 ToolGroup ID 给 SDK。

**验证**

```bash
cargo test -p cli --bin aemeath retained_output_view -- --nocapture
cargo test -p cli --bin aemeath indexed_resume -- --nocapture
```

**提交**

```text
feat(tui): #1353 增量保留工具分组窗口
```

---

### 任务 16：为 ToolGroup 标题和视觉深度写失败测试

**修改**：

- `render/output/blocks.rs` 测试模块或新增模块内测试；
- `render/output/document_renderer.rs` 的测试模块；
- `render/output/gutter.rs` 测试。

覆盖：

- 标题只显示 Explore/Run/Write/Tasks，无数量和汇总状态；
- ToolGroup 不使用 Running marker；
- 组内 ToolCall 文本宽度与独立根 ToolCall 相同；
- 组内 ToolResult 只比 ToolCall 多一级视觉缩进；
- 组前只有一个 root breathing line，成员前不重复插入；
- 组标题、未变成员在单成员状态更新后命中 block/gutted cache；
- animation frame 只改变 running ToolCall 的 viewport marker，不使标题失效；
- 30/50 列窄屏阈值保持既有行为。

**验证**

```bash
cargo test -p cli --bin aemeath tool_group_render -- --nocapture
```

---

### 任务 17：实现 ToolGroup renderer 与双深度遍历

**新增**：`render/output/blocks/tool_group.rs`
**修改**：

- `render/output/blocks.rs`
- `render/output/block_component.rs`
- `render/output/document_renderer.rs`
- `render/output/gutter.rs`

实现：

- ToolGroup component 只画 muted 标题；
- DFS 函数参数显式命名 `logical_depth` 与 `visual_depth`；
- ToolGroup child 的 logical +1、visual 不变；
- ToolCall child result 的 logical +1、visual +1；
- nesting/DFS 用 logical，text width、gutter、GuttedKey 用 visual；
- breathing line 依据 root group，而不是 `visual_depth == 0`，避免每个组成员加空行；
- cache live set 继续 DFS 收集所有 block ID。

**验证**

```bash
cargo test -p cli --bin aemeath tool_group_render -- --nocapture
cargo test -p cli --bin aemeath document_renderer -- --nocapture
cargo fmt --all -- --check
```

**提交**

```text
feat(tui): #1353 渲染轻量工具分组
```

---

### 任务 18：增加 L4 TUI 场景测试

**新增**：`app/scenario_tests/tool_group.rs`
**修改**：`app/scenario_tests.rs`

使用现有 `TuiScenarioHarness` 和 TestBackend，覆盖用户可见旅程：

1. Read → Glob → Grep 显示一个 Explore 标题，成员 header/result 均存在。
2. AssistantText/Thinking 后的新 Read 不并入前组。
3. 两个 Bash 显示 Run；第一个失败不切断第二个。
4. Write/Edit 显示 Write。
5. 两个当前 Task 工具显示 Tasks；TaskFutureTool 独立。
6. 运行中成员完成并追加同类成员时，标题无计数、成员状态独立。
7. 历史窗口加载后同 step 工具成组，不跨 step。

避免对整屏不稳定内容做过宽 snapshot；断言标题、顺序、缩进和关键样式/marker。

**验证**

```bash
cargo test -p cli --bin aemeath scenario_tests::tool_group -- --nocapture
```

**提交**

```text
test(tui): #1353 覆盖连续工具分组场景
```

---

### 任务 19：同步 TUI Target 文档

**修改**：

- `docs/design/02-modules/tui/01-architecture-and-dataflow.md`
- `docs/design/02-modules/tui/04-view-layer.md`
- 有明确延期时才修改 migration governance

同步内容：

- block 类型清单不再写死“10 种”，改为与代码穷尽枚举一致的当前数量；
- ToolGroup 来源、树结构、逻辑/视觉深度；
- 展示单元窗口原子；
- live/resume adapter + shared planner；
- placeholder、step boundary、ToolResult identity 回绑、LRU 淘汰/重载；
- 当前 9 个显式 Task 工具和显式更新责任；
- 成员级缓存与动画不变量；
- 不修改事实层和持久化。

**验证**

```bash
rg -n '10 种|Explor\b|TaskList\b|ToolGroup|视觉深度|StepPlaceholder' \
  docs/design/02-modules/tui \
  docs/superpowers/specs/2026-08-04-issue-1353-tool-group-design.md
git diff --check
```

**提交**

```text
docs(tui): #1353 对齐工具分组目标架构
```

---

### 任务 20：执行定向与完整 CLI 验证

按由窄到宽顺序运行：

```bash
cargo fmt --all -- --check
cargo test -p cli --bin aemeath tool_group -- --nocapture
cargo test -p cli --bin aemeath output_window_index -- --nocapture
cargo test -p cli --bin aemeath retained_output_view -- --nocapture
cargo test -p cli --bin aemeath resumed_history -- --nocapture
cargo test -p cli --bin aemeath
cargo clippy -p cli --bin aemeath -- -D warnings
cargo clippy -p cli --all-targets -- -D warnings
```

然后执行仓库架构门禁：

```bash
scripts/setup-dev-env.sh --check
.agents/hooks/check-agent-stop.sh
```

若 Stop Hook 报阻断，按仓库规则优先修复阻断根因。记录每条命令、耗时与结果，不用历史运行结果替代本分支验证。

**完成条件**：L0–L4 全部通过；L5 在 PR 中记录“不改变真进程/网络/PTY/发布资产，故不适用”。

---

### 任务 21：更新 Issue 完成证据

**操作**

1. 逐项核对 #1353 验收标准。
2. 只有测试真实通过后才勾选 L1–L4。
3. 写入最终文档—代码差异状态：已对齐、已修正文档或明确延期。
4. 若存在未闭合项，创建承接 Issue 前必须先询问用户；未经同意不拆 Issue。
5. 保持 #1353 Open，等待 PR 合并和用户确认，Agent 不自行关闭。

**验证**

```bash
gh issue view 1353 --repo rushsinging/aemeath --json body,state,comments
```

---

### 任务 22：请求代码审查并创建 PR

1. 使用 `superpowers:requesting-code-review` 对设计契约、增量正确性、resume 和缓存行为进行独立审查。
2. 修复所有 Critical/Important 问题并重新运行受影响验证。
3. 确认 worktree 只包含 #1353 范围提交。
4. push 实现分支并创建 PR，正文包含：
   - Summary；
   - `Refs #1353` 或按仓库关闭策略使用 `Closes #1353`；
   - Breaking change: No；
   - L0–L5 Test plan；
   - 文档、代码、测试和 Guard 对照；
   - L5 不适用理由；
   - 无未解释 checklist。
5. 不自行 merge，不自行关闭 Issue。

## 3. 最终验收矩阵

| 需求 | 证据 |
|---|---|
| 四类显式工具分类 | classification L1 tests |
| 当前 9 个 Task 工具且无前缀兜底 | explicit-list contract tests |
| 连续/切断/失败行为 | planner L1 + assembler L2 |
| ToolGroup→ToolCall→ToolResult | nesting L3 + assembler L2 |
| 窗口完整组原子 | window index L3 |
| 增量追加和成员独立更新 | retained view L2 + cache L3 |
| 逻辑/视觉深度 | renderer L3 |
| 标题无折叠/计数/汇总 | component L1 + scenario L4 |
| Resume placeholder/step/tool result/LRU | resume L2/L3/L4 |
| 不修改事实层或持久化 | architecture L0 + code review |
| 用户可见布局 | TestBackend L4 |

## 4. 预期提交序列

1. `feat(tui): #1353 定义工具分组分类`
2. `feat(tui): #1353 建立展示单元分段规划器`
3. `feat(tui): #1353 扩展 ToolGroup 视图树契约`
4. `feat(tui): #1353 物化实时工具分组树`
5. `feat(tui): #1353 统一恢复历史工具分组`
6. `refactor(tui): #1353 统一展示单元窗口索引`
7. `feat(tui): #1353 增量保留工具分组窗口`
8. `feat(tui): #1353 渲染轻量工具分组`
9. `test(tui): #1353 覆盖连续工具分组场景`
10. `docs(tui): #1353 对齐工具分组目标架构`

允许在执行中因编译原子性合并相邻提交，但不得把测试先行证据和生产实现倒序，也不得把无关改动混入。
