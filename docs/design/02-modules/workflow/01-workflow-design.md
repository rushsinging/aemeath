# Workflow · 单一设计真相源

> 状态：Target（MVP 已实现，开关默认关闭）｜Milestone：v0.2.0｜对应 Issue：[#555](https://github.com/rushsinging/aemeath/issues/555)
> 层级：02-modules / workflow（模块战术设计）
>
> 本文是 Workflow Graph 的**唯一设计真相源**。实现见 `agent/features/workflow/**` 与
> `agent/features/tools/src/adapters/workflow_tool.rs`。尚未拍板的目标机制集中列于
> 《11. POST-MVP 移出清单》，正文不假装其已定。

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

Runtime **只能通过 Prompt 调动 LLM**。Runtime 不直接调用 Workflow 内部的 LLM，也不把
Workflow 聚合对象直接传给 LLM。

Runtime 不负责：

- 理解完整 Workflow 业务语义；
- 判断用户整体目标是否完成；
- 自动替用户决定 Run 与 Run 之间是否继续。

Runtime 原则上不直接修改 Workflow Definition、Node State 或 Workflow State；#2160 Q1
职责线重写后仅开放两个固定写入口，其余维持禁止：

1. **默认建图**——`workflow.enabled=true` 且符合 D1 条件时，由 Runtime 创建
   `plan→done` 最小图（§15.2）；
2. **agent 结论草稿写入**——agent 完成时把结论写入服务节点的草稿位（§15.4）。

节点完成 / 失败 / Replan / 终态提交等业务语义仍只能由 LLM 经六个 Tool 提交。

Runtime 为组织上下文而读取当前 Active Workflow 的 `workflow_id` 与 `ActiveMode`，这是内部
装配职责，不等于执行 Workflow 语义操作。

### 1.2 LLM

LLM 是 Runtime 通过 Prompt 调用的模型能力。LLM 不拥有 Workflow 状态，也不直接修改
Workflow。

LLM 可以：

- 通过普通 Tool 执行实际工作；
- 通过六个 `workflow_*` Tool 创建、查询、推进、重规划、结束 Workflow。

LLM 不负责：

- 直接写 Workflow 存储；
- 绕过 Runtime 的 ToolOrchestration；
- 实现 Runtime 的 Tool Policy、Hook 或用户审批。

### 1.3 Workflow

Workflow 是**支撑域 BC**，Session 级 Graph orchestration 的拥有者：

- 拥有 Graph Definition / Node / Edge / State；
- 拥有 Node 状态机、聚合生命周期状态机与结构不变量校验（Kernel）；
- 拥有 continuation subgraph 的校验与提交；
- **不**创建、调度、暂停或终止 Run；
- **不**调用 LLM、不执行 Tool；
- **不**负责计时、后台唤醒或通知路由。

Workflow 与 Runtime 只通过两条缝协作：六个 `workflow_*` Tool（LLM 经 Runtime 调用）与
只读状态查询（Runtime 装配提示用）。

## 2. 创建与生命周期

### 2.1 创建

```text
LLM 调用 workflow_create
  ↓
Runtime ToolOrchestration（Policy / Hook / 审批 / 复验）
  ↓
Workflow Application → Kernel 校验完整 DAG
  ↓
创建为 Active(Executing)，root 成为唯一初始 Active Node
  ↓
Tool Result 回到下一次 Prompt
```

`workflow_create` 一次提交完整 Graph 提案，不是先创建空容器。核心语义：

```text
CreateWorkflowSpec
  ├─ name
  ├─ objective
  ├─ root_node_id
  ├─ nodes: [NodeDefinition]
  └─ edges: [EdgeDefinition]
```

创建成功时：

- root 是唯一无前驱节点，且必须是普通业务 Node；
- root 成为唯一初始 Active Node，其余 Node 初始 Pending；
- Workflow 进入 `Active(Executing)`；
- 若 Session 已存在 Active Workflow，创建被拒绝并返回 `ActiveWorkflowAlreadyExists`；
  LLM 必须先显式 `workflow_update(deactivate)`；
- 创建失败不留下半成品。

**Runtime 默认建图（D1，#2160）**：`workflow.enabled=true` 时，session 开始 / resume /
clear 后若无 Active Workflow 且无 Inactive 候选，Runtime 经上述固定入口直接创建
`plan→done` 最小图（root 为 Planner 角色的 WorkNode，唯一后继为 CompleteNode），不经
LLM 提交。存在 Inactive 候选时不默认建图，进入 Selecting（§2.2）。非对话 Run（sub run、
后台 wakeup、RSI 等）不触发默认建图。

MVP 不在领域层设置固定节点数、Edge 数或描述长度上限；通用请求体大小、解析、内存与
系统资源保护仍然生效。

### 2.2 WorkflowStatus 与 ActiveMode

WorkflowStatus 是五个并列状态，不使用 Open / Closed 两层包装：

```text
WorkflowStatus
  ├─ Active
  ├─ Inactive
  ├─ Completed
  ├─ Cancelled
  └─ Failed
```

`Replanning` 不是 Node，也不是第六个 WorkflowStatus；它与 `Executing` 一起构成 Active
Workflow 的模式：

```text
ActiveMode
  ├─ Executing
  └─ Replanning
```

合法迁移：

```text
Active(Executing)    ──node fail────▶ Active(Replanning)
Active(Replanning)   ──replan ok────▶ Active(Executing)
Active(Executing)    ──deactivate───▶ Inactive(pending_mode=None)
Active(Replanning)   ──deactivate───▶ Inactive(pending_mode=Replanning)
Inactive(None)       ──activate────▶ Active(Executing)
Inactive(Replanning) ──activate────▶ Active(Replanning)
Active(Executing)    ──complete────▶ Completed
Active(Executing)    ──cancel──────▶ Cancelled
Active(Replanning)   ──cancel──────▶ Cancelled
Inactive             ──cancel──────▶ Cancelled
Active(Replanning)   ──fail────────▶ Failed
```

`Active(Replanning)` 允许 `deactivate` 与 `cancel`，拒绝 `complete`。

`deactivate` 是通用生命周期操作，不专属于任何模式：用户临时挂起 Workflow（例如被其他
事务打断）时使用。它**不清除** ActiveMode——从 `Active(Replanning)` 进入 Inactive 时保留
待恢复模式 `Replanning`，再次 `activate` 回到 `Active(Replanning)`，而不是
`Active(Executing)`。理由是 Graph 中存在 Failed Node 且尚未提交 continuation 时，直接回到
Executing 会让旧 DAG 带着失败路径继续推进，违反已冻结不变量。

因此 Inactive 携带一个 `pending_mode`：

```text
Inactive(pending_mode = None | Replanning)
```

状态与模式语义：

- `Active(Executing)`：Session 同一时刻最多一个；按 Active Node 推进当前 DAG；
- `Active(Replanning)`：任一 Active Node 被 LLM 显式标记 Failed 后立即进入；其他并行
  Active Node 原子转为 Stopped，旧 Graph 不再推进；Runtime 执行可以跨零到多个 Node 或
  服务整个 Workflow，不要求在 Replan 前全部停止；
- `Inactive`：执行屏障，不改变领域状态；不注入任何提示；activate 后按 `pending_mode` 恢复；
- `Completed` / `Cancelled` / `Failed`：不可逆终态，不能重新激活，不再接受节点更新或 Replan。

create / activate 不自动替换当前 Active Workflow。deactivate 不结束当前 Run，也不自动创建
新 Run。Workflow 不自动进入 Completed 或 Failed：CompleteNode 完成后仍须 LLM 显式
`workflow_update(complete)`（最小图未分裂时例外，见下方 D2 原子收口）；Replanning 判定
无法继续时仍须 LLM 显式 `workflow_update(fail)`。

`Suspended`、`Sleeping`、`AwaitingUser` 不是 WorkflowStatus 或 ActiveMode，分别属于 Runtime
Run、Runtime Goal 或后台任务能力。

**Selecting（D1 配套，#2160 Q3）**：全程激活要求任何时刻存在 Active Workflow。无 Active
且存在 Inactive 候选时，系统进入 **Session 级协调状态** `SessionWorkflowMode = Executing |
Selecting`（存 session 元数据，不是第六个 WorkflowStatus）：Selecting 下工具分发层只放行
`workflow_create / workflow_update(activate) / workflow_list / workflow_query`，其余工具一律
typed error（错误文本带候选列表）。activate / create 成功后回到 Executing。这是与
Replanning 同构的状态硬约束，不是提示约束。无退出通道：用户说「先不做计划」也只能建
最小图走 D2 简单出口。sub run 有独立 SessionState，不受该模式约束。

**简单出口（D2，#2160）**：图自创建后未分裂、仍为 `plan→done` 最小结构时，
`workflow_node_update(complete)` 完成 plan 节点即**原子完成整个 Workflow**（同一领域提交内
完成 CompleteNode 与 Workflow，无需第二次显式 `workflow_update(complete)`），一问一答场景
固定开销为一次工具调用。图一旦分裂过，回到常规显式提交流程。

### 2.3 Session reset（`/clear`）

`/clear` 是 Session reset，**只处理 Session 可见性，不删除 Workflow 实体**：

- Session 内只保存 Workflow 索引引用（`workflow_id` 集合）与 active 指针，Workflow 实体
  独立存储；
- `/clear` 清空对话上下文，并同步移除该 Session 的全部 Workflow 引用与 active 指针；
- 之后 `workflow_list` 返回空，不再显示 `/clear` 之前的 Workflow；
- Workflow 实体仍然留在磁盘，成为无引用数据，不由 `/clear` 负责擦除；
- 已失去引用的 Workflow 拒绝任何按 `workflow_id` 的直接查询与操作，返回
  `WorkflowNotVisible`，不提供隐式复活路径。**例外（#2160 跨 workflow 检索）**：
  Completed / Cancelled 终态实体放开只读检索——`workflow_list` 可按 status 过滤列出、
  `workflow_query` 可按 id 读取结论与证据引用（§15.6），供后续 workflow 回查历史；
  终态不可写、不可复活的状态机约束不变；
- 落盘内容的物理清理由统一的清理能力负责（见 §2.4），不新增 `workflow_clear` Tool，也不
  把清理做成用户直接操作 Workflow 的语义。

### 2.4 落盘与统一清理

```text
~/.agents/workflows/
├── index/{session_id}.json      workflow_id 集合 + active 指针
└── entities/{workflow_id}.json  聚合快照
```

- 写入一律 temp + rename 原子替换；
- 索引引用缺失实体、实体 JSON 损坏 → fail-closed（`PersistenceError::Corrupt`），**NEVER**
  静默重置；
- resume 按索引加载全部实体，恢复 `WorkflowStatus`、`ActiveMode`、`pending_mode` 与全部
  Node 状态 / 结论 / 证据 / 出口。

统一清理（`aemeath cleanup`）回收无引用实体：

- 汇总全部 Session 索引引用（含 active 指针），只删除无引用实体；
- `--dry-run` 只列出、`--yes` 跳过确认；默认先预览再交互确认，不可恢复；
- **MUST** 幂等：重复执行不再删除；
- 索引损坏时 **MUST** 拒绝清理任何实体（fail-closed）；
- 清理是用户显式运维动作，**NEVER** 暴露为 `workflow_*` Tool。

## 3. Graph 领域模型

### 3.1 Node 类型与状态

MVP 有三类 Node：

```text
NodeKind
  ├─ WorkNode       // 普通业务工作节点，root 也属于此类
  ├─ ConfirmNode    // 通用确认语义节点
  └─ CompleteNode   // 唯一正常拓扑出口
```

所有 Node 使用统一状态：

```text
NodeStatus
  ├─ Pending
  ├─ Active
  ├─ Completed
  ├─ Failed
  └─ Stopped
```

约束：

- root 是普通业务 Node，不是虚拟结构节点；
- CompleteNode 是显式特殊 Node 类型，不通过「唯一无后继普通节点」隐式识别；它在 Workflow
  创建时确定，生命周期内保持唯一且身份稳定；
- CompleteNode 只汇总最终结论、关键证据和交付结果，不承担用户确认，也不执行可能失败的
  业务工作；所有可能失败的验收、测试或交付工作必须位于它的前驱 Node；
- Replan 不创建、替换或修改 CompleteNode，也不能让 continuation subgraph 直接连接
  CompleteNode；
- ConfirmNode 可出现在 Graph 任意合法位置，是否影响最终完成只由拓扑关系表达；
- `Completed` / `Failed` / `Stopped` 是 Node 终态，禁止恢复为 Active 或改成另一终态；
- 只有 LLM 可通过 `workflow_node_update(fail)` 将 Active Node 显式标记为 Failed；
  Runtime、Tool 或后台任务失败只形成事实，不自动映射为 Node Failed；
- 任一 Active Node 进入 Failed 后，Workflow 立即从 `Active(Executing)` 切换为
  `Active(Replanning)`，所有其他并行 Active Node 在同一领域提交中进入 Stopped；Stopped
  表示旧计划下的执行资格已撤销，不是成功或业务失败，不满足 AND-join，也不自动激活旧后继；
- Workflow 只提交 Node 状态变化，不直接停止 Run。Runtime Run、Sub-agent、Tool Call 或后台
  任务可以跨零到多个 Node，也可以服务整个 Workflow；Node 进入 Stopped 不构成机械取消这些
  执行的命令；
- 旧执行在 Replan 前后返回的结果、错误和副作用信息只形成 facts。它们不能让 Stopped Node
  自动转为 Completed / Failed，不能触发旧出口，不能激活旧后继，也不能自动改写已提交的
  新 Graph。

### 3.2 Kernel 结构不变量

确定性 Kernel 在创建与 Replan 时校验。错误统一为
`GraphInvariantViolation { reason, subject }`，`reason` 取值：

```text
MissingNode               引用指向未定义 Node
MultipleRoots             无前驱 Node 不唯一，或与 root_node_id 不一致
RootIsSpecialKind         root 不是 WorkNode
MissingCompleteNode       没有 CompleteNode
MultipleCompleteNodes     多于一个 CompleteNode
CompleteNodeHasSuccessor  CompleteNode 有出边
NodeWithoutSuccessor      非 CompleteNode 无出边
UnreachableNode           从 root 不可达
NoPathToCompleteNode      不存在到 CompleteNode 的有向路径
CycleDetected             存在环
SelfLoop                  自环
RoleOnSpecialKind         Confirm / Complete 携带 role
WorkNodeMissingRole       WorkNode 缺 role
```

创建时全量校验（固定优先级，测试依赖该顺序断言具体 reason）：

1. Edge 端点与 root 引用存在；
2. 无自环；
3. 无环（三色 DFS）；
4. 唯一 CompleteNode；
5. CompleteNode 无后继；
6. 唯一 root；
7. root 是 WorkNode；
8. Role 约束（WorkNode 必须有 Role，Confirm / Complete 禁止 Role）；
9. 从 root 可达全部 Node（BFS）；
10. 非 CompleteNode 至少一个后继；
11. 非 CompleteNode 存在到 CompleteNode 的路径（逆向 BFS）。

### 3.3 Edge、出口决定与节点激活

Graph 只保存静态 Edge；Edge 不包含由 Kernel 解释的业务条件。LLM 完成 Active Node 时，通过
`workflow_node_update` 同时提交结果、证据引用和一个或多个出口决定：

```text
SuccessorDecision
  ├─ node_id      必须是当前 Node 的静态 Edge 直接指向的后继
  └─ description  为何进入该后继、后继要继续什么（必填）
```

规则：

- `node_id` 必须是**直接后继**，不是「可达后继」；
- 即使只有一个后继，也必须由 LLM 显式提交；
- 任一非 CompleteNode 可选择多个直接后继，从而形成并行 Active Node；
- CompleteNode 无后继，因此完成时不提交出口决定；
- `description` 必填，Kernel 只校验结构，不解释其业务语义；
- 同一提交内重复的 `node_id` 被拒绝。

### 3.4 隐式 AND-join

多前驱 Node（含 WorkNode、ConfirmNode、CompleteNode）使用隐式 AND-join：

- 目标的全部**有效**直接前驱（静态边中非 Failed / Stopped 的前驱）都 Completed 且出口
  决定均选择该目标时，**只激活一次**；
- 部分前驱完成时目标保持 Pending；
- Failed / Stopped 前驱被**忽略**——既不消耗 join，也不阻塞激活。这样 Replan 后被冻结
  前驱包围的 rejoin target 仍可由 continuation 节点激活；
- 有效前驱集为空时不激活。

### 3.5 Node Role

Node Role 是 Workflow-owned 的系统预定义封闭集合，恰好包含：

```text
Planner / Explorer / Architect / Implementer / Verifier / Reviewer
```

Confirm / Complete 由 NodeKind 表达，不设置 Confirmer / Completer。Node 只引用 Role，不直接
声明 `allowed_tools`，也不支持在 Role 之上按 Node 增删 Tool。

> **MVP 状态：Node Role 对普通 Tool 的裁剪整体不启用。**
>
> MVP 期间 Role 只是语义标签：不发布 Role capability Profile、不计算并行 Active Node 的
> 能力并集、不投影普通 Tool Schema、不装配 Replanning Profile。普通 Tool 可见性与授权完全
> 由现有 Run 级 Tool Profile、Policy、Hook 与用户审批决定；六个 `workflow_*` Tool 始终可用
> 且不参与裁剪。
>
> 具体映射与装配契约见 §11 POST-MVP 清单。

### 3.6 Replanning、Replan 与不可变历史

Replanning 是 Active Workflow 的模式，不是 Node，也不是并列 WorkflowStatus。

触发与全局停止：

```text
任一 Active Node 提交 fail
  ↓
该 Node → Failed（结论与证据冻结）
  ↓
Workflow 立即 Active(Executing) → Active(Replanning)
  ↓
其他并行 Active Node 原子转为 Stopped
  ↓
旧 DAG 不再推进；Runtime 执行可继续存活
```

`workflow_node_update(fail)` 的 Tool Result 必须即时返回：已进入 Replanning、Failed Node 已
冻结、其他并行 Active Node 已转为 Stopped，以及合法下一步（query / replan / workflow fail）。

Replan 提交的领域结构：

```text
ReplanBoundary（历史来源，不是 Node 或执行 Edge）
├── trigger_node_id
├── stopped_node_ids
└── frozen_fact_refs

ContinuationSubgraph
├── boundary
├── new_nodes
├── new_edges
├── entry_node_ids
├── rejoin_edges
│   ├── from_node_id    continuation 出口
│   └── target_node_id  原 Graph 既有 Pending 非 CompleteNode
└── pending_rewrites
    ├── update
    └── remove
```

- ReplanBoundary 承接触发失败、冻结事实与 Stopped Node，仅表达 provenance；它不是 Node，
  不参与激活、完成或 AND-join，也不把 Failed / Stopped Node 伪装成成功前驱；
- continuation 用 `entry_node_ids` 声明新执行入口，必须通过一个或多个普通 rejoin Edge 汇入
  原 Graph 尚未发生的部分；
- 每个 rejoin target 必须是 Replan 前已存在、当前仍为 Pending 的非 CompleteNode；不得指向
  Active / Completed / Failed / Stopped Node，也不得指向本次新建 Node；
- 禁止创建、替换或修改 CompleteNode，也禁止 continuation 直接连接 CompleteNode；
- `pending_rewrites` 可修改、删除尚未生效的 Pending Node 与未发生 Edge；作用于非 Pending
  节点即返回 `NodeHistoryFrozen`；
- 若不存在合法 rejoin target，返回 `WorkflowCannotContinue`；LLM 应显式
  `workflow_update(fail)`，不得通过新建普通「收口节点」再直连 CompleteNode 绕过；
- Runtime 执行可以跨多个 Node 或服务整个 Workflow；Replan 不要求逐项给 Stopped Node 或旧
  执行指定处置，也不等待它们 settled；
- 旧执行晚到的结果、错误与副作用信息保留为 facts，可供后续 LLM 引用，但不得自动推进旧
  Graph、改变 Failed / Stopped Node 或改写新 Graph。

合并后的**新 Plan Graph** 必须满足：

1. 结构仍满足全部 DAG 不变量；
2. 不依赖 Failed / Stopped Node 才能继续；
3. 至少存在一个可立即激活的入口 Node；
4. 每个新入口都存在经 rejoin Edge 与原 Graph 未发生部分到达原 CompleteNode 的有向路径；
5. 无自环、无环；CompleteNode 唯一无后继；有效图内非 CompleteNode 均有后继。

满足即提交：入口激活、remove 退役、update 生效、`ActiveMode` 回到 `Executing`、冻结历史逐
字段不变。

`workflow_replan` 的提交门禁是「新 Plan Graph 合法且可推进」，**不**包含旧执行是否存活或
副作用是否收敛——运行时冲突由通用 Policy、Hook、用户审批与执行安全机制处理。

## 4. 上下文生效边界

Workflow 相关上下文分两类，各有独立渠道，不混用。

### 4.1 稳定规则（system prompt）

Replanning 的行为约束与会话无关，随其他稳定内容一起组装进 `STATIC_SYSTEM_PROMPT_EN/_ZH`：

```text
当 Workflow 因节点失败进入 Replanning 模式时：不得继续推进旧 Graph，不得恢复或
原地改写 Failed / Stopped 节点。先用 workflow_query 读取当前状态，再选择
workflow_replan 继续，或用 workflow_update 的 fail / cancel / deactivate 终止或
挂起 Workflow。continuation 必须汇入已存在的 Pending 非 Complete 节点；不得直连
Complete 节点，也不得新建 Complete 节点。
```

system prompt 在会话启动 / resume / compact 时组装一次并冻结，Run 边界不重组，模式切换不
触发重建（前缀缓存不失效）。

### 4.2 当前状态（状态变化驱动的系统提示）

模式本身是会变化的事实，不能靠冻结的 system prompt 承载，也不能只依赖 Tool Result 历史。
使用状态变化驱动的系统提示注入，复用既有 Reminder 机制：

| 状态 | 注入 |
|---|---|
| 出现 Active(Executing) | 一条最小存在提示，含 `workflow_id` |
| 进入 Active(Replanning) | 一条模式提示，含 `workflow_id` |
| 状态未变 | 不重复注入（同 kind fingerprint 相同即跳过） |
| Inactive / 终态 | 不注入 |

Reminder source 契约：

- kind：`workflow_state`；优先级 180（高于环境、低于任务状态）；
- 触发：`OnStepInterval(1)`（每 step 边界检查一次）；
- 位置：`TailUserMessage`；
- 去重：`SkipIfUnchanged`；
- compact：`Rebuild`。

两条提示都不自动注入 Workflow 名称、目标、Active Node 详情、前驱结论、证据或完整 Graph；
LLM 必须调用 `workflow_query` 按需取得详情。

`/clear` 之后不重放；`Inactive(pending_mode=Replanning)` 重新 activate 时按状态变化再注入
一次。

### 4.3 其它边界

`workflow_node_update(fail)` 的 Tool Result 仍即时返回模式与合法下一步。Tool Result 与状态
提示互补，但都不构成 Kernel 可观测的前置条件；`workflow_query` 的顺序只是 guidance。

向 Provider 发起调用前，Runtime 已将提示组织进通用 messages；Provider 不接收 Workflow
专用字段，也不理解 Graph。Workflow、ActiveMode 或 Node Role 在一次 Model Invocation 发出后
发生变化，只在下一次 Model Invocation / continuation Step 重新组织上下文时生效。

### 4.4 两级上下文（D5，#2160）

全程激活后 Workflow 参与上下文组装，结构自上而下：

```text
system prompt（冻结，缓存命中）
├─ node summaries 区：每节点完成时【窗口追加一条】summary 消息，只 append
├─ 当前节点段：该节点激活后的 main runsteps（编排、回查、Web 结果等）
└─ 当前输入与 per-turn 变化内容（recall、Reminder，尾部）
```

- **前缀缓存约束只有一条「稳定在前、变化在尾」**：summaries 区位于 system prompt 之后、
  所有 per-turn 变化内容之前，只 append，不引入新保障机制；
- **过程隔离是省 token 的主收益**：sub run 的详细过程全量落盘（§15.5）但永不进入 main
  窗口；历史节点 main 侧过程消息在窗口中撤离、只留 summary，canonical append-only 不动；
- **合并最旧摘要只发生在窗口视图**：超阈值时本地结构化拼接（节点 id + 一行结论 +
  evidence 指针），不走 LLM，与不可变历史不冲突（同 compact 只压窗口的口径）；
- **区段感知 compact**：compact 仍为单一入口、触发逻辑不变，按区段分流——summaries 区
  只走本地合并；当前节点段走现有 LLM compact；节点完成时该节点段整体坍缩为一条
  summary，是天然压缩点。不做分段状态机 / 独立阈值。

## 5. Workflow Tool 协议

六个 Tool 始终出现在 LLM 的 Tool Schema 中。「始终」仅表示不受 Node Role 裁剪，仍受全局
工具开关、Runtime Policy、Hook、用户审批与执行期复验约束。

Tool Call 不等于状态已修改：只有 Runtime 通用链路和 Workflow Application / Kernel 校验均
通过后，修改才提交。MVP 不携带 `expected_revision`，不实现 CAS、stale revision conflict、
自动合并或并发写协调；更新按到达顺序针对当时当前状态校验并提交。

字段风格与现有 Tool schema 保持一致：`snake_case`、结构体级默认值、参数说明由字段注释
生成，不引入 Workflow 私有包装或错误描述约定。所有 Tool 除 `workflow_create` 外都要求
`workflow_id`。

### 5.1 `workflow_create`

一次提交完整 Graph；成功即 `Active(Executing)`；失败不留下半成品；已有 Active 时返回
`ActiveWorkflowAlreadyExists`；不承担后续 Replan。

```text
Input
├─ name: string                       必填，简短名称
├─ objective: string                  必填，完成目标
├─ root_index: int                    必填，root 在 nodes 中的下标
├─ nodes: [NodeInput]                 必填，≥1；LLM 不填 node id
└─ edges: [{ from_index, to_index }]  必填；下标引用 nodes；单 Node Graph 允许空数组

node id 由系统自动生成（`wnd_<下标+1>`，图内唯一）；create 结果返回真实
id，后续 node_update / rejoin 引用真实 id。replan 新节点同理用 `wrn_<序号>`。

Output
├─ workflow_id
├─ status: active
├─ active_mode: executing
├─ root_node_id
├─ complete_node_id
└─ node_count
```

### 5.2 `workflow_list`

只读列举当前 Session 可见的 Workflow 摘要，支持按状态过滤与分页。

```text
Input
├─ status: optional  active | inactive | completed | cancelled | failed
├─ limit:  optional int，默认 20，上限 100
└─ offset: optional int，默认 0

Output
├─ workflows: [
│    workflow_id, name, status, active_mode, pending_mode,
│    node_summary { pending, active, completed, failed, stopped },
│    updated_at
│  ]
├─ total: int        满足过滤条件的总数，不受分页影响
├─ limit, offset
```

`/clear` 之后该 Session 的引用集合为空，返回空列表与 `total = 0`；不提供跨 Session 检索。

### 5.3 `workflow_query`

只读查询指定 Workflow 的详情。它是 LLM 获取 Graph 结构的唯一入口：Context 只注入
`workflow_id` 级别的提示，不注入 Graph 内容。

```text
Input
├─ workflow_id: string          必填
└─ include_facts: bool          选填，默认 false

Output
├─ workflow_id, name, objective
├─ status, active_mode, pending_mode
├─ root_node_id, complete_node_id
├─ nodes: [
│    node_id, kind, role, status, description, completion_criteria,
│    conclusion?, evidence_refs[], successors[]?
│  ]
├─ edges: [{ from_node_id, to_node_id }]
├─ facts?: [{ fact_id, kind, source, related_node_ids, at }]
└─ allowed_actions: [allowed_action]
```

`evidence_refs` 与 `facts` 都只返回引用与摘要，不内联 Tool 输出正文；需要核对原始输出时，
LLM 通过既有 Tool result 读取能力取得。Workflow 侧不做存在性校验。

`allowed_actions` 是 Kernel 依据当前状态计算的**提示**，用于减少无效调用，不构成授权；真正
校验仍在提交时执行：

| 状态 | allowed_actions |
|---|---|
| `Active(Executing)` | `deactivate`、`complete`、`cancel`、`node_complete`、`node_fail` |
| `Active(Replanning)` | `replan`、`fail`、`cancel`、`deactivate` |
| `Inactive` | `activate`、`cancel` |
| `Completed` / `Cancelled` / `Failed` | 空数组 |

`workflow_list`、`workflow_query` 为只读操作，不列入 `allowed_actions`；`workflow_create`
不是针对单个 Workflow 的操作，也不列入。

### 5.4 `workflow_update`

仅负责 Workflow 聚合生命周期，用单一 `action` 枚举承载五个动作，不拆成多个 Tool。

```text
Input
├─ workflow_id: string
├─ action: activate | deactivate | complete | cancel | fail
├─ summary: optional string
│    complete：必填，交付摘要
│    fail：必填，失败原因
│    cancel：必填，取消理由
│    activate / deactivate：禁止
└─ evidence_refs: optional [EvidenceRef]

Output
├─ workflow_id
├─ status
├─ active_mode
└─ pending_mode         Inactive 时返回
```

动作语义：

- `activate`：Inactive → Active，按 `pending_mode` 恢复到 Executing 或 Replanning；已有其他
  Active 时返回 `WorkflowNotActive`；
- `deactivate`：`Active(Executing)` / `Active(Replanning)` → Inactive；当前 Run 不结束；保留
  `pending_mode`；
- `complete`：只允许 `Active(Executing)`；`Active(Replanning)` 一律返回
  `WorkflowReplanning`；
  - MVP 完成条件：CompleteNode 已 Completed，且当前有效 DAG 中不存在 Active Node；
  - MVP 允许 Pending 分支残留，但 `workflow_query` 必须如实显示，不得隐去；
  - 目标态（见 §11）：要求当前有效 DAG 中不存在 Active 与 Pending Node；
  - `summary` 必填；
- `cancel`：`Active(Executing)` / `Active(Replanning)` / `Inactive` → Cancelled；要求理由；
- `fail`：只允许 `Active(Replanning)` → Failed；要求失败原因；
- `Completed` / `Cancelled` / `Failed` 不可再次更新（`WorkflowAlreadyTerminal`）。

`workflow_update` 不接受 Node 结果、出口决定或 Graph 结构修改。

### 5.5 `workflow_node_update`

只负责 `Active(Executing)` Workflow 中的 Active Node。

```text
Input
├─ workflow_id: string
├─ node_id: string
├─ action: complete | fail
├─ conclusion: string                    必填
├─ evidence_refs: optional [EvidenceRef]
└─ successors: optional [SuccessorDecision]
     ├─ node_id: string                  必须是当前 Node 静态 Edge 的直接后继
     └─ description: string              必填

Output
├─ workflow_id, node_id, node_status
├─ status, active_mode
├─ activated_node_ids: []
├─ stopped_node_ids: []          仅 fail 时非空
└─ replan_boundary?: { trigger_node_id, stopped_node_ids, frozen_fact_refs[] }
```

`successors` 提交规则：

- `action = complete` 且 Node 不是 CompleteNode：必填，≥1；
- 即使只有一个直接后继，也必须显式提交；
- 多个后继形成并行 Active Node；
- CompleteNode 完成时禁止提交；
- `action = fail` 时禁止提交；
- 同一列表内重复 `node_id` 拒绝。

其它行为：

- `complete`：提交节点结果、证据引用和出口决定；触发隐式 AND-join 激活判定。**最小图
  例外（D2）**：图未分裂过时，完成 plan 节点即原子完成整个 Workflow（§2.2）；
- `fail`：提交失败结论和证据引用，不激活后继；整个 Workflow 立即进入 `Active(Replanning)`，
  其他并行 Active Node 在同一领域提交中进入 Stopped；Tool Result 返回 Replanning 模式、
  冻结事实与合法下一步；
- 校验顺序（固定，测试依赖）：Replanning 拒 → 节点状态（Pending / Stopped →
  `NodeNotActive`，Completed / Failed → `NodeHistoryFrozen`）→ successors 形状 → 状态迁移 →
  激活判定；
- `Active(Replanning)` / `Inactive` / 终态 Workflow 不接受该操作。

### 5.6 `workflow_replan`

只负责 `Active(Replanning)` Workflow 的 continuation subgraph。

```text
Input
├─ workflow_id: string
├─ boundary: {
│    trigger_node_id: string
│    stopped_node_ids: [string]
│    frozen_fact_refs: [EvidenceRef]
│  }
├─ new_nodes: [NodeInput]                    可为空（仅重组 Pending）
├─ new_edges: [{ from_node_id, to_node_id }]
├─ entry_node_ids: [string]                  必填，≥1
├─ rejoin_edges: [{
│    from_node_id: string                    continuation 出口
│    target_node_id: string                  既有 Pending 非 CompleteNode
│  }]                                        必填，≥1
└─ pending_rewrites: optional {
     update: [{ node_id, description?, role?, completion_criteria? }]
     remove: [node_id]
   }

Output
├─ workflow_id
├─ status: active
├─ active_mode: executing
├─ activated_node_ids: []
├─ applied_node_ids: []
└─ retired_pending_node_ids: []
```

行为约束与提交门禁见 §3.6。失败时保持 Replanning 并返回对应 typed error。

### 5.7 共享结构类型

```text
NodeInput
├─ kind (work | confirm | complete), description
├─ role                  kind = work 必填；confirm / complete 禁止
└─ completion_criteria   选填

EvidenceRef
├─ fact_id
└─ kind: tool_result | sub_agent_output | background_task | user_message

SuccessorDecision
├─ node_id
└─ description          必填
```

### 5.8 Typed business errors

```text
ActiveWorkflowAlreadyExists
WorkflowNotActive
WorkflowAlreadyTerminal
WorkflowReplanning
WorkflowNotReplanning
WorkflowCannotContinue
NodeNotActive
NodeHistoryFrozen
InvalidSuccessor
InvalidEvidenceReference
GraphInvariantViolation
CompletionConditionsUnsatisfied
WorkflowNotVisible
```

- `InvalidEvidenceReference` 只表示引用结构非法（缺 id、类型不符、字段形状错误），**不**表示
  被引用实体不存在；
- `WorkflowNotVisible` 表示该 `workflow_id` 已不在当前 Session 的引用集合中（例如 `/clear`
  之后），拒绝任何直接查询或操作；
- **不存在** `EvidenceNotFound`：Kernel 不校验证据实体是否存在。

错误附带可读说明与修复所需上下文。`Internal` 只用于非业务故障。

## 6. Runtime 与 Workflow 的交互边界

### 6.1 Runtime 路由 Workflow Tool

```text
Runtime ToolOrchestration
  ├─ 普通 Tool（MVP：仅受 Run 级 Tool Profile、Policy、Hook 与用户审批约束，
  │            不按 Active Node Role 裁剪）
  └─ Workflow Tool（六个，始终加入模型 Catalog）
```

Runtime 负责 Tool 解析、Catalog、Policy、Hook、Authorization、Interaction、执行和 Tool
Result 回传；Workflow 负责业务校验与状态提交。「始终可见」不意味着绕过 Runtime 通用链路，
也不意味着忽略全局禁用或权限拒绝。

### 6.2 Runtime 按 Session 最新状态组织上下文

- 提示由两部分组成：system prompt 承载稳定规则，状态变化驱动的 Reminder 承载当前
  ActiveMode；
- Workflow 详情由 LLM 通过 `workflow_query` 按需读取；
- 六个 Workflow Tool 始终出现在 Tool Schema，且不参与 Role 裁剪；
- 已发出的 Model Invocation 不被中途改写；
- Provider 完全不知道 Graph；
- Workflow 不创建、调度或终止 Run。

### 6.3 Node 结论、Evidence 与 Runtime facts

三类信息必须分开：

```text
Node 结论（conclusion）
  └─ LLM 的业务判断，属于 Workflow 聚合，随 Node 持久化

Evidence 引用（evidence_refs）
  └─ 指向客观执行产物的引用，随 Node 持久化

执行事实（facts）
  └─ Tool 调用结果、Sub-agent 输出、后台任务结果
     由既有 Tool result / 落盘机制保存，Workflow 不复制正文
```

已冻结的边界：

- Runtime 事实属于 Session 级执行事实，不依赖 Run → Workflow 绑定；
- Runtime、Tool、后台任务失败不自动等于 Node Failed；
- Runtime 事实不自动等于 Workflow 节点结论；
- LLM 必须在 `workflow_node_update` / `workflow_replan` 中显式提交结论与 evidence 引用；
- Workflow Kernel 不解释执行事实的业务含义。

evidence 引用的目的**不是让 Kernel 校验存在性**，而是让后续 LLM 能再次核对结论依据。因此
Kernel 只校验引用的**结构形态**（id 存在、类型字段合法），不校验实体是否存在。
`workflow_query` 只返回引用与可选的事实摘要，不内联事实正文。

### 6.4 用户不直接操作 Workflow

```text
用户输入 / TUI / slash
  ↓
Runtime 构造 Prompt（表达作为 LLM 语义输入或证据）
  ↓
LLM 决策并调用 workflow_* Tool
  ↓
Runtime ToolOrchestration → Workflow Application → Kernel
```

例如用户说「取消这个流程」，由 LLM 调用 `workflow_update(cancel)`；说「先不管这个计划」，
由 LLM 调用 `workflow_update(deactivate)`。TUI、slash、用户输入与 Runtime 均不直接执行这些
Workflow 语义操作。

## 7. 关键场景链路

### 7.1 创建并执行

```text
LLM 调用 workflow_create 提交完整 DAG
  ↓
Kernel 校验结构不变量
  ↓
Active(Executing)，root 成为唯一 Active Node
  ↓
LLM 调 workflow_query 读取 Graph 详情
  ↓
LLM 执行工作后调 workflow_node_update(complete) + successors
  ↓
Kernel 提交结果并激活合法后继（AND-join 判定）
```

### 7.2 节点失败与 Replanning

```text
任一 Active Node 无法继续
  ↓
LLM 调 workflow_node_update(fail)
  ↓
Node → Failed；结论与证据冻结
  ↓
Workflow 立即进入 Active(Replanning)
  ↓
其他并行 Active Node 原子转为 Stopped；旧 Graph 不再推进
  ↓
跨零到多个 Node 的旧 Runtime 执行可以继续存活
  ↓
其晚到结果、错误与副作用只记录为 facts
  ↓
Tool Result 即时告知模式与合法下一步
  ↓
system prompt 规则 + 状态 Reminder 携带 Replanning 约束
  ↓
LLM 判断能否提交可推进的新 Plan Graph
  ├─ 否：workflow_update(fail) → Failed
  └─ 是：workflow_replan 提交 continuation
            ↓
          continuation 严格汇入原 Graph 既有 Pending 非 CompleteNode
            ├─ 无合法 rejoin target：WorkflowCannotContinue，保持 Replanning
            └─ 有合法 target：Kernel 校验结构合法且可推进
                  ├─ 失败：保持 Replanning，返回 typed error
                  └─ 成功：至少一个入口可立即激活，回到 Active(Executing)
```

### 7.3 并行分支与 AND-join

```text
非 CompleteNode 提交多个 successors
  ↓
多个后继同时进入 Active（并行）
  ↓
各分支独立推进；汇聚 Node 在部分前驱完成时保持 Pending
  ↓
全部有效前驱 Completed 且均选择它 → 汇聚 Node 激活一次
```

### 7.4 Inactive 挂起与恢复

```text
用户被其他事务打断
  ↓
LLM 调 workflow_update(deactivate)
  ↓
Inactive(pending_mode = 当前模式)
  ↓
用户要求恢复
  ↓
LLM 调 workflow_list 找目标
  ↓
LLM 调 workflow_query 核对
  ↓
若已有其他 Active，先 workflow_update(deactivate)
  ↓
LLM 调 workflow_update(activate)
  ↓
按 pending_mode 恢复：None → Active(Executing)，Replanning → Active(Replanning)
  ↓
状态变化触发一次 Reminder（含 workflow_id）
```

## 8. Runtime Goal、Loop 与后台通知

Runtime Goal 是跨 Run 的长任务聚合根；Workflow 是 Session 级独立 Graph 聚合。两者互不拥有：

```text
Session
├── Workflows（0..n，至多一个 Active）
│   └── Workflow 聚合（Graph + 冻结历史）
├── Runtime Goals（0..n，可选）
│   └── 多次 Runtime Run + 继续授权
└── Standalone Runtime Run（可无 Goal）
```

Workflow 可以跨多个 Goal 和 Run 持续存在；Goal 可以不存在，Run 也可以不依附 Goal。Run 与
Workflow 没有结构绑定，只通过 Session 上下文组织和 LLM 的 Tool 调用发生关系。

Loop 不是 Runtime 类型，也不是特殊 Node。由于当前 Graph 仅允许 DAG，重复工作不通过回边或
自环表达。需要下一次迭代时，LLM 通过 `workflow_replan` 提交新的 continuation subgraph，并
保留既有 Node 历史；该操作只在 `Active(Replanning)` 中合法。周期计时、是否自动继续、是否
创建后台 Run 属于 Runtime Goal / Background Policy，而不是 Workflow Graph 的职责。

后台任务完成后的事实处理边界：

- 不自动完成或失败任何 Workflow Node；
- 不自动触发 Replan；
- 不自动修改 WorkflowStatus；
- 不自动创建 Run；
- 只能由 LLM 取得后，在 `Active(Executing)` 中经 `workflow_node_update` 形成节点结论；如果
  节点被显式标记 Failed，则 Workflow 进入 Replanning，之后才能经 `workflow_replan` 提交
  continuation subgraph。

Runtime facts 以引用形式进入 Node 的 `evidence_refs`；Workflow 只校验引用结构，不校验实体
是否存在，也不复制事实正文。

**agent 结论草稿管道（D4，#2160）**：§6.3 的「Runtime facts 不自动等于节点结论」在终态
层面保留，但开放一条草稿自动管道（§15.4）：节点 = 单次 agent 派发；agent 完成时 Runtime
把结论写入服务节点的草稿位（含 Failed / Cancelled 状态，但不自动 fail 节点）；main 显式
提交节点 Completed 时可采纳或改写草稿为最终 conclusion；重派为同节点新 attempt，替换
草稿位，不触发 Replanning。

## 9. MVP 一致性与并发边界

Workflow MVP 不做版本控制或并发写协调：

- 六个 Tool 不带 `expected_revision`；
- 不发布 Workflow revision 字段；
- 不实现 CAS、stale revision conflict、自动合并或 mutation_id；
- 不定义并发写入仲裁协议；
- 每次更新针对请求到达时的当前状态顺序校验并提交；
- 若未来真实并发需求出现，另行设计一致性协议，不在当前类型中预埋字段。

这不放松业务校验：每个请求仍必须基于提交时的当前状态通过状态机、冻结历史与 DAG 不变量
校验；不满足时返回对应 typed business error。

## 10. MVP 范围

MVP 的判定标准：只覆盖 Workflow Graph 领域自身闭环所必需的能力——

```text
LLM 创建 Graph → 推进 Node → 失败 → Replanning
→ continuation 续接 → 汇聚完成 → 持久化与恢复
```

不影响这条主链能否跑通的细节一律暂缓。**Node 对 Tool 的使用（Node Role 能力裁剪）整体不在
MVP 范围内**，见 §11。

### 10.1 MVP 内已确认

| 决策 | 结论 | 实现位置 |
|---|---|---|
| 六 Tool 协议 | Input / Output schema、共享结构、13 个 typed error | `tools/domain/types/workflow.rs`、`workflow/domain/tool_error.rs` |
| 生命周期状态机 | 五状态 + `pending_mode` 挂起恢复 + `allowed_actions` | `workflow/application/aggregate.rs` |
| 节点推进 | SuccessorDecision 校验 + 出口记录 + 隐式 AND-join | `workflow/application/aggregate.rs` |
| Replanning | 全局停止 + ReplanBoundary + 严格 rejoin 校验 | `workflow/application/aggregate.rs` |
| 持久化 | `~/.agents/workflows/{index,entities}` + temp+rename 原子写 + resume | `workflow/adapters/file_persistence.rs` |
| `/clear` | 只清 Session 索引与 active 指针，实体保留 | `workflow/application/store.rs` + `runtime` ResetRequested 分支 |
| 统一清理 | `aemeath cleanup`，只删无引用实体，幂等，fail-closed | `workflow/adapters/file_persistence.rs` + `composition/workflow_cleanup.rs` |
| Context 提示 | 稳定规则进 system prompt；模式用 Reminder | `share/i18n/prompt/constants.rs` + `runtime/reminder_sources.rs` |
| 总开关 | `workflow.enabled` / `AEMEATH_WORKFLOW_ENABLED`，**默认关闭** | `share/config/domain/workflow.rs` + `composition/runtime.rs` |

## 11. POST-MVP 移出清单

以下内容整体不属于 MVP，保留在此便于后续单独推进。实施时 **不得**因实现便利而隐式引入。

### 11.1 Node Role 与 Tool Policy

MVP 期间 Node Role 只是语义标签，不产生任何 Tool 裁剪；普通 Tool 可见性完全由现有 Run 级
Profile、Policy、Hook 与用户审批决定；六个 `workflow_*` 工具始终可见且不参与裁剪。

**#2160 D3 提前落地（覆盖本段 MVP 口径）**：全程激活后 main run 采用**全局固定 Profile**
（非 Node Role 驱动）：白名单 = 读 / WebSearch / WebFetch / Task / workflow_* / Agent /
AskUserQuestion，永久禁写、无退出通道，一切写操作经 agent 派发（§15.3）。下表 P1–P7 的
Role 级精细裁剪仍为暂缓项，D3 不依赖其实现。

| 编号 | 暂缓内容 |
|---|---|
| P1 | 六个 Node Role → `ToolCapability` 精确映射 |
| P2 | Verifier 的 Bash 能力（是否提供 Execute；只读命令如何用既有输入安全声明表达） |
| P3 | Workflow Role Profile 发布 / 读取契约 |
| P4 | Catalog 可见性与执行期授权同源（schema 投影与执行复验使用同一 Profile） |
| P5 | 并行 Active Node 的 Profile 装配（标识、缓存与失效策略） |
| P6 | 六个 Workflow Tool 的 capability 表达（如何在不做名字特判的前提下不被 Role 裁剪） |
| P7 | Replanning Profile 装配（失败恢复窗口内的普通 Tool 权限边界） |

### 11.2 其他暂缓项

| 编号 | 暂缓内容 | 暂缓理由 |
|---|---|---|
| P8 | `workflow_create` / `workflow_replan` 产品级资源保护 | 先由 Tool 超时与上下文预算兜底 |
| P9 | Kernel 与语义 Judge 边界 | MVP 只用确定性 Kernel |
| P10 | Workflow 创建触发 guidance | 先给最小指引，完整触发规则后补 |
| P11 | 生命周期终止 guidance（何时 complete / cancel / deactivate） | 状态机已定，行为指引后补 |
| P12 | Runtime Goal / 后台继续授权完整模型 | 与 Graph 主链正交 |
| P13 | v0.1 历史文档与 v0.2 Target 文档共存策略 | 治理事项 |
| P14 | Provider resolver 独立接线 | 已判定与 Workflow 解耦 |
| P15 | `complete` 对 Pending Node 的额外收紧 | 目标态要求当前有效 DAG 无 Pending |
| P16 | TUI 展示 Workflow 状态 | 用户不直接操作 Workflow，展示不阻塞主链 |

### 11.3 后续讨论顺序

建议顺序为 P1–P7（Node Role 与 Tool Policy 是一个整体，需一次对齐），再到 P12（Runtime
Goal），最后 P13、P14。

## 12. 当前确认的不变量与目标护栏

1. Runtime 只能通过 Prompt 调动 LLM；Provider 只接收通用 messages / tool schema / model
   options，完全不知道 Graph。
2. Workflow 是 Session 级独立聚合；Run、RunStep、ModelInvocation 和 Runtime Goal 不绑定
   Workflow。
3. 用户不直接操作 Workflow；所有语义操作只能由 LLM 经六个 `workflow_*` Tool 完成。
4. 一个 Session 可保存多个 Workflow，同一时刻最多一个 Active；create / activate 不自动替换
   当前 Active。
5. WorkflowStatus 是 Active / Inactive / Completed / Cancelled / Failed 五个并列状态；
   终态不可逆。Active 使用 Executing / Replanning 两种模式，Replanning 不是 Node 或并列
   WorkflowStatus。
6. `/clear` 只移除 Session 内的 Workflow 引用与 active 指针，不删除实体，不新增
   `workflow_clear`；失去引用的 Workflow 返回 `WorkflowNotVisible`。
7. `workflow_create` 一次提交完整 DAG；Graph 有且只有一个普通 root 和一个显式 CompleteNode。
8. Graph 只允许 DAG；禁止自环、回边和 Failure Routing；全部 Node 从 root 可达，所有非
   CompleteNode 均存在到 CompleteNode 的路径。
9. NodeStatus 是 Pending / Active / Completed / Failed / Stopped；只有 LLM 可显式 fail
   Active Node，执行失败事实不自动映射为 Node Failed。
10. 任一多前驱 Node 使用隐式 AND-join；全部**有效**直接前驱 Completed 且均选择该后继后，
    目标 Node 只激活一次；Failed / Stopped 前驱被忽略（既不消耗也不阻塞）。
11. CompleteNode 不负责用户确认；确认由通用 ConfirmNode 在 Graph 任意合法位置表达。
12. Active / Completed / Failed / Stopped Node 及冻结历史不可被 Replan 修改；Pending Node
    与未发生 Edge 可调整，但修改后必须保持全部 DAG 不变量。
13. ReplanBoundary 以 provenance 承接触发失败、冻结事实与 Stopped Node，不是 Node 或执行
    Edge；continuation 用 entry 声明新入口，并必须通过普通 rejoin Edge 汇入 Replan 前已存在
    的 Pending 非 CompleteNode。
14. CompleteNode 在 Workflow 创建时确定，生命周期内身份稳定；Replan 不得创建、替换或修改
    它，continuation 不得直接连接它。没有合法 rejoin target 时返回 `WorkflowCannotContinue`。
15. 任一 Active Node 经 LLM 显式 fail 后，整个 Workflow 立即进入 Active(Replanning)，其他
    并行 Active Node 原子转为 Stopped；跨零到多个 Node 的 Runtime 执行不因此被机械停止。
16. Stopped 不恢复、不自动 Completed / Failed，也不触发旧出口。旧执行晚到结果只形成
    facts，不自动推进旧 Graph、改变 Failed / Stopped Node 或改写新 Graph。
17. `workflow_replan` 只以合并后的新 Plan Graph 合法且可推进为提交门禁：不依赖 Failed /
    Stopped Node，至少一个可立即激活的入口，且每个新入口都可沿有效路径到达原 CompleteNode。
18. Replan 不要求逐项决定 Stopped Node 的承接 / 放弃，也不等待旧执行 settled。
19. `workflow_update`、`workflow_node_update`、`workflow_replan` 职责分离；Tool Call 只有
    校验并提交后才产生状态变化。
20. Workflow MVP 不做 revision、CAS 或并发写协调。
21. Replanning 的稳定行为规则常驻 system prompt；当前 ActiveMode 属于易变状态，由状态变化
    驱动的 Reminder 注入（复用既有管线），状态未变不重复注入；两者都不携带 Graph 详情。
22. Node Role 是六元封闭集合，不复用 `agents.roles` 配置身份。
23. Node 持久化 `conclusion` 与 `evidence_refs`；Kernel 只校验引用结构形态，不校验被引用
    实体是否存在，因此不存在 `EvidenceNotFound`。
24. Tool result、Sub-agent 输出与后台任务结果由既有落盘机制保存，Workflow 只存引用，不复制
    事实正文。
25. Session 只保存 Workflow 索引与 active 指针，Workflow 实体独立存储；`/clear` 只清索引，
    不删实体。
26. `deactivate` 保留 `pending_mode`：从 `Active(Replanning)` 挂起后再次 activate 必须回到
    Replanning，不得直接回到 Executing。
27. **MVP 不启用 Node Role 对普通 Tool 的任何裁剪**：不发布 Role Profile、不计算能力并集、
    不投影 Tool Schema、不装配 Replanning Profile；普通 Tool 可见性完全由现有 Run 级 Tool
    Profile、Policy、Hook 与用户审批决定。
28. Workflow 不创建、调度、暂停或终止 Run，不负责计时、后台唤醒、通知路由或跨 Node 执行
    收敛。
29. Runtime finalized facts、Workflow 节点结论和 Runtime Goal 生命周期事实是不同事实，不能
    自动混同。
30. ReasoningLevel / Provider resolver 与 Workflow 解耦，resolver 不感知 Graph。
31. MVP 的 `complete` 要求 CompleteNode 已 Completed 且当前有效 DAG 不存在 Active Node；
    允许 Pending 残留但必须如实查询显示。
32. 六个 Workflow Tool 不受 Node Role 裁剪，但仍经过 Runtime Catalog、Policy、Hook、全局
    开关、用户审批和执行期复验；MVP 总开关默认关闭。

### 12.1 目标护栏（非当前可执行不变量）

- `complete` 进一步要求当前有效 DAG 不存在 Pending Node（§11 P15）；
- Node Role 能力映射与 Replanning Profile（§11 P1–P7）；
- Workflow 创建 / 终止的完整 LLM guidance（§11 P10–P11）；
- Runtime Goal 与后台继续授权模型（§11 P12）。

## 13. 非目标

- 通用工作流编排平台（可编程 DSL、可视化编辑器）；
- 用 Graph 替代 Runtime 的 Run 生命周期管理；
- Workflow 直接驱动 Provider、LLM 或 Tool 执行；
- 用户直接编辑 Graph 或通过 TUI / slash 操作 Workflow 状态；
- 多 Workflow 并行 Active；
- Provider 感知 Graph 结构或节点语义。

## 15. Workflow 底层化（#2160 定稿）

> 2026-11-01 经 B1–B5 五批逐项拍板。核心：「Workflow 是开启后的唯一执行组织形态」，
> main run 只做编排，一切写操作经 subagent，过程全量落盘、上下文按节点隔离。
> 本节是 D1–D6 的汇总与唯一登记处；各既有章节的对应修订见 §1.1、§2.1–2.3、§4.4、
> §5.5、§6.3、§11.1。

### 15.1 决策总表

| # | 决策 | 内容 |
|---|---|---|
| D1 | 全程激活 | 无 Inactive 候选 → Runtime 默认建 `plan→done` 最小图；有候选 → Selecting（§2.2） |
| D2 | 简单出口 | 最小图未分裂时 plan complete 原子收口整个 Workflow，一次调用（§2.2、§5.5） |
| D3 | main run 永久禁写 | 固定 Profile 白名单，无退出通道（§11.1、§15.3） |
| D4 | agent 结论管道 | 节点 = 单次 agent 派发；草稿自动写入，Completed 由 main 显式提交（§6.3、§15.4） |
| D5 | 两级上下文 | summaries 区 + 当前节点段；区段感知 compact（§4.4） |
| D6 | sub run 落盘与展示 | canonical 全量落盘、挂父 session 子空间、TUI 下钻（§15.5） |

### 15.2 D1 全程激活细则

- 挂点：session 开始 / resume / clear 后判定；非对话 Run（sub run、后台 wakeup、RSI 等）
  不触发；
- 无候选时 Runtime 直接建最小图（§1.1 写入口一），root 为 Planner 角色 WorkNode；
- Selecting 为 Session 级状态硬约束（§2.2），与 Replanning 同构，非提示约束；
- `workflow.enabled=false` 时行为与底层化之前完全一致。

### 15.3 D3 main run 禁写

- 白名单：读 / WebSearch / WebFetch / Task / workflow_* / Agent / AskUserQuestion
  （Ask 是交互不是写）；复用 `ToolProfile::derive_restricted` 实现；
- 设计判断（已拍板）：「需要写时已经是一个复杂工作，值得完整 workflow 做」，不做按
  任务规模的轻量写豁免——裁量规则会让 LLM 每次判断「这算轻量吗」，行为不可预测；
- 交互边界：sub 维持禁 `AskUserQuestion`；澄清 = main 派发前问清 + sub 返回澄清需求时
  main 代问**重派**（无 resume，接受重派成本）；policy 审批透传与 main 等待期交互服务
  循环搁置（§15.8）。

### 15.4 D4 结论管道与 Q7 workflow adaptor

- **Q7 归属注入不改 Agent 工具**：Agent 工具参数 schema 与调用语义零改动。main run 在
  workflow 模式下，Agent 派发路径经 **workflow adaptor**：由 adaptor 依据 main 当前服务
  节点，把 `workflow_id / node_id` 注入 `AgentRunRequest` 的 system prompt 扩展与 sub 元
  数据。「多 Active 归属谁」由 adaptor 读 workflow 状态判定，不靠 LLM 填参数；开关关闭
  时 adaptor 不装配；
- **结论草稿**：agent 完成时 Runtime 经写入口二把结论原文写入节点草稿位（单条当前有
  效值，无集合），Failed / Cancelled 也记录状态但不自动 fail 节点；
- **提交权在 main**：main 显式 `workflow_node_update(complete)` 时采纳或改写草稿为最终
  conclusion；压缩责任在 main 提交时，runtime 搬运路径不做压缩、不做硬截断；
- **重派 = attempt**：同节点再派一次替换草稿位，旧 attempt 过程留 sub 存档；不触发
  Replanning（replan 只管「图错了」，不管「同一节点再试一次」）；
- 与 D6 共用 sub 收尸挂点：flush 落盘 + 写草稿 + 回 tool result，一次实现。

### 15.5 D6 sub run 落盘与展示

- sub run 装配 canonical writer，复用 main 的 append 契约（revision、receipts、终态
  flush）；独立 session id，元数据带 `parent_run_id` 与 `workflow_id / node_id`（adaptor
  注入）；
- 存档挂父 session 子空间（如 `<project>/<parent_session>/subruns/<sub_id>`），**不进
  session 列表枚举**；父 session 统一清理时级联 GC；
- sub 不是用户可 resume 的会话，续派（如未来支持）是 runtime 内部按 id 加载；
- TUI 按 Agent 调用下钻只读查看 sub 完整消息流；main 界面保持只有编排与结论。

### 15.6 跨 workflow 记忆与失真框架

前提（已拍板）：**失真必然，设计目标是丢得可控、可回溯**。摘要只当索引、不当事实源；
省 token 大头是过程隔离，不是结论压缩。

- **终态可检索**：Completed / Cancelled 实体放开只读检索（§2.3）；`/clear` 无特殊处理；
- **回查三档粒度**：L0 = `workflow_query`（结论 + 证据引用，百字级）；L1 = 按单条
  `evidence_ref` 取原文——指针升级为可解析定位（`session_id / run_id / message_id`），
  从 canonical 存档读取，单条 KB 级；L2 = 节点完整 runsteps / sub run 存档（诊断与深查，
  依赖 D6）；
- **行为约束**：默认信摘要，信息不足或冲突时回查；回查是只读、不需要 replan；回查后
  发现需返工才走图变更；
- **经验沉淀**：workflow 历史不进 memory（150/200 容量不背此负载）；可复用经验由
  reflect 按既有门禁提炼，禁止任务流水账进 corpus；
- **边界**：canonical 被统一清理后原文永久消失，回查返回「原文已清理」，属可接受失真；
- **不做**：三字段免压缩结构化摘要（提取即失真，伪保真）、近期完成摘要主动注入。

### 15.7 实施分期与验收

P0–P5 对应子 issue #2161–#2166。依赖：全部依赖 P0（设计文档）；P3 另依赖 P2（共用收
尸挂点）；P4、P5 依赖 P3。验收口径统一为「单测 + 真模型 L4/L5」：

- P4 是 D3 生死验收：禁写形态下复杂编码任务端到端完成；
- P5 指标：压缩后重问率、重复读取率、摘要回查率、前缀缓存命中率；开/关 workflow 的
  token 与延迟基线对比。

**分支策略**：全部改动在集成分支 `feat/2160-workflow-foundations`，整体验证通过后合
main；合入前 main 的 workflow 现状与 `workflow.enabled` 默认行为不变。

**回退策略**：开关只影响新 session / resume 后建图行为；存量实体与存档不动；关闭后
adaptor 不装配、main 恢复完整工具集。

### 15.8 搁置项（不在任何分期，勿视为遗漏）

- policy 审批透传展开（sub 写操作审批的呈现与路由）；
- main 等待 Agent 期间的交互服务循环（审批与 sub Ask 的共同前置）；
- sub 开放 `AskUserQuestion`；sub resume 续派；
- 近期完成 workflow 摘要主动注入（先观察终态检索是否够用）。

## 16. 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-11-02 | #2160 底层化定稿落地：§1.1 Runtime 写入口、§2.1 默认建图、§2.2 Selecting 与 D2 原子收口、§2.3 终态检索放开、§4.4 两级上下文、§6.3 D4 草稿管道、§11.1 D3 固定 Profile、新增 §15（D1–D6、跨 workflow 失真框架、分期与搁置项） | [#2160](https://github.com/rushsinging/aemeath/issues/2160) / [#2161](https://github.com/rushsinging/aemeath/issues/2161) |
| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-10-26 | 设计真相源收敛：六 Tool 协议、五状态生命周期与 ActiveMode、Node 五状态含 Stopped、Replanning continuation、MVP 范围与 POST-MVP 移出清单、A–G 45 场景验收链、不变量与目标护栏 | [#555](https://github.com/rushsinging/aemeath/issues/555) / [#2090](https://github.com/rushsinging/aemeath/issues/2090) |
| 2026-10-29 | 按已落地实现（#2082–#2089）重写本文：补充 §2.4 落盘与统一清理、§3.2 Kernel reason 全表、§3.4 有效前驱 AND-join 语义、§5.5 校验顺序、§10.1 实现位置表、§12 不变量扩充至 32 条 | [#2091](https://github.com/rushsinging/aemeath/issues/2091) |
