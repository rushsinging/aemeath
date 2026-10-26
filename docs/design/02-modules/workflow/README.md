# Workflow · 模块总览

> 层级：02-modules / workflow（模块战术设计）
> 状态：Target（设计收敛中，待决见 01-workflow-design.md §11）｜Milestone：v0.2.0｜对应 Issue：#792（S2）/ [#921](https://github.com/rushsinging/aemeath/issues/921)
> 本目录描述 Workflow Graph 的目标设计：Workflow 是独立 Bounded Context，承担 Session 级 Graph orchestration。[01-workflow-design.md](01-workflow-design.md) 是**唯一设计真相源**；旧 Reasoning Graph 设计仅以退役历史说明保留。

## 文档索引

| 编号 | 文档 | 内容 |
|---|---|---|
| 01 | [01-workflow-design.md](01-workflow-design.md) | **唯一设计真相源**：Workflow / Runtime / LLM 边界，Graph 创建与 Kernel 校验，五个 Workflow Tool、四状态生命周期（层级待决）、Session workflows / active 指针、Goal / Run 边界与不变量 |
| — | [01-reasoning-graph.md](01-reasoning-graph.md) | **退役历史说明**：旧 Reasoning Graph（五节点固定 effort 状态机、ReasoningPort、Reasoning Node）设计，仅供追溯，不再是 Target |

## 定位

Workflow 是**支撑域 BC**，Session 级 Graph orchestration 的拥有者：

- Workflow 拥有 **Graph Definition / Node / Edge / State / Failure Routing**；允许 DAG，也允许回边、自环、Join 与并行 `active_nodes`。
- Node Role 是 Workflow-owned 的有限封闭角色集合；普通 Tool 在每次 Model Invocation 前按 Active Node Role 的 capability Profile 收缩。Node Role 不复用可配置 `agents.roles` 身份，只复用 Tools BC 的 capability vocabulary 与 `ToolProfile` 机制；并行 Active Node 先取 capability 并集，仍不得突破 Run 基础 Scope 与父 ceiling。
- **Loop 不是 Runtime 类型，也不是特殊 Graph 节点**：单节点自环、多节点重试、条件回退与周期重复都使用同一套 Node / Edge / State 模型。
- 一个 Session 保存 **0..n 个 Workflow**，同一时刻至多一个 Active（`active_workflow` 指针，0..1）；Workflow 可跨多个 Goal 与 Run 持续存在，**不从属于 Runtime Goal**。`/clear` 是 Session reset，同步、不可恢复地移除该 Session 全部 Workflow 与 active 指针。
- Workflow 生命周期四种已知状态：`Active` / `Inactive`（可被 LLM 重新激活）/ `Completed` / `Cancelled`（不可逆，仅历史查询）；四并列还是 Open / Closed 分层**尚未拍板**。`Suspended` / `Sleeping` / `AwaitingUser` 属 Runtime Run、Runtime Goal 或后台任务能力，不是 Workflow 状态。
- 完成与取消只能由 LLM 经 `workflow_update(complete / cancel)` 显式提交，**Kernel 不自动完成**；用户不直接与 Workflow 交互，只与 LLM 对话。
- Workflow **不调用 LLM、不执行 Tool、不创建 / 调度 / 暂停 / 终止 Run、不驱动 Run 生命周期**；Run 与 Run 之间的继续由对话驱动与 Runtime 授权策略决定（后台自动继续必须有 Runtime 侧授权）。
- **Runtime Goal** 是独立的跨 Run 聚合根；**SubAgent 是 Runtime capability，不是 Graph**。
- 已退役：Reasoning Graph、`ReasoningPort`、Reasoning Node、五节点固定 effort、`GraphPhaseChanged`；**ReasoningLevel 只从配置 / 用户命令静态冻结**，Provider clamp 独立，与 Workflow 解耦。

## 与 Runtime 的协作边界

双方通过三个窄交互面协作，不共享内部聚合：

| 交互面 | 方向 | 说明 |
|---|---|---|
| `workflow_create` / `workflow_list` / `workflow_query` / `workflow_update` / `workflow_node_update` | LLM → Runtime ToolOrchestration → Workflow | 五个 Workflow Tool **始终对 LLM 暴露**；解析、Policy、Hook、执行与 Tool Result 回传归 Runtime，业务校验与 revision 提交归 Workflow；聚合生命周期（`workflow_update`）与节点更新（`workflow_node_update`）职责严格分开 |
| Graph Kernel | Workflow 内部 | 校验创建提案、生命周期迁移、节点结论与 Replan；合法提交 revision + 1，非法返回结构化 Tool Error；ConfirmNode 只做基础结构 / revision / 证据存在校验 |
| Workflow Facts / Evidence 边界（待决） | Runtime → Session facts；Workflow 按显式引用消费 | finalized RunStep / Run facts 如何成为 Workflow 可引用 evidence、是否需要独立 adapter 或入站契约均尚未拍板；已冻结的只有“Runtime 事实不自动等于节点结论” |

Runtime 每轮按 Session 最新状态组织上下文，向 Provider 只传通用 messages / tool schema / model options；**不存在每次 Model Invocation 的 Workflow 专用查询端口**。Workflow Prompt 的具体读取 / 注入协议、刷新时机与预算待决（见 01-workflow-design.md §11）。普通 Tool 的可见范围已冻结为：Runtime 在每次 Model Invocation 前按当前 Active Node 的 Node Role 复用 Tools BC 的 capability-based Tool Profile 收缩；并行 Active Node 取 capability 并集后仍受 Run 基础 Scope 与父 capability ceiling 约束。五个 Workflow Tool 始终可见。Workflow **NEVER** 放宽 Runtime 全局 Policy / Hook / 用户审批，也不负责计时、唤醒与 Run 调度。

## Target 物理目录

Workflow 采用 Hexagonal 最简形态（按需取层，**NEVER** 为对称预建空层）：

```text
agent/features/workflow/
├── Cargo.toml
└── src/
    ├── lib.rs                    # 窄 façade：稳定 PL + composition-only wiring
    ├── domain/
    │   ├── graph.rs              # Graph Definition / Node / Edge / State / Failure Routing 聚合与不变量
    │   ├── kernel.rs             # Graph Kernel：创建校验、迁移、节点结论、Replan、revision
    │   └── status.rs             # WorkflowStatus：Active / Inactive / Completed / Cancelled（层级待决）
    ├── application/              # 五个 workflow tool 用例编排
    └── ports/                    # Workflow Tool façade；Facts / Evidence 入站契约是否需要仍待决
```

Workflow 尚无易变技术外部 detail，故 **NEVER** 预建 `adapters/`；Runtime facts 如何被 Workflow 发现、查询或引用，以及是否需要 Facts adapter / 入站契约仍待决。若后续出现独立的外部协议 detail，再按判据增补 adapters 层。

## 相关文档

- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Provider 端口：[../provider/02-ports-stream-and-client-scope.md](../provider/02-ports-stream-and-client-scope.md)
- Config 分层：[../config/01-config-layer.md](../config/01-config-layer.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-10-07 | 同步 01-workflow-design.md 冻结决策：Session 0..n Workflow、四状态（层级待决）、五个 Workflow Tool、/clear Session reset、Node Role 复用 Tools BC capability Profile 收缩普通 Tool 且 Workflow Tool 始终可见；移除 `WorkflowRuntimeViewQuery` / tool policy / 三类 tool 旧口径；Prompt 读取与状态层级列入待决 | workflow/01-workflow-design.md |
| 2026-07-17 | Graph Engine 单文档设计取代旧 ReasoningPort effort 轨；旧历史说明保留仅用于追溯（当时记录的 `WorkflowRuntimeViewQuery` 消费口径已由 2026-10-07 决策取代） | 历史记录 |
| 2026-07-17 | 明确 Workflow 为独立 BC crate；v0.1.0 旧 Reasoning Graph 设计仅作历史保留，Graph Engine 目标设计统一见 01-workflow-design.md | [#998](https://github.com/rushsinging/aemeath/issues/998) |
| 2026-07-16 | 冻结 Workflow Target 物理目录：Graph Engine 采用 `domain + application + ports` 最简形态，明确不建 `capabilities/` 或无证据 adapter | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-17 | #921 收缩范围记录：Config `reasoning_graph` 与五节点固定 effort 轨退役；Provider resolver 尚未接生产链路；当前 Workflow Graph 设计与生产接线边界以 01-workflow-design.md 为准 | [#921](https://github.com/rushsinging/aemeath/issues/921) |
