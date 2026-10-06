# Workflow · 单一设计结论

> 状态：设计收敛中｜目标版本：v0.2.0
> 本文只记录已经确认的 Workflow / Runtime / LLM 边界与交互结论，不同步其他设计文档。

## 1. 核心定位

### 1.1 Runtime

Runtime 负责一次 Run 的执行生命周期：

```text
Run
  ├─ 接收用户或后台运行时触发的输入
  ├─ 构造 Context Window 与 Prompt
  ├─ 通过 Prompt 调用 LLM
  ├─ 处理 LLM 返回的 Tool Call
  ├─ 执行 Tool / Policy / Hook / Interaction
  ├─ 将 Tool Result 放回下一次 Prompt
  ├─ 管理 RunStep、取消、暂停、失败和终态
  └─ 发布 Run / Step finalized facts
```

Runtime **只能通过 Prompt 调动 LLM**。Runtime 不直接调用 Workflow 内部的 LLM，也不把 Workflow 聚合对象直接传给 LLM。

Runtime 不负责：

- 理解完整 Workflow 业务语义；
- 判断用户整体目标是否完成；
- 自动替用户决定 Run 与 Run 之间是否继续；
- 直接修改 Workflow Definition 或 Workflow State。

### 1.2 LLM

LLM 是 Runtime 通过 Prompt 调用的模型能力。LLM 不直接拥有 Workflow 状态，也不直接修改 Workflow。

LLM 可以：

- 通过普通 Tool 执行实际工作；
- 通过 `workflow_create` 提交 Workflow 创建提案；
- 通过 `workflow_query` 查询 Workflow 状态和节点结论；
- 通过 `workflow_update` 提交节点结果、证据或 Replan 提案。

Tool Call 必须经过 Runtime 的通用 ToolOrchestration，并由对应 Workflow Application / Kernel 校验其业务语义。

### 1.3 Workflow

Workflow 是工作流 Graph 的定义、状态和节点结果的拥有者：

- Workflow 保存 Node、Edge、Graph State 和 Failure Routing；
- Workflow 提供 Runtime 所需的最小查询视图：当前 Workflow 是否有效、当前有哪些活动节点、当前请求是否允许节点移动；
- Workflow 提供 `workflow_query` / `workflow_update`，并校验 LLM 提交的状态更新、节点结果和 Replan；
- Workflow 提供激活时的默认 Prompt 提示。

Workflow 不负责：

- 调用 LLM；
- 执行 Tool；
- 创建或启动 Runtime Run；
- 管理 Run 的取消、暂停和终态；
- 决定 Run 与 Run 之间是否继续；
- 代替用户判断整体目标是否成功；
- 管理后台任务的定时唤醒。

## 2. Workflow 的创建

Workflow 由 LLM 创建，但创建必须通过 Runtime 已有的 Prompt → Tool Call 链路：

```text
Runtime 构造 Prompt
  ↓
LLM 调用 workflow_create
  ↓
Runtime ToolOrchestration
  ↓
Workflow Application
  ↓
Workflow Kernel 校验
  ↓
Workflow 创建并返回 activation receipt
  ↓
Tool Result 回到下一次 Prompt
```

LLM 提交的是结构化提案，不是任意 JSON：

```text
WorkflowDefinitionProposal
  ├─ objective
  ├─ nodes
  ├─ edges
  ├─ initial_nodes
  └─ failure_routing```

Kernel 至少校验：

- 初始节点集合非空且存在；
- 节点和边引用有效；
- 节点可从入口到达；
- 允许 DAG，也允许存在回边和自环；
- 并行分支的 Join 条件可判定；
- 失败路由引用有效且不会产生未定义出口；
- 工具策略只引用系统已注册能力；
- 不允许节点突破 Runtime 全局 Policy / Hook / 用户审批边界；
- 生成新的 Workflow revision。

`workflow_create` 成功后返回：

```text
WorkflowActivationReceipt
  ├─ workflow_id
  ├─ revision
  ├─ initial_node
  ├─ runtime_view
  └─ activation_prompt
  ```

  ### 2.1 Workflow 生命周期

  Workflow 只保留两类生命周期状态：

  ```text
  WorkflowStatus
    ├─ Active
    └─ End
        ├─ Completed
        └─ Cancelled
  ```

  - `Active`：Session 当前可使用的 Workflow，可以被 `workflow_query` 查询和被 `workflow_update` 更新。
  - `End.Completed`：Workflow 正常结束，只保留历史查询，不再接受新的流程更新。
  - `End.Cancelled`：Workflow 被用户或系统取消，只保留历史查询，不再接受新的流程更新。

  `Suspended`、`Sleeping`、`AwaitingUser` 不是 Workflow 状态，分别属于 Runtime Run、Runtime Goal 或后台任务能力。Session 的 `active_workflow` 只指向 `Active` Workflow；Workflow 进入任一 `End` 状态后，该槽位清空。

## 3. Workflow 如何约束 LLM

Workflow 通过节点上下文、可用工具范围和移动校验约束 LLM。Runtime 不解释 Workflow 内部节点语义，只消费 Workflow 返回的最小运行视图。

### 3.1 Runtime 最小运行视图

```text
WorkflowRuntimeView
  ├─ validity
  ├─ active_nodes
  └─ movement```

它只回答：

1. 当前绑定的 Workflow 是否仍然有效；
2. 当前有哪些节点处于 Active、可以接收一次 Runtime 执行；
3. 当前请求是否允许发生节点移动。

Runtime 不负责计算下一节点、解释节点语义、判断迁移条件或修改 Workflow。节点移动是否合法，由 Workflow Kernel 判断。

`WorkflowRuntimeView` 的 `movement` 只表达当前请求是否允许移动，不是 Runtime 计算出的下一节点。具体目标节点、迁移条件、节点结果和 Replan 语义仍由 Workflow Kernel 处理。


### 3.2 Tool Schema 裁剪

当前节点可以声明本次模型调用可见的工具；并行节点分别使用自己的 Tool policy：

```text
Planner：
  workflow_query / workflow_update / Read / Grep

Explorer：
  Read / Glob / Grep / workflow_query / workflow_update

Actor：
  Read / Edit / Write / Bash / workflow_query / workflow_update

Reviewer：
  Read / Grep / Bash / workflow_query / workflow_update
```

当前节点明确禁止的 Tool 不进入本次 Prompt 的 Tool Schema。

### 3.3 Tool 执行门禁

LLM 即使伪造了被禁止的 Tool Call，Runtime 仍必须在执行前拒绝：

```text
LLM Tool Call
  ↓
Workflow Node Tool Policy Gate
  ├─ denied → Tool Error
  └─ allowed
       ↓
Runtime Global Policy / Hook / Approval
       ↓
Tool 执行
```

有效工具范围是逐层收紧，而不是 Workflow 替代 Runtime 安全策略：

```text
effective_tools(node) =
    node.tool_policy
    ∩ runtime_available_tools
    ∩ global_policy_allowed_tools
```

Workflow 只能限制工具，不能放宽 Runtime 全局权限。

### 3.3 Workflow Completion Gate

当前节点可以声明本次 Run 收口前必须完成的 Workflow 协议。例如探索节点必须提交节点结论，验证节点必须提交验证结果。

如果 LLM 只输出普通文本：

```text
我已经完成探索，项目使用 Rust。
```

但没有调用 `workflow_update`，Runtime 不能直接接受该模型输出作为 Workflow 节点完成。收口门返回结构化协议错误：

```text
当前节点尚未完成：
必须通过 workflow_update 提交 node conclusion，
不能仅通过普通文本宣布节点完成。
```

Runtime 将该错误作为下一次 Prompt 的反馈，继续当前 Run：

```text
ModelStep::Complete
  ↓
Workflow Completion Gate
  ├─ Accept → Runtime 正常 finalize step
  └─ ContinueRequired
       ↓
    生成 Workflow protocol Tool Result
       ↓
    下一次 Prompt
       ↓
    LLM 补齐 workflow_update
```

Workflow 只能约束流程协议，不能保证 LLM 的结论真实。结论正确性仍需依赖 Tool 事实、用户确认或验证步骤。

## 4. Workflow Runtime View 与执行约束

Runtime 只消费 Workflow 提供的最小运行视图，不拥有 Workflow 的执行契约，也不解释 Workflow 的节点语义：

```text
WorkflowRuntimeView
  ├─ validity
  ├─ active_nodes
  └─ movement```

Runtime 只据此判断：

- 当前绑定的 Workflow 是否仍然有效；
- 当前 Workflow 位于哪个节点；
- 当前请求是否允许发生节点移动。

Runtime 不负责计算下一节点、解释节点语义、判断迁移条件或修改 Workflow。节点移动是否合法，由 Workflow Kernel 判断；LLM 通过 `workflow_update` 提交移动提议，Runtime 只按普通 Tool 流程路由并返回结果。

### 4.1 Tool Schema 与运行视图

当前节点可以返回可用 Tool 范围，Runtime 只负责将其转换为 Tool Schema 和执行前门禁：

```text
Workflow 返回当前节点可用 Tool
  ↓
Runtime 裁剪 Prompt 中的 Tool Schema
  ↓
LLM Tool Call
  ↓
Runtime 检查当前 Workflow 是否有效、是否允许当前节点移动
  ↓
Runtime Global Policy / Hook / Approval
  ↓
Tool 执行
```

有效工具范围仍然逐层收紧：

```text
effective_tools(node) =
    node.tool_policy
    ∩ runtime_available_tools
    ∩ global_policy_allowed_tools
```

Workflow 只能限制工具，不能放宽 Runtime 全局权限。Workflow 是否要求提交节点结论、是否允许迁移等业务规则，仍由 Workflow Kernel 判断，Runtime 不复制这些规则。

当前绑定的 Workflow 若已进入 `End.Completed` 或 `End.Cancelled`，Runtime 视为 Workflow 无效，不再为该绑定暴露 Workflow Tool 或 Workflow 节点工具；Run 本身仍按 Runtime 自身的生命周期继续收口。Session 级 Active Workflow 槽位只绑定 `Active` Workflow。

### 4.2 Model Invocation 的生效边界

Runtime 在每次 Model Invocation 前获取一次最新 `WorkflowRuntimeView`。已经发出的 Model Invocation 不会被中途改写；Workflow revision 更新后，只在下一次 Model Invocation / continuation Step 重新获取视图。

## 5. 默认 Prompt 与按需查询

Workflow 激活时提供稳定的默认 Prompt 提示：

```text
Workflow 已激活。

你正在一个受约束的工作流中执行。
开始行动前，可以使用 workflow_query 查询当前节点状态和已有节点结论。
只能使用当前节点暴露的工具。
如需记录节点结论、提交证据或调整工作流，必须使用 workflow_update。
不要仅通过普通文本宣布 Workflow 节点完成。
```

默认 Prompt 只表达稳定的行为协议，不注入完整 Workflow 状态。动态状态由 LLM 通过 `workflow_query` 按需读取：

```json
{
  "query": "node_conclusion",
  "node_id": "explore_repository"
}
```

返回只读结构化结果：

```json
{
  "node_id": "explore_repository",
  "status": "completed",
  "conclusion": "已确认项目使用 Rust workspace",
  "evidence_refs": ["run-step:123"],
  "workflow_revision": 7
}
```

## 6. Workflow Tool

### 6.1 `workflow_query`

只读查询 Tool，用于查询：

- 当前 Workflow 状态；
- 当前节点；
- 节点状态；
- 探索节点结论；
- 节点证据引用；
- 合法的后续流程选项；
- 当前 Workflow revision。

`workflow_query` 不修改 Workflow，不触发 Replan，不改变 revision。

### 6.2 `workflow_update`

修改 Tool，用于提交：

- 当前节点结果；
- 节点结论；
- 证据引用；
- 当前节点需要更多证据；
- Replan 提案；
- 合法的流程状态更新。

调用链：

```text
LLM workflow_update Tool Call
  ↓
Runtime ToolOrchestration
  ↓
Workflow Application
  ↓
Workflow Kernel
  ├─ 合法：提交 revision + 1
  └─ 非法：返回结构化 Tool Error
  ↓
Tool Result 回到下一次 Prompt
```

`LLM Tool Call` 不等于 `Workflow 已修改`。只有 Kernel 校验并提交后才产生新的 revision。

## 7. Runtime 与 Workflow 的交互边界

双方通过四个窄交互面协作，不共享内部聚合。

### 7.1 Runtime 获取最小运行视图

在每次 Model Invocation 前，Runtime 通过 Workflow Query Port 获取当前节点最小运行视图：

```text
Runtime
  ↓ WorkflowRuntimeViewQuery
Workflow
  ↓ WorkflowRuntimeView { validity, active_nodes, movement, tool_policies }
Runtime```

该查询面向 Runtime，不是 LLM 可见的 `workflow_query` Tool。两者用途不同：

```text
WorkflowRuntimeView：给 Runtime 当前有效性、活动节点、工具范围和移动许可workflow_query：让 LLM 按需查询 Workflow 状态、节点结论和证据
```

### 7.2 Runtime 把运行视图转换为 Prompt 与 Tool policy

Runtime 只把当前活动节点返回的 Tool policy 转换为 Prompt 中的 Tool Schema，并在执行前复核当前 Workflow 是否有效、是否允许当前节点移动：
```rust
let view = workflow_query_port
    .runtime_view(session_id, run_id, step_id)
    .await?;

let tool_schemas = tool_catalog
    .schemas()
    .filter_by_ids(&view.tool_policies.visible_tool_ids());
// Workflow 激活提示经 Reminder 管线的 source 注册注入（SystemTail 类），
// Runtime 不直接拼 Provider-visible 内容。
let context_request = ContextRequest {
    raw_tool_schemas: tool_schemas,
    ..base_context_request
};

let model_step = model_invocation
    .invoke_model(context_window)
    .await?;
```

Runtime 仍然只通过 Prompt 调用 LLM，不复制 Workflow 的节点迁移或完成规则。

### 7.3 Runtime 路由 Workflow Tool

Workflow Tool 是 Tool Catalog 中的一类 Tool：

```text
Runtime ToolOrchestration
  ├─ Read / Glob / Grep / Edit / Write / Bash
  ├─ AskUser
  └─ Workflow Tool
       ├─ workflow_create
       ├─ workflow_query
       └─ workflow_update
```

Runtime 负责通用的解析、Policy、Hook、Interaction、执行和 Tool Result 回传；Workflow 负责 Tool 的业务校验和状态提交。

### 7.4 Runtime 发布执行事实

RunStep 或 Run 完成后，Runtime 发布稳定事实给 Workflow：

```text
Runtime
  └─ finalized Step / Run facts
       ├─ Run / Step identity
       ├─ Tool receipts
       ├─ User interaction facts
       ├─ output references
       └─ terminal fact
              ↓
       Workflow Facts Adapter
              ↓
       Workflow 记录节点执行事实
```

Runtime 事实不自动等于 Workflow 节点结论。比如：

```text
Runtime 事实：cargo test 退出码为 1
Workflow 结论：验证失败，下一步需要修复 xxx
```

后者必须由 `workflow_update` 提交，或由明确的 Workflow 确定性规则生成。

## 8. 非简单场景链路

### 8.1 创建 Workflow 的首个 Run

```text
用户输入复杂任务
  ↓
Runtime 创建普通 Run
  ↓
Runtime 构造 Prompt，并暴露 workflow_create
  ↓
Runtime 通过 Prompt 调用 LLM
  ↓
LLM 调用 workflow_create
  ↓
Workflow Kernel 创建 Workflow
  ↓
activation receipt 作为 Tool Result 回到 Prompt
  ↓
Runtime 重新查询当前节点 Execution Contract
  ↓
Runtime 裁剪 Tool Schema，加入默认 Prompt
  ↓
LLM 通过 workflow_query 查询状态或探索节点结论
  ↓
LLM 执行当前节点允许的普通 Tool
  ↓
LLM 通过 workflow_update 提交节点结论 / 结果 / Replan
  ↓
Workflow Kernel 校验并提交 revision
  ↓
Tool Result 回到下一次 Prompt
  ↓
LLM 继续当前 Run
  ↓
Runtime 完成 Run
  ↓
Workflow 接收 finalized facts
  ↓
等待用户决定是否启动下一次 Run
```

### 8.2 后续 Run

```text
用户决定继续
  ↓
Runtime 创建新的 Run
  ↓
Run 开始前查询最新 Workflow Execution Contract
  ↓
Runtime 形成 Prompt 与有效 Tool 集合
  ↓
LLM 通过 workflow_query 查询当前状态和探索节点结论
  ↓
LLM 执行当前节点允许的工作
  ↓
LLM 通过 workflow_update 提交节点结果或 Replan
  ↓
Runtime 完成 Run
  ↓
Workflow 记录 finalized facts
  ↓
再次等待用户决定
```

`Run 1` 完成后不会由 Workflow 自动创建 `Run 2`。Run 与 Run 之间的继续权属于用户；后台自动继续必须是另行授权的 Runtime 后台运行能力。

## 9. Runtime Goal 与 Workflow Graph 的关系

`Goal` 属于 Runtime 的长任务能力；Workflow 是 Session 级独立能力。Workflow 内部的 Graph 负责描述节点、边、状态和失败路由，不是 Runtime Goal 的子对象。

```text
Session
├── Active Workflow（0..1）
│   └── Graph
│       ├── Node
│       ├── Edge
│       ├── State
│       └── Failure Routing
├── Runtime Goal（0..n，可选）
│   └── 多次 Runtime Run
│       └── RunStep / Prompt / LLM / Tool / Interaction
└── Standalone Runtime Run（可无 Goal）
```

Session 中最多存在一个 Active Workflow。Workflow 可以跨多个 Goal 和 Run 持续存在；Goal 可以不存在，Run 也可以不依附 Goal。Run 是否使用当前 Workflow，由 Run 创建时记录的可选 Workflow 快照决定。

```text
Workflow Graph
  └─ Session 级独立聚合
       ├─ 节点定义与基础角色
       ├─ 条件边与失败路由
       ├─ 当前节点状态与节点结果
       └─ LLM 行为约束

Runtime Goal
  └─ Runtime 长任务生命周期与跨 Run 继续策略
```

`Loop` 不是 Runtime 类型，也不是 Graph 内的特殊节点。Loop Graph 只是允许 Graph 存在回边的普通 Graph；单节点自环、多节点重试、条件回退和周期重复都使用同一套 Node / Edge / State 模型。

### 9.1 Session 级 Active Workflow

Workflow 是 Session 级独立聚合，不属于任何 Runtime Goal。每个 Session 同时最多存在一个 Active Workflow：

```text
Session
├── active_workflow: Option<WorkflowId>   // 0..1
├── Runtime Goal（0..n，可选）
│   └── Runtime Run
└── Standalone Runtime Run（可无 Goal）
```

Active Workflow 可以跨多个 Goal 和 Run 持续存在。Goal 创建、完成或终止不会自动创建、销毁或切换 Workflow；Workflow 的结束只通过 `End.Completed` 或 `End.Cancelled` 表示。

Run 在创建时可以选择绑定当前 Active Workflow，也可以不绑定 Workflow。绑定只保存快照事实，不把 Workflow 嵌入 Goal：

```text
WorkflowBindingSnapshot
  ├─ workflow_id
  ├─ workflow_revision
  └─ workflow_node_path
```
Session 的 Active Workflow 后续发生 revision 更新，不会回写已经开始的 Model Invocation。下一次 Run 或 continuation Step 重新获取最新的 Runtime View。

### 9.2 Goal：跨 Run 的长任务聚合根

Runtime Goal 定义“要持续完成什么”，是多个 Run 的归属边界：

```text
RuntimeGoal
  ├─ goal_id
  ├─ objective
  ├─ lifecycle
  ├─ execution_mode: Foreground | Background
  ├─ continuation_authorization
  └─ run_history_refs
```

Goal 负责：

- 保存跨 Run 的长期任务身份和目标描述；
- 记录前台 / 后台执行模式；
- 记录用户是否授权后台自动继续；
- 接收 Run 终态、取消、失败和暂停等 Runtime 事实；- 管理 Goal 自身的 Active、AwaitingUser、Sleeping、Completed、Aborted 生命周期。
Goal 不直接执行 LLM、Tool 或 RunLoop，也不解释 Workflow 节点结论。

### 9.3 Runtime Goal 的继续策略

Runtime Goal 的继续策略负责回答：

- 前台是否应等待用户；
- 后台是否已获得自动唤醒授权；
- 是否达到时间或资源预算；
- 没有 Active Run 时是否允许 Runtime 创建新的后台 Run。

它不负责描述 Workflow 的 Graph 拓扑，也不负责决定节点之间的迁移：

```text
Runtime Goal
  └─ continuation_policy
       ├─ AwaitingUser
       ├─ AuthorizedForBackgroundWakeup
       ├─ BlockedByBudget
       ├─ Completed
       └─ Aborted
```

### 9.4 Goal 与 Runtime Run 的不变量

  1. Runtime Goal 可以没有任何 Run，也可以关联多个顺序 Run。
  2. Goal 进入 `Completed` 或 `Aborted` 后，不能再创建属于它的新 Run。
  3. Goal 的继续策略不能改变 Workflow Graph 的 Definition、Node / Edge / State 或 Failure Routing。
  4. Workflow Graph 的回边、失败路由和周期性重复不属于 Runtime Goal 的继续策略。
  5. 一个没有 Goal 的 Standalone Run 仍然可以绑定当前 Session Workflow。

### 9.5 Run 与 Goal 的关联

Run 仍然是 Runtime 的一次执行生命周期，不是 Goal 的状态机。Run 可以独立存在，也可以在创建时关联 Goal。Run 只携带创建时的关联事实快照：

```text
RunBinding
  ├─ goal_id: Option<GoalId>
  ├─ workflow_id: Option<WorkflowId>
  ├─ workflow_revision: Option<WorkflowRevision>
  └─ workflow_node_path: Option<NodePath>
```
典型关系：

```text
Workflow Graph：修改项目并通过测试
├── Run 1：分析项目
├── Run 2：修改代码
├── Run 3：运行验证
└── Run 4：继续修复
```每个 Run 内部仍由 Runtime 独立完成：

```text
Run
  ├─ Context / Prompt
  ├─ LLM Invocation
  ├─ Tool Round
  ├─ Interaction
  ├─ 多个 RunStep
  └─ Run Terminal
```

Workflow 节点结果可以作为关联事实被记录，但不能直接改变 Goal 生命周期：

```text
Workflow：验证节点失败
  ↓
Runtime 记录本次 Run 失败事实
  ↓
Runtime Goal 的 continuation_policy 决定 AwaitingUser / Sleeping / Completed / Aborted
  ↓
这类失败事实不会改变 Workflow Graph 的生命周期；Graph 保留失败节点状态，并按 Failure Routing 等待下一次允许的节点执行。
```

### 9.6 Standalone Run 与 Workflow 绑定

没有 Goal 的 Run 是合法路径：

```text
Session
└── Standalone Run
    ├─ goal_id = None
    └─ workflow_binding = None | 当前 Session Workflow 快照
```
因此，Workflow 与 Goal 没有从属关系。一个没有 Goal 的 Run 仍可以在创建时绑定当前 Session 的 Active Workflow；同样，一个有 Goal 的 Run 也可以不绑定 Workflow。

Run 创建时只捕获当时的 Workflow 绑定事实：

```text
WorkflowBindingSnapshot
  ├─ workflow_id
  ├─ workflow_revision
  └─ workflow_node_path
```

Session 的 Active Workflow 后续发生 revision 更新，不会回写已经开始的 Model Invocation。下一次 Run 或 continuation Step 重新获取新的 Execution Contract。

### 9.7 Run 之间的继续决策

前台默认由用户决定是否创建下一次 Run：

```text
Run 1 完成
  ↓
Runtime Goal 更新 Runtime 状态  ↓
AwaitingUser
  ↓
用户选择：
  ├─ Continue → Runtime 创建 Run 2
  ├─ Replan → Workflow 更新后，仍等待用户决定是否 Run
  ├─ Pause
  └─ Abort
```

Workflow 不调用 `create_run`，也不通过 `next_request` 自动推进前台任务。

### 9.8 后台任务完成与唤醒

后台任务的“唤醒”不等于创建新的 Runtime Run。后台任务完成后，Runtime 先产生完成事实，再由通知路由决定如何让 LLM 获知该事实。

```text
后台任务执行完成
  ↓
Runtime 记录 BackgroundTaskCompleted fact
  ↓
Runtime Notification Router
  ├─ 存在可接收通知的 Active Run
  │    ↓
  │  排队 InvocationReminder
  │    ↓
  │  下一次 Model Invocation 的 Prompt 携带 reminder
  │    ↓
  │  LLM 通过 Prompt 获知后台结果
  │
  └─ 没有可接收通知的 Active Run
       ↓
     保存 PendingRuntimeNotice
       ↓
     下次用户启动 Run 时注入 Prompt
```

Runtime 不打断已经发出的 Model Invocation。若后台任务在模型调用进行期间完成，通知只在下一个安全的 Prompt / Model Invocation 边界生效。

如果 Active Run 处于 `AwaitingUser`，Runtime 不应偷偷启动新的模型调用；完成事实先作为 pending notice 保留，待该 Run 被用户输入恢复后，再以 reminder 形式进入 Prompt。

### 9.9 后台任务完成通知与 Workflow

后台任务完成后，Runtime 可以将完成事实发布给绑定的 Workflow，使 Workflow 能在 `workflow_query` 中提供任务结论或证据引用：

```text
BackgroundTaskCompleted
  ├─ task_id
  ├─ goal_id: Option<GoalId>
  ├─ status
  ├─ output_refs
  └─ completed_at
       ↓
Runtime facts boundary
  ├─ Active Run → InvocationReminder
  └─ Workflow → 可查询的节点事实 / 证据引用
```

该事实不会自动完成 Workflow 节点，也不会自动改变 Goal：

```text
后台任务完成
  ↓
Reminder 告知当前 LLM
  ↓
LLM 按 Prompt 决定是否 workflow_query
  ↓
LLM 通过 workflow_update 提交节点结论或 Replan
  ↓
Workflow Kernel 校验并更新 revision
```

### 9.10 新建 Run 的授权分两条独立通道

后台任务完成后是否创建新 Run，按任务种类走两条**互不混同**的授权通道：

- **Goal / Loop 级长任务的后台继续**：由 Runtime Goal 的
  `continuation_authorization` 与后台策略管辖，未授权时仅保存
  `PendingRuntimeNotice`、等待用户启动下一次 Run。
- **tool call 后台任务（后台任务模型）**：无 Active Run 时 Runtime 创建
  **Wakeup Run** 回注结果——授权来源是 **agent 发起转后台这个动作本身**
  （转后台即视为该任务的唤醒授权），不依赖 `continuation_authorization`，
  也不经 Workflow 决定。用户可用 Esc 标准取消 Wakeup Run。

因此，`Wakeup` 表示 Runtime 的后台状态变化或通知事件，不是固定的 Run 类型：

```text
Wakeup outcome（tool call 后台任务）
  ├─ ReminderQueued(active_run_id)
  └─ WakeupRunStarted（授权来源：agent 发起转后台）
Wakeup outcome（Goal / Loop 级后台继续）
  ├─ ReminderQueued(active_run_id)
  ├─ PendingNoticeStored
  ├─ UserInputRequired
  └─ BackgroundRunStarted（仅 continuation_authorization 授权时）
```

Runtime 后台状态至少包括：

```text
BackgroundTaskState
  ├─ Sleeping
  ├─ WakeDue
  ├─ Running
  ├─ Completed
  ├─ NotificationQueued
  ├─ AwaitingUser
  ├─ Failed
  └─ Cancelled
```

Workflow 不负责计时、唤醒、创建 Run 或决定通知如何投递；但后台任务完成事实进入 LLM Prompt 后，LLM 仍必须服从当前 Workflow 节点的 Execution Contract。

## 11. Workflow 与后台通知的交互

Runtime 使用 Reminder 统一管线（`ReminderSource` + per-kind policy；真相源
`docs/design/02-modules/context-management/07-reminder-pipeline.md`）把后台完成
事实送入 LLM Prompt。该机制是 Runtime 的 Prompt 传递能力，不是 Workflow 直接
调用 LLM。tool call 后台任务的完成事实注册为 `BackgroundTaskEvent` kind
（`OnEvent(task_terminal)` 触发、`TailUserMessage` 注入并显式落盘 canonical）。

```text
Runtime Notification Router
  ↓
ReminderSource(BackgroundTaskEvent) + OnEvent(task_terminal)
  ↓
Reminder Pipeline（per-kind policy 注入决策）
  ↓
Context Window
  ↓
Prompt
  ↓
LLM
```

概念伪代码：

```rust
async fn on_background_task_completed(
    &self,
    completion: BackgroundTaskCompleted,
) -> BackgroundNotificationOutcome {
    let reminder = InvocationReminderData::background_task_completed(
        completion.task_id.clone(),
        completion.status,
        completion.output_refs.clone(),
    );

    if let Some(active_run) = self.active_run_for(completion.session_id) {
        // Reminder 统一管线：source 经 OnEvent(task_terminal) 注入下一 invocation
        self.context.reminder_handle_event(active_run.run_id(), "background_task");
        return BackgroundNotificationOutcome::ReminderQueued {
            run_id: active_run.run_id().clone(),
        };
    }

    // 无 active Run：Wakeup Run 回注（授权来源 = agent 发起转后台）
    self.wakeup_mailbox.send(completion.task_ids()).await;
    BackgroundNotificationOutcome::WakeupRunStarted
}
```

reminder 的注入决策由 Context 管线独占（per-kind policy：refresh / placement /
dedup / 预算），Runtime 只注册 source 并触发事件，**NEVER** 在窗口生成后追加
Provider-visible 内容。TailUserMessage 类 reminder 随 step 收口显式落盘
canonical。LLM 收到通知后，可以：

```text
<system-reminder>
后台任务 task-123 已完成，结果引用 artifact-456。
如需了解其对当前 Workflow 的影响，请调用 workflow_query。
</system-reminder>
```

Reminder 只传递 Runtime 事实和查询提示，不替 Workflow 生成结论，不直接修改 Workflow，也不强制 LLM 进入新的 Run。

Workflow 不负责计时、唤醒、创建 Run 或决定后台通知是否打断当前模型调用。后台完成事件通过 Active Run reminder、Pending Runtime Notice 或经授权的后台 Run 三种方式之一进入 Runtime 流程。

## 12. 当前确认的不变量

1. Runtime 只能通过 Prompt 调用 LLM。
2. Workflow 由 LLM 通过 `workflow_create` 创建。
3. LLM 只能通过 Workflow Tool 查询和修改 Workflow。
4. Workflow 不直接调用 LLM、Tool 或 Runtime。
5. Workflow 节点决定当前 Run 可见和可执行的 Tool 范围，但不能放宽 Runtime 全局安全策略。
6. Workflow 激活时必须提供默认 Prompt 提示。
7. 动态 Workflow 状态和探索节点结论由 LLM 通过 `workflow_query` 按需查询。
8. `workflow_update` 必须经过 Workflow Kernel 校验并产生 revision。
9. LLM 未完成节点要求的 Workflow 协议时，Workflow Kernel 返回协议错误；Runtime 只将其作为下一次 Prompt 的 Tool / continuation 反馈，不复制 Workflow 的完成判断。
10. Workflow 更新只在下一次 Model Invocation / continuation Step 生效。
11. Workflow 只有 `Active` 和 `End.Completed` / `End.Cancelled`；Run 等待、后台睡眠和 Runtime Goal 状态不属于 Workflow 生命周期。
12. Workflow 不自动创建下一个 Run；前台 Run 之间由用户决定是否继续。
13. Runtime Goal 是跨 Run 的长任务聚合根；Workflow Graph 的回边、失败路由和周期性重复属于 Workflow Graph 的 Node / Edge / State，而不是 Runtime Goal 的 Loop。
14. Goal 的继续策略管理跨 Run 生命周期；Workflow 是 Session 级独立聚合，管理 Graph 节点状态、节点结果和 LLM 行为约束。
15. Session 同时最多存在一个 Active Workflow；Workflow 不从属于 Goal，也不因 Goal 生命周期自动创建、销毁或切换。
16. Run 可以没有 Goal，也可以独立绑定当前 Session Workflow 的快照；Goal 与 Workflow 绑定均为可选关联。
17. Workflow 不自动创建下一个 Run；前台 Run 之间由用户决定是否继续，后台自动继续必须有 Runtime 侧授权。
18. Workflow 不负责计时、唤醒或创建 Run；后台完成事件优先通过 Active Run 的 Reminder 进入 Prompt；tool call 后台任务在无 Active Run 时经 Wakeup Run 回注（授权 = agent 发起转后台），Goal / Loop 级后台继续创建 Run 仅在 `continuation_authorization` 授权时发生。
19. Runtime finalized facts、Workflow 节点结论、Runtime Goal 生命周期事实是三类不同事实，不能自动混同。
20. tool call 后台化唤醒与 `continuation_authorization` 是两条独立授权通道：前者授权来自 agent 发起转后台动作本身，后者管辖 Goal / Loop 级长任务的后台继续，两者不得混同。
## 13. 非目标

- 不把 Workflow 变成第二个 Runtime；
- 不让 Workflow 直接调用 LLM；
- 不让 Workflow 自动调度前台 Run；
- 不用 Prompt 单独承担 Workflow 约束；
- 不把 Workflow 当成只记录状态的弱 Task；
- 不让 Workflow 放宽 Runtime Policy、Hook 或用户审批；
- 不把 Runtime Goal 或 Background Task 的生命周期塞进 Workflow；
- 不把 Session 级 Active Workflow 错建模为 Goal 的子对象；
- 不把无 Goal 的 Standalone Run 排除在模型之外；
- 不把后台唤醒建模为 Workflow 的 LLM 角色或节点。
