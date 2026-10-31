# Workflow · 模块总览

> 层级：02-modules / workflow（模块战术设计）
> 状态：Target（MVP 已实现，开关默认关闭）｜Milestone：v0.2.0｜对应 Issue：[#555](https://github.com/rushsinging/aemeath/issues/555)
> 本目录描述 Workflow Graph 的目标设计：Workflow 是独立 Bounded Context，承担 Session 级 Graph orchestration。[01-workflow-design.md](01-workflow-design.md) 是**唯一设计真相源**；旧 Reasoning Graph 设计仅以退役历史说明保留。

## 文档索引

| 编号 | 文档 | 内容 |
|---|---|---|
| 01 | [01-workflow-design.md](01-workflow-design.md) | **唯一设计真相源**：Workflow / Runtime / LLM 边界，Graph 创建与 Kernel 不变量，六个 `workflow_*` Tool 协议，五状态生命周期与 ActiveMode，Node 五状态含 Stopped，Replanning continuation，持久化与 `/clear`，MVP 验收链 A–G 45 场景，不变量与 POST-MVP 清单 |
| — | [01-reasoning-graph.md](01-reasoning-graph.md) | **退役历史说明**：旧 Reasoning Graph（五节点固定 effort 状态机、ReasoningPort、Reasoning Node）设计，仅供追溯，不再是 Target |

## 定位

Workflow 是**支撑域 BC**，Session 级 Graph orchestration 的拥有者：

- Workflow 拥有 **Graph Definition / Node / Edge / State**；只允许 DAG，**禁止**回边、自环与 Failure Routing。
- Graph 有且只有一个普通 root 与一个显式 CompleteNode；CompleteNode 身份在 Workflow 生命周期内稳定，不承担用户确认，也不执行可能失败的业务工作。
- **六个 `workflow_*` Tool**：`workflow_create` / `workflow_list` / `workflow_query` / `workflow_update` / `workflow_node_update` / `workflow_replan`。它们始终出现在 Tool Schema，仍受全局开关、Policy、Hook、用户审批与执行期复验约束。
- **WorkflowStatus 五状态**：Active / Inactive / Completed / Cancelled / Failed；Active 内区分 Executing / Replanning 两种模式。Inactive 携带 `pending_mode`，从 Replanning 挂起后恢复仍回 Replanning。
- **NodeStatus 五状态**：Pending / Active / Completed / Failed / Stopped。Failed 与 Stopped 是互不相同的不可逆终态；任一 Active Node 失败即让整个 Workflow 进入 Replanning，其他并行分支原子转 Stopped。
- **Replan 使用 continuation subgraph**：`ReplanBoundary` 表达历史来源（不是 Node、不是 Edge），`entry_node_ids` 声明新入口，`rejoin_edges` 必须汇入既有 Pending 非 CompleteNode；不得直接连接或替换 CompleteNode。
- **Node Role 是语义标签**：MVP 不启用任何 Tool 裁剪（POST-MVP，见真相源 §11）。
- **Loop 不是 Runtime 类型，也不是特殊 Graph 节点**：重复工作通过 Replanning 提交新的 continuation subgraph 表达，不通过回边或自环。

## 与 Runtime 的边界

```text
Runtime
├─ 构造 Context Window 与 Prompt
├─ 调用 LLM
├─ 路由六个 workflow_* Tool（Policy / Hook / 审批 / 复验）
└─ 装配稳定规则（system prompt）与状态提示（Reminder）

Workflow
├─ 校验 Graph 结构不变量（Kernel）
├─ 维护 Node 状态机与聚合生命周期状态机
├─ 校验并提交 continuation subgraph
└─ 持久化 Session 索引与实体快照
```

Workflow **不**创建、调度、暂停或终止 Run；**不**调用 LLM；**不**执行 Tool；**不**解释执行事实的业务含义。

## 相关文档

- [01-workflow-design.md](01-workflow-design.md) —— 设计真相源
- [docs/design/02-modules/README.md](../README.md) —— 模块文档索引
- [specs/3.10-storage.md](../../../specs/3.10-storage.md) §3.10.2.1 —— Workflow 落盘与统一清理契约

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-10-26 | 目录级说明同步六 Tool、五状态生命周期与 Replanning continuation 口径 | [#2090](https://github.com/rushsinging/aemeath/issues/2090) |
| 2026-10-29 | 按已落地实现（#2082–#2089）重写：DAG 禁回边自环、六 Tool、五状态、Stopped、continuation、Node Role MVP 不启用 | [#2091](https://github.com/rushsinging/aemeath/issues/2091) |
