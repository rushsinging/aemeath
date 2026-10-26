# Workflow 设计收敛检查点

> 日期：2026-10-08
> 状态：已获用户确认的修改范围
> 用途：保存本轮 Workflow 设计收敛的明确边界，避免会话压缩导致决策丢失。
> 非规范说明：本文件是设计过程检查点，不是第二份目标设计真相源；规范性目标设计仍以 [01-workflow-design.md](../docs/design/02-modules/workflow/01-workflow-design.md) 为准。

## 具体修改内容

### 1. 修正 Workflow Tool 数量和职责

当前文档仍写“五个”，但已确认应为六个：

```text
workflow_create
workflow_list
workflow_query
workflow_update
workflow_node_update
workflow_replan
```

调整：

- `workflow_replan` 从 `workflow_node_update` 中独立出来；
- `workflow_update` 只负责 Workflow 生命周期；
- `workflow_node_update` 只负责 Active Node 的状态、结论、证据和后继；
- `workflow_replan` 只负责 Pending Node 和未发生 Edge 的图结构调整；后续 2026-10-09 决策将其限定为 Active(Replanning) 中提交 continuation subgraph，详见下文增量；
- 六个 Tool 都始终出现在 LLM 的 Tool Schema；
- 六个 Tool 仍统一经过 Runtime 的 ToolOrchestration、Policy、Hook 和执行期复验，不创建旁路。

### 2. 把 Node Role 改成封闭六角色

替换当前“例如十个角色”的开放写法，明确为：

```text
Planner
Explorer
Architect
Implementer
Verifier
Reviewer
```

同时明确：

- Node 只引用 Role；
- Node 不直接声明 `allowed_tools`；
- Node 不允许在 Role 之上增删单个 Tool；
- Confirm / Complete 是 Node 类型，不新增 Confirmer / Completer Role；
- Node Role 不复用 `agents.roles` 配置对象；
- 只复用现有 Sub-agent Tool Policy 的 capability vocabulary、`ToolProfile`、Registry Scope 和 `derive_restricted`。

### 3. 写清普通 Tool 的有效范围

在 §3.5 固化以下规则：

```text
有效普通 Tool
= Run 基础 Scope
  ∩ 当前 Active Node Role capability Profile
  ∩ 父 capability ceiling
```

并明确：

- Node Role 只能收缩，不能扩权；
- Runtime 每次 Model Invocation 前重新计算；
- 当前已发出的 Model Invocation 不被中途修改；
- 多个 Active Node 的能力合并规则先按“能力并集后再与 ceiling 求交”记录；
- 六个 Workflow Tool 不受 Node Role 裁剪，但仍受 Runtime 全局 Policy、Hook、用户审批和全局工具开关约束。

### 4. 不把尚未决定的 Role 映射伪装成已决定

只落盘已经确认的机制，不擅自冻结以下内容：

- 六个 Role 各自具体对应哪些 capability；
- Verifier 是否允许 Bash；
- 如果允许 Bash，是否只能执行只读命令；
- Workflow Role Profile 的发布和读取契约；
- 并发 Active Node 的详细运行时装配协议。

这些内容从正文的“已定规则”移到 §11 的“仍待决”，并分别列出决策影响。

### 5. 收口当前 Workflow 文档内部的旧口径

因为当前文档内部还存在会误导实现的旧内容，在同一个文件内同步修正：

- 五个 Tool → 六个 Tool；
- Replan 不再属于 `workflow_node_update`；
- 删除 Failure Routing、回边、自环等已被推翻的表述；
- DAG、唯一 root、唯一 CompleteNode、隐式 AND-join 等 Graph 规则已确认；后续 2026-10-09 决策将 NodeStatus 扩展为五状态并加入 Stopped，详见下文增量；
- 删除 revision、`revision + 1`、CAS、并发冲突等内容；
- 删除 WorkflowStatus 层级仍待决的旧描述；
- 修正 Inactive Workflow 不能直接 `node_update` / `replan` 的描述；
- Active Workflow 的 Context 只注入“存在提示 + workflow_id”，详情通过 `workflow_query` 获取；
- 修正 §1.3 中残留的旧 `execution_contract` / `ExecutionContract` / Completion Gate 术语。

### 6. 重写待决项总览

§11 不再重复列已经确认的事项，只保留真正未决项，主要包括：

1. 六个 Tool 的最小请求、返回和完整 typed error schema；
2. 六个 Node Role 的 capability 映射；
3. Verifier 的 Bash 及只读命令约束；
4. Workflow Role Profile 的发布 / 读取契约；
5. `workflow_create` 的安全边界和资源保护；
6. `complete` / `cancel` 的完整 Kernel 校验；
7. Workflow 持久化、resume 和损坏恢复；
8. Runtime facts 如何成为 Workflow evidence；
9. Kernel 是否需要独立 Judge；
10. Prompt 刷新时机和预算；
11. Goal / Loop 与 Workflow 的继续授权关系；
12. 关键场景验收链；
13. v0.1 历史文档与 v0.2 目标文档的共存策略。

## Replanning 决策增量（2026-10-09）

以下决策由用户在后续讨论中明确确认，用于取代本检查点末尾“替代节点激活路径尚未决定”的旧问题表述：

1. **不替换 Failed Node**：Failed Node、结论、evidence 与已发生历史永久保留；Replan 不创建具有“替代身份”的 Node。
2. **Replanning 是 Active Workflow 模式，不是 Node**：Active 内区分 `Executing` / `Replanning`；不新增 ReplanNode，也不把 Replanning 建模为并列 WorkflowStatus。
3. **新增 Workflow 终态 `Failed`**：WorkflowStatus 从四个并列状态扩展为 `Active / Inactive / Completed / Cancelled / Failed`；Failed 不可逆。
4. **任一节点失败立即全局 Replanning**：LLM 经 `workflow_node_update(fail)` 将任一 Active Node 置为 Failed 后，整个 Workflow 立即进入 Replanning；其他并行 Active Node 在同一领域提交中不可逆地转为 Stopped，旧 Graph 不再推进。
5. **Replan 只要求新 Plan Graph 可推进**：LLM 可通过 `workflow_query` 获取当前 Graph、冻结历史与现有 Runtime facts；query 顺序是 guidance，不建立 Kernel 前置状态。不能继续则 `workflow_update(fail)`；可以继续则直接提交 `workflow_replan`，不等待旧执行停止或 settled。
6. **可继续时提交 continuation subgraph**：`workflow_replan` 一次提交新的依赖规划；它可以创建一个或多个新 Node，也可以重组尚未生效的 Pending Node / Edge。Kernel 校验新 Plan Graph 的结构合法性与可推进性。
7. **Replan 成功恢复 Executing**：新 Plan Graph 不依赖 Failed / Stopped Node，至少存在一个可立即激活的入口，且每个新入口都有到原 CompleteNode 的有效路径时，ActiveMode 从 Replanning 返回 Executing；校验失败保持 Replanning。
8. **通知与硬约束同时生效**：
   - `workflow_node_update(fail)` Tool Result 即时告知已进入 Replanning、Failed Node 已冻结、并行 Active Node 已转为 Stopped 以及合法下一步；
   - 后续每次 Model Invocation 注入 Replanning 持续提示，但不自动注入完整 Graph；
   - 普通 Tool 使用独立的模式级 Replanning Profile，不新增 Node Role；
   - Tool Profile、Catalog 与 Workflow 业务校验共同阻止旧 DAG 推进并保证新 Plan Graph 可推进；Prompt 不是唯一约束。
9. **Replanning Profile 仍受上层边界**：六个 Workflow Tool 继续可见；普通 Tool 必须受 Run 基础 Scope、父 capability ceiling、Policy、Hook、用户审批和全局开关约束。
10. **显式历史边界承上**：`workflow_replan` 使用 `ReplanBoundary` 引用触发失败、冻结事实与 Stopped Node。Boundary 只表示 provenance，不是 Node，也不形成 Failed / Stopped Node 到新工作的成功执行 Edge。
11. **严格 Rejoin 启下**：continuation 以 entry 声明新执行入口，并必须通过一个或多个普通 rejoin Edge 汇入 Replan 前已存在、当前仍为 Pending 的非 CompleteNode；不得汇入 Active / Completed / Failed / Stopped Node、本次新建 Node 或 CompleteNode。
12. **CompleteNode 身份稳定**：唯一 CompleteNode 在 Workflow 创建时确定，生命周期内不得由 Replan 创建、替换或修改；continuation 必须在汇入原 Graph 后沿其尚未发生路径到达原 CompleteNode。
13. **无合法 Rejoin 即不可继续**：如果原 Graph 不存在合法 Pending 非 CompleteNode 作为 rejoin target，则 `workflow_replan` 返回 `WorkflowCannotContinue` 并保持 Replanning；LLM 应显式 `workflow_update(fail)`，不得新建普通收口 Node 再直连 CompleteNode。
14. **Stopped 是不可逆 Node 终态**：进入全局 Replanning 时，除触发失败者外的并行 Active Node 转为 Stopped。Stopped 不是 Completed 或 Failed，不满足 AND-join，不恢复、不触发旧出口，也不自动推进旧 DAG。
15. **Runtime 可以跨多个 Node**：Run、Sub-agent、Tool Call 与后台任务可以关联零到多个 Node 或服务整个 Workflow；Stopped 不拥有或机械取消这些执行，Replan 也不要求它们先停止、settled 或完成影响分类。
16. **旧执行晚到结果仅作为 facts**：Replan 前后返回的结果、错误和副作用信息不能自动完成或失败 Stopped Node，不能触发旧出口、推进旧 DAG 或改写新 Graph；但是否仍有旧执行存活不阻塞 `workflow_replan`。
17. **无需逐项处置 Stopped Node**：LLM 不必为每个 Stopped Node 明确承接或放弃。Kernel 只校验新 Plan Graph 不依赖 Failed / Stopped Node、存在可立即激活的入口，并可沿有效路径到达原 CompleteNode。
18. **本段替代此前收敛门禁**：不再使用 `WorkflowExecutionNotQuiescent`，不再把 cancellation_unconfirmed、副作用未收敛或旧执行仍存活作为 Workflow Graph Kernel 的提交前置条件；运行时冲突由通用 Policy、Hook、用户审批和执行安全机制处理。

## 2026-10-25 增量：MVP 范围拍板

用户在 2026-10-25 明确：节点对 Tool 的使用不属于 MVP，并按此重划范围。以下决策已写入 01-workflow-design.md 正文。

### 已拍板（进入 MVP）

20. **M2 Replanning 生命周期出口（方案 B）**：`Active(Replanning)` 允许 `replan` / `fail` / `cancel` / `deactivate`，拒绝 `complete`。`deactivate` 是通用生命周期操作，典型场景是用户被其他事务打断而主动挂起 Workflow。
21. **Inactive 待恢复模式**：`deactivate` 不清除 ActiveMode。从 `Active(Replanning)` 挂起后 `Inactive` 携带 `pending_mode = Replanning`，再次 `activate` 回到 `Active(Replanning)`；否则会带着 Failed Node 直接回到 Executing，违反冻结不变量。
22. **M3 完成充分条件（先 B 后 C）**：MVP 要求 CompleteNode 已 Completed 且当前有效 DAG 不存在 Active Node，允许 Pending 残留但必须如实查询显示，并要求交付摘要；目标态进一步要求不存在 Pending Node，仅允许 Completed / Failed / Stopped。
23. **M4 证据语义修正**：Node 持久化 `conclusion`（LLM 业务判断）与 `evidence_refs`（引用）。facts 指客观执行产物（Tool 结果、Sub-agent 输出、后台任务结果），由既有落盘机制保存。引用的目的不是让 Kernel 校验存在性，而是让后续 LLM 能回读原始输出再次核对。因此 Kernel 只校验引用结构形态，删除 `EvidenceNotFound`，改为 `InvalidEvidenceReference`。
24. **M1 Tool 风格约束**：六个 Tool 的 schema 与 prompt 必须与现有 Tool schema / prompt 保持一致，不引入 Workflow 私有字段命名、错误包装或 description 约定。
25. **M5 持久化（方案 C）**：Session 只保存 Workflow 索引与 active 指针，Workflow 实体独立存储。
26. **M6 上下文提示**：Replanning 的稳定规则常驻 system prompt；当前模式属于易变状态，由状态变化驱动的系统提示注入，复用既有 Reminder 机制，状态未变不重复注入。不因模式切换重建 system prompt。
27. **M7 `/clear` 语义替换**：`/clear` 只移除 Session 内的 Workflow 引用与 active 指针，不删除实体；实体成为无引用数据。已失去引用的 Workflow 拒绝任何按 `workflow_id` 的直接查询与操作（`WorkflowNotVisible`），不提供隐式复活路径。落盘内容的物理清理由后续统一清理能力负责，覆盖 session、tool result、workflow 等。

### 移出 MVP（POST-MVP，暂缓）

28. **Node Role 与 Tool Policy 整体移出**：MVP 期间 Node Role 只是语义标签，不发布 Role capability Profile、不计算能力并集、不投影普通 Tool Schema、不装配 Replanning Profile。普通 Tool 可见性完全由现有 Run 级 Tool Profile、Policy、Hook 与用户审批决定；六个 `workflow_*` Tool 始终可见且不参与裁剪。涉及六 Role capability 映射、Verifier Bash、Role Profile 发布 / 读取、Catalog 与执行期同源、并行装配、Workflow Tool capability 表达、Replanning Profile 装配共七项。
29. **其他移出项**：create / replan 资源保护、Kernel 与语义 Judge 边界、Workflow 创建与终止 guidance、Runtime Goal / 后台继续授权模型、v0.1 与 v0.2 文档共存、Provider resolver 独立接线。

### MVP 内剩余待决

30. 无。MVP 内设计待决项已清空，验收链已冻结（见下条）。

### 已冻结：MVP 验收链

30. **A 创建与结构校验**：合法最小 Graph、无 CompleteNode、多 root、有环、非 CompleteNode 无后继、已有 Active 时创建、CompleteNode 有后继；拒绝场景必须同时断言「状态不变」与「返回具体 typed error」。
31. **B 正常推进与 AND-join**：单后继完成、缺 successors 被拒、非法后继被拒、并行 + 汇聚只在全部前驱 Completed 且均选择后激活一次、CompleteNode 完成后 Workflow 不自动 Completed。
32. **C 节点失败与 Stopped**：单节点失败、并行时另一分支转 Stopped、Stopped 不可再更新、Failed / Stopped 不可修改。
33. **D Replanning 与 continuation**：Replanning 拒绝 node_update 与 complete、合法 continuation 恢复 Executing、无合法 rejoin target 返回 `WorkflowCannotContinue`、禁止直连 CompleteNode、rejoin 目标必须为既有 Pending 非 CompleteNode、entry 必须可激活、冻结历史不可改、旧出口不被 Stopped Node 触发、`fail` 进入不可逆 Failed。
34. **E 生命周期**：Executing 挂起恢复、Replanning 挂起后 activate 回到 Replanning、Inactive 拒绝推进、三态均可 cancel、complete 条件不足被拒、complete 允许 Pending 残留且查询如实显示、缺少 summary 被拒、终态不可逆。
35. **F 可见性、持久化与查询**：列表分页与 total、状态过滤、`/clear` 后不可见且按 id 拒绝（`WorkflowNotVisible`）、`/clear` 不删实体、resume 恢复三种状态、query 只返回引用不内联正文、引用结构合法即可提交（无 `EvidenceNotFound`）。
36. **G 提示与 allowed_actions**：allowed_actions 与 §5.3 对照表一致、提示不构成授权、状态提示只在变化时注入、提示不含 Graph 内容。
37. **追加规则**：后续依真实使用可追加场景，但不得修改已冻结的 A–G 断言。

### 已冻结：六 Tool 协议

31. **Tool 结构**：六个 Tool 全部保留；lifecycle 的五个动作（activate / deactivate / complete / cancel / fail）用单一 `workflow_update.action` 枚举承载，不拆成多个 Tool。
32. **workflow_create**：一次提交 `name` / `objective` / `root_node_id` / `nodes` / `edges`，创建即 Active，root 为唯一初始 Active Node。
33. **workflow_list**：支持 status 过滤与分页（`limit` 默认 20、上限 100，`offset` 默认 0），返回 `total`；`/clear` 后返回空列表。
34. **workflow_query**：返回 Graph、Node 状态与结论、evidence 引用、可选 facts 摘要，以及 `allowed_actions` 提示。`allowed_actions` 由 Kernel 按状态计算但不构成授权：Executing 为 deactivate / complete / cancel / node_complete / node_fail；Replanning 为 replan / fail / cancel / deactivate；Inactive 为 activate / cancel；终态为空。
35. **workflow_node_update**：`action ∈ complete | fail`；`conclusion` 必填；`successors` 在非 CompleteNode 完成时必填且 `description` 必填，fail 与 CompleteNode 完成时禁止。
36. **workflow_replan**：提交 `boundary`（trigger_node_id / stopped_node_ids / frozen_fact_refs）、`new_nodes`、`new_edges`、`entry_node_ids`、`rejoin_edges`（target 必须是既有 Pending 非 CompleteNode）与可选 `pending_rewrites`。
37. **共享类型**：`NodeInput`、`EvidenceRef { fact_id, kind }`、`SuccessorDecision { node_id, description }`；字段风格与现有 Tool schema 一致，走结构体级默认值 + 字段注释生成 schema。
38. **错误枚举**：ActiveWorkflowAlreadyExists、WorkflowNotActive、WorkflowAlreadyTerminal、WorkflowReplanning、WorkflowNotReplanning、WorkflowCannotContinue、NodeNotActive、NodeHistoryFrozen、InvalidSuccessor、InvalidEvidenceReference、GraphInvariantViolation、CompletionConditionsUnsatisfied、WorkflowNotVisible；不使用 `EvidenceNotFound`。

## 本检查点不包含的后续问题

本文件保存已经获得用户确认的修改范围与 Replanning 决策增量。已冻结的 continuation 使用 ReplanBoundary + entry + 严格 rejoin Edge，并保持原 CompleteNode 身份稳定；NodeStatus 已加入不可逆 Stopped。当前 Replan 门禁只要求新 Plan Graph 合法且可推进，不要求跨 Node Runtime 执行先停止或收敛，晚到结果只作为 facts。Node Role 对 Tool 的任何裁剪整体属于 POST-MVP，MVP 内不得隐式引入。Tool schema、typed error 枚举与关键场景验收链仍必须继续作为独立决策，不得由实现自行假设。
