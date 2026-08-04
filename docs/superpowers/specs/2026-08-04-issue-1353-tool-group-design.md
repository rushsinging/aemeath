# TUI 连续相关工具分组设计

> 对应 Issue：https://github.com/rushsinging/aemeath/issues/1353  
> 状态：已确认设计  
> 范围：仅调整 TUI 展示投影、窗口物化与渲染，不改变 Runtime、ConversationModel、OutputTimeline 或 Session 持久化事实

## 1. 背景

当前 TUI 将每个 ToolCall 作为独立输出根块展示。连续的读取、搜索、命令、写入或任务管理操作会产生大量视觉重复，但这些 ToolCall 在事实层仍必须保持逐条顺序、独立状态和独立结果。

现有输出链路已经具备：

```text
ConversationModel / OutputTimeline
  → RetainedOutputView / OutputWindowIndex
  → OutputViewAssembler
  → OutputViewModel { roots: Vec<Arc<BlockNode>> }
  → OutputDocumentRenderer
  → BlockCache / GuttedCache
```

现有 `BlockNode` 支持树结构，ToolCall 已拥有 ToolResult 子节点；现有缓存也以稳定 `block_id` 与 `block_version` 为粒度。缺口不是“能否画一行标题”，而是如何在不破坏历史窗口原子性、增量缓存和事实顺序的前提下，把相邻工具投影为稳定展示单元。

## 2. 目标

- 将至少两个连续、同类别的 ToolCall 投影为一个轻量 ToolGroup。
- 分组仅存在于 ViewAssembler / ViewModel 展示层。
- 每个 ToolCall、ToolResult 的事实顺序、状态、结果和稳定 identity 保持不变。
- 分组在流式追加、工具状态更新、历史窗口滚动和 Session resume 后保持确定性。
- 分组标题不折叠、不显示数量、不汇总状态，不承担成员动画。
- 成员状态变化只使受影响的 ToolCall / ToolResult 缓存失效，不使整组内容重渲染。

## 3. 非目标

本次不实现：

- 分组折叠；
- 数量或状态汇总；
- 用户自定义分类；
- 分组持久化；
- 跨文本、Thinking、notice 或未知工具合并；
- 改变 ToolCall / ToolResult 的事实顺序；
- Child Run 或 Main/Sub 输出树重构；
- 新增 Runtime、SDK、Model 或 Session schema。

## 4. 方案比较

### 4.1 方案 A：只插入分组标题

在独立 ToolCall 根块前插入标题，不改变树结构。

优点是改动小、风险低；缺点是分组只是视觉假象，无法形成 `ToolGroup → ToolCall → ToolResult`，历史窗口可能只保留标题或部分成员，后续仍需重构。

该方案只适合作为临时止血，不作为最终实现。

### 4.2 方案 B：窗口物化后包装 ToolGroup

先按现有流程选出窗口，再对窗口内相邻 ToolCall 进行包装。

该方案可复用成员缓存，但同一连续区段会在窗口边缘被切开，向上滚动时可能出现分组形态变化；追加工具也会让逐 item 的 root cache 无法准确表达邻接失效。

该方案存在结构性复发风险，不采用。

### 4.3 方案 C：分组感知的展示单元

在展示投影边界先将 timeline 分段为稳定展示单元，再做窗口索引、物化和渲染。普通 item 是单独展示单元，至少两个同类连续 ToolCall 是 ToolGroup 展示单元。

该方案完整解决窗口原子性、邻接失效和树结构问题，成本高于前两种，但职责边界清晰，后续无需推翻。本设计采用方案 C。

## 5. 领域无关的展示分类

### 5.1 ToolGroupKind

ViewModel 定义封闭分类：

| ToolGroupKind | 标题 | 工具 |
|---|---|---|
| `Explore` | `Explore` | `Read`、`Glob`、`Grep` |
| `Run` | `Run` | `Bash` |
| `Write` | `Write` | `Write`、`Edit` |
| `Tasks` | `Tasks` | `TaskListCreate`、`TaskCreate`、`TaskUpdate`、`TaskGet`、`TaskList`、`TaskListComplete`、`TaskStop` |

分类函数是封闭纯函数：已知工具返回对应类别，其他工具返回 `None`。未知工具保持独立展示，禁止按前缀、参数或到达位置猜测分类。

Issue 原文使用 `Explor`，本设计统一为语义完整的 `Explore`，避免把截断拼写固化为类型和用户可见术语。

### 5.2 连续性

分段只读取 `OutputTimelineItem` 的稳定顺序及 ToolCall 的工具名称。

以下项目切断当前分组：

- 不同类别的 ToolCall；
- 未分类 ToolCall；
- UserMessage；
- AssistantText；
- Thinking；
- System、Hook 或 Error notice；
- AskUser；
- orphan ToolResult；
- 其他非 ToolCall 输出。

已嵌入所属 ToolCall 的 ToolResult 不形成独立展示单元，因此不切断分组。ToolCall 的 Running、Success、Error、Cancelled 状态不参与分段；成员失败或取消不切断后续同类工具。

连续区段长度为 1 时保持原有独立 ToolCall；长度至少为 2 时形成 ToolGroup。

## 6. 展示单元规划

新增纯展示层规划器，职责是把可见输出序列转换为展示单元：

```text
OutputTimelineItem sequence
  → classify ToolCall
  → segment adjacent items
  → DisplayUnitPlan
      ├─ Single { item_id }
      └─ ToolGroup { kind, member_item_ids }
```

规划器：

- 只依赖 timeline 的只读 item 信息与 ToolCall 只读查询；
- 不读取 Render、ratatui、缓存或可变 ViewState；
- 不写 Model、timeline 或持久化；
- 不复制 ToolCall 的业务状态；
- 对同一输入始终产生相同计划。

`DisplayUnitPlan` 是 ViewAssembler 内部契约，不进入 Model 或公共 Published Language。

## 7. ViewModel 树

### 7.1 新类型

新增：

- `ToolGroupKind`；
- `ToolGroupBlockView`；
- `OutputBlockKind::ToolGroup(ToolGroupBlockView)`。

逻辑树：

```text
ToolGroup
├── ToolCall
│   └── ToolResult
└── ToolCall
    └── ToolResult
```

单个工具仍保持：

```text
ToolCall
└── ToolResult
```

### 7.2 稳定 identity

ToolGroup 的 `block_id` 由以下稳定字段生成：

```text
tool-group:<kind>:<first-tool-call-stable-id>
```

它不包含成员数量、成员状态或最后一个成员 ID。因此在第二个、第三个同类工具持续追加时：

- 分组 ID 不变；
- 已有 ToolCall / ToolResult ID 不变；
- 只追加新成员；
- 组标题的 `block_version` 不因成员状态或数量变化而改变。

若第一个成员被删除或分段边界改变，展示单元 identity 可以变化；这是根结构真实变化，必须重建受影响单元。

### 7.3 嵌套规则

合法关系扩展为：

| 父节点 | 合法子节点 |
|---|---|
| ToolGroup | ToolCall |
| ToolCall | ToolResult、AssistantMessage、DiagnosticNotice、SystemNotice |
| 其他 | 无 |

逻辑最大深度覆盖 `ToolGroup → ToolCall → ToolResult`。深度常量表达节点层数上限，不再用“只有 ToolCall 可以有子”的旧假设。

非法嵌套不能导致工具消失：debug 构建断言暴露编程错误；生产构建记录诊断，并把可独立展示的 ToolCall 降级为根块。禁止静默丢弃 ToolCall 或 ToolResult。

## 8. 窗口索引与 RetainedOutputView

### 8.1 窗口原子性

窗口索引的原子单位从单个 timeline item 提升为 `DisplayUnitPlan`：

- `Single` 估算并物化一个根；
- `ToolGroup` 估算并物化整个组；
- 窗口选择不从 ToolGroup 中间切开。

`source_total_lines` 与 `folded_earlier_lines` 必须基于展示单元估算，避免窗口索引和实际根树使用两套边界。

### 8.2 增量变化

按变化类型处理：

- **追加同类 ToolCall**：前一个单项与新项升级为组，或追加到现有组；使受影响单元及其前一邻接边界失效。
- **追加异类或切断项**：新增独立展示单元，既有分组保持不变。
- **更新 ToolCall 状态或内容**：分段不变，只使对应成员物化结果失效。
- **删除 ToolCall 或改变工具 identity/name**：重算受影响单元及其前后边界。
- **Reset、resume history 替换、无法证明局部安全的变化**：执行确定性全量重建。

局部增量必须以正确性为前提；不能为了保留缓存而猜测邻接关系。

### 8.3 缓存所有权

RetainedOutputView 缓存展示单元和成员节点：

- ToolGroup 包装节点按稳定组 ID 复用；
- ToolCall / ToolResult 继续使用已有 block ID 与 block version；
- 成员状态变化不改变组标题 version；
- 不再假设一个 timeline item 永远对应一个根缓存项。

缓存容量仍由现有 bounded LRU 约束，不新增无界集合。

## 9. Renderer 与视觉深度

### 9.1 分组标题

ToolGroup 自身只渲染一行轻量标题：

- 使用类别标题；
- 使用 muted/secondary 样式；
- 不显示数量；
- 不显示状态汇总；
- 不折叠；
- 不显示 Running marker；
- 不承担成员动画。

### 9.2 逻辑深度与视觉深度分离

逻辑树增加一层，但 Issue 要求组标题和 ToolCall 成员维持根级视觉宽度，ToolResult 仍只相对 ToolCall 缩进一级。因此 renderer 必须区分：

- **逻辑深度**：用于合法嵌套、DFS 和 root group 原子性；
- **视觉深度**：用于 gutter、有效文本宽度和缓存 key。

视觉映射：

| 节点 | 逻辑深度 | 视觉深度 |
|---|---:|---:|
| ToolGroup | 0 | 0 |
| ToolCall（组成员） | 1 | 0 |
| ToolResult（组成员结果） | 2 | 1 |
| 独立 ToolCall | 0 | 0 |
| 独立 ToolResult 子块 | 1 | 1 |

GuttedCache 的 depth 分量必须使用视觉深度，因为它决定文本宽度和 gutter 布局。DFS、root group 和 nesting 校验继续使用逻辑结构。

ToolGroup 内成员不插入普通根块的额外呼吸空行；分组作为一个 root group 只在组标题前保留一次根级分隔。成员之间依靠现有 ToolCall/ToolResult布局和轻量标题形成连续区段。

## 10. 缓存与动画不变量

- ToolGroup 标题 `block_version` 只取决于 kind 和标题展示字段。
- ToolCall 的状态、参数、activity 和 result payload 继续进入自己的 version。
- ToolResult 的文本、data projection 和 style 继续进入自己的 version。
- Running marker 动画只影响运行中的 ToolCall gutted entry。
- 某成员更新时，其他成员和 ToolGroup 标题必须命中缓存。
- live-set retain 必须 DFS 收集 ToolGroup、ToolCall 和 ToolResult 的全部 ID。
- 淘汰缓存不能改变 ViewModel 或用户可见内容。

## 11. 错误与降级

- 未知工具：作为独立 ToolCall 展示。
- ToolCall 查询失败：不得进入分组；沿用现有独立诊断展示，并携带 timeline item ID 与 ToolCall identity，禁止为了凑分组吞掉 item。
- 展示计划与物化结果不一致：优先拆为独立根块，保证每个可物化工具可见，同时记录 warning 级诊断；诊断必须携带展示单元 ID 与成员 item ID，但不输出工具结果正文。
- 非法嵌套：debug 断言；生产环境记录 warning 级诊断并降级，不丢失成员；诊断携带父/子 block ID 和逻辑深度。
- 窗口增量无法证明安全：全量重建展示计划和索引。
- 分组失败不进入 Model，不产生持久化状态，也不改变工具业务生命周期。

## 12. 测试策略

遵循 `docs/design/03-engineering/04-testing-and-coverage.md` 的 L0–L5 分层，并遵守测试先行。

### L0：编译与结构

- `cargo fmt --check`；
- CLI production/all-targets clippy；
- TUI architecture guards；
- 检查 Render 不反向读取 Model，ViewModel 不依赖 ratatui。

### L1：单元测试

- 所有工具名称的分类；
- 未知工具不分类；
- 单项不分组；
- 同类连续项形成分组；
- 类别变化及每种切断项结束分组；
- 失败、取消和 ToolResult 不改变分段；
- 稳定 group ID 与标题 version。

### L2：模块协作测试

- timeline → DisplayUnitPlan；
- DisplayUnitPlan → `ToolGroup → ToolCall → ToolResult`；
- 追加第二个同类工具时单项升级为组；
- 追加第三个工具时组 ID 与已有成员 ID 保持稳定；
- 状态更新只重建对应成员；
- 删除和边界变化正确重分段；
- resume history 与 live timeline 使用同一分组规则。

### L3：契约测试

- nesting 合法矩阵与最大逻辑深度；
- 窗口不从 ToolGroup 中间截断；
- 视觉深度映射保持成员根级宽度；
- ToolResult 仍相对 ToolCall 缩进一级；
- ToolGroup 标题和未变成员保持缓存命中；
- 运行中 marker 只使对应成员的 gutted cache 失效；
- live-set retain 不泄漏已移除组或成员。

### L4：场景测试

使用现有 TestBackend / TUI scenario 基础设施覆盖：

- 连续 Read / Glob / Grep；
- 连续 Bash；
- Write / Edit；
- Task 工具；
- 文本或 Thinking 切断；
- 运行中成员完成、失败及新同类成员实时追加；
- 滚动窗口和恢复历史中的完整分组。

### L5：系统 smoke

不适用。本功能不改变真实进程、网络、PTY、安装或发布资产边界；PR Test plan 必须记录不适用理由。

Issue 当前预先勾选的 L1/L2/L3 不能视为完成证据；实施开始前应恢复为未完成，只有对应测试实际通过并可追溯后再勾选。

## 13. 设计文档同步

实现过程中必须同步核对并更新：

- `docs/design/02-modules/tui/04-view-layer.md`：block 类型、nesting、ViewAssembler、窗口与缓存；
- `docs/design/02-modules/tui/01-architecture-and-dataflow.md`：ViewModel 与缓存边界；
- `docs/design/03-engineering/03-migration-governance.md`：仅在存在 Current → Target 差距或延期项时记录；
- `docs/design/03-engineering/04-testing-and-coverage.md`：只引用测试分层，不复制规则。

Target 文档必须使用最终代码中的同一术语：`ToolGroup`、`ToolGroupKind::Explore`、展示单元、逻辑深度和视觉深度。

## 14. 实施顺序

1. 修正 Issue 测试 checklist 的预勾选状态，并记录开发前文档—代码差异。
2. 为分类与分段写失败测试。
3. 引入 `ToolGroupKind`、`ToolGroupBlockView` 与 nesting 失败测试。
4. 建立 `DisplayUnitPlan` 与分组感知窗口索引。
5. 调整 RetainedOutputView 的展示单元缓存和邻接失效。
6. 让 OutputViewAssembler 物化 `ToolGroup → ToolCall → ToolResult`。
7. 增加 ToolGroup renderer，并分离逻辑深度与视觉深度。
8. 补齐成员级缓存、窗口、resume 与场景测试。
9. 同步 Target 文档、执行格式化、定向测试、clippy 与架构守卫。

每一步保持可编译；生产实现必须在对应失败测试之后落地。

## 15. 完成定义

- 四类连续工具按规则形成 ToolGroup，单项和未知工具保持独立。
- 所有切断条件、失败/取消、流式追加和边界变化行为确定。
- ViewModel 结构为 `ToolGroup → ToolCall → ToolResult`。
- 分组在窗口滚动和 resume history 中保持原子、稳定。
- 组标题不折叠、无计数、无状态汇总，成员视觉宽度和既有 gutter 语义不变。
- 成员独立更新，未变标题和成员保持缓存命中。
- L0–L4 适用证据通过，L5 有不适用说明。
- Issue、PR Test plan 与 TUI Target 文档中的术语、范围和验证证据一致。
