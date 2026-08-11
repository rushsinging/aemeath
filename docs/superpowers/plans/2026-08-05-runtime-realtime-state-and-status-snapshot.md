# Runtime 实时状态发布与 TUI Status 快照实施计划

> **范围：** 通用 committed side-effect 实时观察 + Runtime-owned Published State Registry + Status Presentation 快照/心跳 + Compact committed-work 进度模型
> **前置关系：** 当前功能分支落后 `origin/main` 的 Task structured state 提交；执行前必须先 rebase 最新 `origin/main`，解决三事件语言重构与 Task 管线的冲突。
> **执行方式：** 在现有 feature worktree 中按任务顺序执行 TDD；所有跨层链路逐层补测试，先红后绿。计划完成后更新现有 PR #1541，不合并。
> **交付约束：** 本计划不得只为 Task 增加专用回调后结束；必须一次性建立通用的 commit 观察、Runtime 状态注册、立即发布、心跳重发和 TUI 原子替换架构。Task 与 Status 是同一批交付中的首批完整接入能力。

## 1. 目标

交付一个完整的 Runtime 实时状态架构，以及首批接入该架构的两个能力：

1. **通用 committed side-effect observation**
   - 单个 Tool 完成可证实的 durable/authoritative commit 后，立即消费 typed commit metadata；
   - 由 capability-specific handler 查询或读取该 revision 对应的权威状态，并立即发布完整 Published State；
   - 下一个互斥 mutation 必须等对应 committed observation 完成后才能开始；
   - Tool round 仍按 provider 原始顺序统一 materialize LLM-visible Tool Results、写入 canonical message history，并决定下一次 model invocation；
   - Task 作为首批 handler 完整接入，但 seam、dispatcher、执行路径和测试契约不得绑定 Task，必须可承载具备 typed commit metadata 与权威 read model 的 Config、Workspace、Memory 或后续 capability。

2. **Runtime-owned Published State Registry**
   - Runtime 为需要向 SDK/TUI 交付的完整状态维护统一的状态注册与版本语义；
   - capability owner 仍拥有业务真相，Registry 只保存稳定 Published State，不成为 Task、Context、Config、Workspace、Lifecycle 或 Activity 的第二业务状态机；
   - commit observer 与非 Tool Runtime fact observer 都通过同一 publish/update boundary 更新对应状态；
   - 业务变化 dirty-immediate publish，心跳按订阅/session 生命周期周期重发当前完整状态。

3. **Runtime-owned Status Presentation 快照**
   - Runtime 汇集 Status Line 真正需要的运行状态、当前生效配置、待生效配置、workspace 和 Context budget；
   - 权威事实变化时立即发布完整快照；运行期间通过心跳周期重发当前快照；
   - TUI 按 `(session_id, revision)` 原子替换，不再从 `Usage`、配置事件和局部状态自行拼装业务语义；
   - Context 百分比直接使用 Context compact decision 的权威口径，避免 TUI 显示低于阈值却已经 compact 的假象。

4. **Task 完整状态作为首批 committed capability**
   - 单个 Task mutation tool 完成真实 commit 后立即发布该 revision 的完整 `TaskStateChanged` 并触发 typed Task Hook；
   - ordinary、streaming、approval、cancellation-preserved result 共用通用 seam；
   - Task 专属逻辑只存在于 capability handler，禁止侵入 dispatcher、round coordinator 或心跳机制。

5. **Compact committed-work 进度模型**
  - Context owner 发布的 summarization 计数必须表示已完成工作，禁止把 future 创建、排队或调度计为完成；
  - map chunks、reduce 合并、refresh 收敛与 canonical finalization/commit 必须有稳定、typed、可扩展的操作阶段语义；
  - Runtime、SDK 与 TUI 无损传递同一进度事实，TUI 仅将 typed operation progress 转换为视觉比例，不从 context utilization 或 auto-compact threshold 推断操作进度；
  - ordinary auto-compact、provider context-too-long retry 与 idle `/compact` 共用同一 producer/progress contract，成功、跳过、失败、取消、超时均可靠结束对应 Activity；
  - 进度必须单调、不得提前到达阶段上限，`Finalizing` 和 Activity terminal 必须可观测，终态 Activity 不再参与 LiveStatus 选择。

## 2. 根因与边界

### 2.1 Task 延迟的根因

当前链路把两个生命周期错误绑定：

```text
Task domain commit
  → TaskCommandResult(revision, events)
  → CommittedTaskChange（Tools → Runtime 内部 metadata）
  → 等待整个 Tool round 收敛
  → materialize_tool_results
  → ChatToolRoundObserver::results_materialized
  → 查询一次最新 Task read model
  → 发布最终 TaskStateChanged
```

同一 round 连续多个 Task mutation 时，中间 revision 不可见。SDK sink 没有缓存；延迟发生在 Runtime 构造 `TaskStateChanged` 之前。

根因修复必须拆开：

```text
A. Tool side-effect commit
   单结果完成 → committed-result observation → Task state/Hook 实时可见

B. Conversation commit
   round 收敛 → provider 顺序恢复 → Tool Result materialize → canonical history commit
```

### 2.2 Status Line 错误的根因

TUI 当前用：

```text
last provider input_tokens / raw context_size
```

自行计算 `ctx %`；Runtime 实际 compact 判定使用：

```text
provider input + output，或完整候选窗口估算
  对比
(context_size - reserved_context - max_output_tokens) × 0.8
```

两者在分子、分母、阈值和 fallback 上都不同。compact 成功后 Runtime 会重置 usage baseline，但 TUI 仍可能保留旧 `last_input_tokens`，造成进一步滞后。

### 2.3 Compact 进度长期停在 85% 的根因

当前 TUI 把 `Summarizing(current, total)` 映射为：

```text
15% + 70% × current / total
```

因此 `current == total` 精确显示 85%。Context 的 map-reduce producer 当前在构造 chunk futures 时依次发布 `1/N..N/N`，随后才执行 `buffer_unordered` 并等待真实摘要完成。结果是最后一个 `N/N` 表示“所有 chunk 已创建/调度”，而不是“所有 chunk 已完成”。实际耗时更长的 map 执行、reduce 合并和可能的多轮 refresh 收敛发生在 85% 之后，且期间没有新的 typed progress，所以 UI 长时间停在 85%。

这不是以下指标：

- Context utilization；
- auto-compact threshold；
- token 准确进度；
- message 数量进度。

`current/total` 的当前单位是 map chunk；其错误在于 completion 语义，而非 SDK/TUI 字段丢失。成功链路已经具备 `Finalizing` 映射和 Activity terminal，但 producer 的阶段模型不足以表达 reduce/refresh，且 map 计数发布时间错误。

根因修复必须建立稳定的 operation progress contract：

```text
Preparing
→ Generating { state }
→ Mapping { completed_chunks, total_chunks }
→ Reducing { completed_passes, planned_or_bounded_passes }
→ Refreshing { completed_rounds, max_rounds }
→ Finalizing { commit_state }
→ Activity terminal
```

约束：

- `completed_*` 只在对应工作成功完成并被本次 compact operation 接纳后递增；
- future 构造、入队、开始执行不能递增 completed；
- 并发完成顺序不得改变 chunk 原始顺序或最终摘要拼接顺序；
- reduce/refresh 的未知总量必须显式表达为 indeterminate 或 bounded，不得伪造精确百分比；
- fallback、skip、error、cancel、timeout 都必须发布可收敛的 stage/terminal，不允许遗留 Running/Waiting observation；
- TUI 的视觉权重是 presentation policy，不得反向成为 Context 业务状态或 compact decision authority。

### 2.4 共同架构原则

```text
authoritative owner state/fact
  → typed application observation
  → Runtime Published Language 完整 DTO
  → sink 立即发送
  → SDK 无损映射
  → TUI 按 identity/revision 原子替换
```

不引入公共事件总线、event sourcing、ack/retry、增量 replay，也不让 TUI 重建 Task、Context 或配置状态机。

## 3. 通用实时状态架构与事件语言归属

### 3.1 Committed side-effect observation

统一边界由三部分组成：

```text
ToolExecution
  → CommittedSideEffectDispatcher
  → capability-specific CommittedSideEffectHandler
  → authoritative state query / receipt mapping
  → PublishedStateRegistry.update
  → RuntimeStreamEvent
```

约束：

- dispatcher 只识别 typed committed metadata 并路由，不查询业务 read model、不触发 Hook、不构造 SDK DTO；
- 每个 capability handler 只拥有自己的 commit metadata、权威查询、Hook 与 Published State mapper；
- `ToolOutcome::is_error == false` 绝不等价于 committed fact；
- synthetic success/denied/error/cancelled/no-op 不能伪造 commit；
- 不预设一个包含所有未来能力的公共 mega-enum；typed registration/handler 必须允许能力独立接入，同时保持静态可审计；
- 非 Tool 事实（Context decision、provider usage、Run presentation、Config effective transition、Workspace observation）不绕道 Tool dispatcher，而是通过对应 Runtime fact observer 更新 Registry。

### 3.2 Published State Registry

Registry 是 Runtime application-owned 的交付状态容器，负责：

- 按 state family 保存最新完整 SDK-facing view；
- 维护 `session_id`、family-local revision、heartbeat sequence 与 dirty 标记；
- 业务字段变化立即发布；
- 心跳重发当前状态；
- new/resume/reset/session switch 建立或清除 revision epoch；
- 提供确定性的 snapshot read，不在 heartbeat tick 临时跨 BC 拼装事实。

Registry 不负责：

- 决定 Task/Context/Config/Workspace 业务转换；
- 决定 lifecycle terminal；
- 从 Activity 推断 Run 状态；
- 决定是否 compact；
- 替代各 capability 的权威 read model。

### 3.3 Runtime 事件语言归属

保持 Runtime 三语言边界：

- `RuntimeLifecycleEvent`：Run / Run Step lifecycle 与 terminal authority；
- `RuntimeActivityEvent`：Activity changes/snapshots，始终 observational；
- `RuntimeStreamEvent`：Task state、Status Presentation、content、provider/tool/session progress 与 commands。

新增或调整：

```text
RuntimeStreamEvent::TaskStateChanged { state }
RuntimeStreamEvent::StatusPresentationChanged { snapshot }
```

两者都表示面向 SDK/TUI 的完整状态交付，不进入 Lifecycle 或 Activity。

## 4. Status Presentation DTO

### 4.1 身份与版本

快照至少包含：

- `session_id`；
- 当前 active Main `run_id` / `run_step_id`，无 active identity 时为 `None`；
- `revision`：展示字段发生业务变化时递增；
- `heartbeat_sequence`：每次重发递增，不改变 revision；
- `observed_at` 或等价 Runtime 时间戳。

TUI 规则：

- 新 Session 建立独立 revision epoch；
- 高 revision 原子替换；
- 相同 revision 幂等接受，只更新 heartbeat/liveness 字段；
- 低 revision 丢弃；
- Resume/Reset 必须显式建立或清除状态，不能继承旧 Session 快照。

### 4.2 运行展示状态

仅承载 Status Line 需要的 presentation，不拥有生命周期：

- idle / processing / waiting / cancelling 等展示阶段；
- status notice；
- elapsed、API calls、累计 input/output、TPS；
- Activity 可以提供摘要文本，但不得决定 Run/Step terminal。

### 4.3 当前与待生效配置

必须区分：

- 当前 Run 实际生效的 provider/model/context window/max output/thinking/policy；
- next-run pending 配置；
- session-restart-required 配置提示；
- frozen Run config revision 与 committed config revision（若现有 DTO 支持）。

Status assembler 不读取配置文件、环境变量或 `ConfigReader`；只消费 Runtime 发布的稳定 SDK DTO。

### 4.4 Context budget

快照必须直接携带 Context owner 计算后的稳定 view：

- `context_size`；
- `effective_context_window`；
- `decision_token_count`；
- `compact_threshold`；
- `usage_percentage`；
- `decision_source`：provider actual / heuristic fallback / unknown；
- `compaction_needed`；
- compact phase/progress 摘要（仅展示）。

建议 `usage_percentage` 定义为：

```text
decision_token_count / effective_context_window
```

`compact_threshold` 单独保留，避免把“窗口占用率”和“阈值进度”混为一个百分比。TUI 不复刻 `reserved_context`、`max_output` 或 0.8 阈值算法。

### 4.5 Compact operation progress

Status Presentation 中的 compact 摘要必须引用 Context owner 发布的 typed operation progress，不得复用 Context budget 百分比。稳定视图至少表达：

- operation identity；
- operation kind：automatic / provider-context-retry / manual；
- stage：preparing / generating / mapping / reducing / refreshing / finalizing；
- stage work：determinate `{ completed, total }` 或 indeterminate；
- operation revision；
- terminal/Activity identity 关联所需的稳定字段。

阶段内计数的业务语义由 Context 定义；Runtime 只适配、发布并关联 Activity。视觉进度由 TUI 在 typed stage 上应用稳定权重，但必须满足：

- 同一 operation 的视觉比例单调不下降；
- mapping 的 `completed == total` 只表示 map 完成，不表示 summarization 全部完成；
- reduce/refresh 未完成时不得提前显示其阶段上限；
- finalizing 表示摘要已定稿并正在进行 canonical sanitization、Task context append、持久化或 generation publish 等最终提交工作；
- Activity terminal 后立即停止展示 operation progress；
- Context utilization、compact threshold 与 operation progress 分别展示或命名，禁止共享一个模糊的“compact 百分比”。

### 4.6 快照范围限制

Status 快照不得包含：

- 完整 Task 列表；
- 完整 Activity 树；
- canonical messages；
- Tool results；
- lifecycle terminal receipts；
- `CommittedTaskChange` / `TaskEvent`；
- Context 内部 aggregate。

Task 与 Activity 继续走各自完整状态/事件管线。Status 最多引用稳定摘要。

## 5. 发布模型

### 5.1 变更立即发布

以下事实变化后更新 Runtime-owned snapshot、`revision + 1` 并立即发布：

- Session new/resume/reset；
- active Main Run/Step identity 变化；
- processing/waiting/cancelling/idle presentation 变化；
- current/pending config 或 permission mode 变化；
- workspace/worktree/branch 变化；
- provider usage 被接受；
- Context candidate/window 与 compact decision 完成；
- compact started/progress/finished/failed；
- model/config context window 变化。

### 5.2 心跳重发

心跳只用于：

- liveness；
- elapsed 等时间派生字段；
- transport/TUI 短暂不同步后的最终收敛。

规则：

- dirty 状态立即发送，不等待 tick；
- heartbeat 保持相同业务 revision，只增加 `heartbeat_sequence`；
- processing 时建议 500ms–1s；idle 时降频到 3s–5s，或仅在订阅者存在时发送；
- 定时器不得临时跨模块拼装业务真相，必须读取已经维护的完整 snapshot；
- 心跳失败不得改变 Runtime lifecycle 或触发 compact。

## 6. Committed side-effect 统一观察点

### 6.1 通用职责

新增职责分离的通用 dispatcher 与 capability handler，例如：

```text
CommittedSideEffectDispatcher::observe(call, execution)
TaskCommittedSideEffectHandler::observe(change)
```

通用 dispatcher 必须：

1. 在单个真实 Tool execution 返回 typed outcome 后运行；
2. 识别 typed committed metadata；
3. 路由到已注册的 capability handler；
4. 等待 handler 完成，之后才释放该 capability 的 mutation lane；
5. 对无 metadata、failed/no-op/synthetic outcome 直接返回；
6. 不 materialize LLM content、不写 canonical history、不决定 round continuation。

本批至少完整接入：

- Task committed state；
- 会改变 Status Presentation 的 Config/Workspace committed tools（若 main 中已有 typed commit metadata，则接入 dispatcher；若变化事实由现有 Runtime service/command owner 发出，则接入 Runtime fact observer，禁止伪造 Tool commit metadata）；
- 其他已有 committed metadata 的内置工具必须在实施审计中逐项分类：接入、明确无 SDK-visible state，或记录不适用理由。不得只搜索 `task_change` 后结束。

### 6.2 Task capability handler

Task handler 的职责仅为：

1. 消费 `CommittedTaskChange`；
2. 读取 `change.revision()`；
3. 在同一 serialized Task mutation lane 内查询权威 read model；
4. 验证查询状态 revision 与 change revision 一致；
5. 构造完整 `TaskStateView { session_id, revision, ... }`；
6. 更新 Published State Registry 并立即发布 `TaskStateChanged`；
7. 从 `TaskChangeFact` 触发 TaskCreated/TaskCompleted Hook；
8. 保证每个真实 commit 最多查询一次、发布一次、触发一次 Hook。

Task handler 不得：

- materialize LLM content；
- 写 canonical history；
- 改 provider call order；
- 决定 round continuation；
- 将 `CommittedTaskChange` 放入 SDK、LLM 或 Session payload。

### 6.2 串行临界区

Task mutation 的必要顺序：

```text
commit revision N
→ query state N
→ publish state N
→ run typed Task hooks for N
→ release serialized Task mutation lane
→ allow commit N+1
```

禁止：

```text
commit N
→ release lane
→ commit N+1
→ query state for N
```

否则 revision N 可能携带 state N+1。

### 6.3 执行路径覆盖

必须让以下真实 execution 路径共享 observation seam：

- 普通 Tool round；
- streaming Tool round；
- sequential tools；
- concurrent-safe tools 与串行 Task tools 混合；
- approval 后恢复执行；
- cancellation 前已经真实完成并保留的 committed results。

synthetic denied/error/cancelled/no-op 没有 committed change，不查询、不发布、不触发 Task Hook。

### 6.4 Streaming 串行约束审计

执行前必须验证 streaming `submit` 是否真正遵守 `ToolDescriptor::is_concurrency_safe()`。若 Task mutation 只受全局 semaphore 而未进入串行 lane，先补失败测试，再将 ordinary/streaming 统一到相同的安全调度策略。

## 7. 实施任务

### Task 1：同步主线并建立新基线

**步骤：**

1. 在现有 worktree fetch 最新 `origin/main`；
2. rebase feature branch 到最新 `origin/main`；
3. 逐项解决 Task structured state 与三事件语言冲突，禁止丢弃任一侧测试；
4. 搜索并清除旧 `RunDomainEvent` / `map_domain_event` / Activity-in-stream 术语；
5. 运行 Runtime、SDK、CLI 相邻测试与架构守卫，记录基线。

**验收：** 分支包含 main 的 Task 重构和本分支三事件架构，工作区干净，已有测试通过。

### Task 2：先写通用 commit observation 与 Task 多 commit 延迟复现测试

**路径：** Tools committed metadata、Runtime dispatch/coordination/streaming/interaction 测试。

**测试：**

- dispatcher 对 typed commit 路由一次，对无 metadata/failed/no-op/synthetic result 不路由；
- 两个 capability handler 可独立注册/调用，证明 seam 不绑定 Task；
- handler 完成前对应 mutation lane 不释放；
- 同 round 连续 7 个 `TaskCreate`：revision 为 `N..N+6`，item counts 为 `1..7`；
- `Read + TaskCreate + Bash + TaskCreate` 只发布两次；
- failed/no-op/synthetic denied 不发布；
- 每个 commit 只查询一次 read model；
- Task Hook 每个真实 transition 只触发一次；
- Tool Result 最终仍按 provider 原始顺序一次性写入历史；
- streaming Task writes 串行且 revision/state 一致；
- approval 后真实 commit 与 ordinary 路径行为一致；
- cancellation 前完成的 commit 被观察，缺失/合成结果不被误观察；
- 审计现有内置 mutation tools，形成测试内的明确 committed-capability catalog 或 guard contract。

**验收：** 相关测试在生产改动前按预期失败，并能明确归因于缺少通用 commit observation 与 round-level Task observation。

### Task 3：建立通用 dispatcher、capability handlers 与串行 mutation lanes

**步骤：**

1. 定义 committed side-effect dispatcher/handler port，禁止 Task 语义进入通用接口；
2. 为现有 typed committed metadata 建立显式 capability handlers；
3. 把 Task state query、发布、typed Hook 从 `results_materialized` 和分散的 `non_agent` 路径迁入 Task handler；
4. ordinary、streaming、approval 共用同一 dispatch 边界；
5. 确保每个互斥 capability 在 handler 完成前不释放 mutation lane；
6. round finalization 删除任何 committed state query，只保留 conversation commit；
7. 保持内部 commit metadata 为 serde-skipped、non-wire 数据；
8. 对 Config/Workspace 等影响 Status 的 mutation 建立 typed handler 或接入对应 Runtime fact observer，不允许继续由 TUI 猜测；
9. 加 architecture guard，禁止 dispatcher 直接依赖 Task read model/Hook/SDK mapper，禁止 execution path 绕过 dispatcher。

**验收：** Task 2 测试全部转绿；普通/streaming/approval 无行为分叉；至少通过测试证明第二个非 Task capability 可接入同一 seam，且 Status 相关权威变化进入后续 Registry。

### Task 4：先写 Context budget、Compact committed-work 与 Status 快照失败测试

**路径：** Context compact producer/decision、Runtime progress/activity/snapshot publisher、SDK mapper、TUI ACL/reducer/model/assembler/render tests。

**测试：**

- map-reduce 在 future 创建/入队时不增加 completed，首个 chunk 真正完成后才发布 `1/N`；
- 并发 chunk 按完成事实推进计数，但最终 sub-summary 按原 chunk index 排序后 reduce；
- mapping `N/N` 后进入 reducing，而不是把 summarization 操作视为完成；
- reduce 与每轮 refresh 发布 typed stage/work，未知工作量显式 indeterminate 或 bounded；
- preparing → generating 或 mapping → reducing → refreshing（按需）→ finalizing 的 operation revision 单调且字段不丢失；
- 单次摘要路径使用 generating indeterminate，不伪造 chunk counters，并在摘要完成后进入 finalizing；
- LLM fallback、无 generator、本地路径、resume-protection skip、error、cancel、timeout 均产生正确 terminal，并且不遗留 Running/Waiting compaction Activity；
- automatic、provider-context-retry、manual compact 共享 progress contract；
- Runtime progress adapter 保留 operation identity、stage、work 与 revision，Activity update 使用同一 identity；
- SDK mapper/wire 与 TUI adapter 无损保留所有 stage/work；
- TUI LiveStatus 不选择 terminal compact Activity，不在 activity observation stale 时继续展示旧 operation；
- TUI generating/mapping/reducing/refreshing/finalizing 权重单调，mapping 完成不得代表整个 operation 的 85% 完成假象；
- Context utilization、compact threshold、operation progress 三者在 view model 中使用不同字段和标签；
- 200K context、16K max output、provider total 145K 时，快照反映 144K threshold 且 `compaction_needed = true`；
- provider actual 与 heuristic fallback 的 `decision_source`、token count 不丢失；
- compact 成功后旧 provider baseline 不残留在快照；
- config current/next-run/session-restart-required 不混淆；
- business change 增加 revision 并立即发布；
- heartbeat 只增加 sequence，不增加 revision；
- Resume/new Session/reset 建立正确 revision epoch；
- TUI stale revision 丢弃、duplicate revision 幂等、新 Session 独立；
- TUI `ctx %` 使用 SDK snapshot，不读取旧 `last_input_tokens/context_size` 算法。

**验收：** 跨层测试先红，并分别定位 producer 把 scheduled work 当 completed work、operation stage DTO 缺失、publisher/Activity terminal 缺失和 TUI 本地推导。

### Task 5：实现 Compact committed-work producer、Published State Registry 与 Runtime-owned Status snapshot

**步骤：**

1. 在 Context compact domain 定义职责单一的 typed operation stage/work，completed 只表示已完成且已接纳的工作；
2. 重构 map-reduce producer：每个 indexed chunk future 完成后推进 completed，按 chunk index 恢复 sub-summary 原始顺序，再进入 reduce；
3. 为 reduce 与 refresh 收敛发布独立 typed stage/work；不能预知精确总量时使用 indeterminate 或算法上界；
4. 将 finalizing 边界覆盖 summary 定稿后的 sanitize、Task context append、canonical persist/generation publish，并明确定义成功/skip/fallback/error/cancel/timeout terminal；
5. 让 automatic、provider-context-retry、manual compact 共用同一 progress adapter 与 Activity completion helper，删除重复 stage mapping/finish 分支；
6. 在 Runtime application 层建立窄职责 Published State Registry；
7. 定义 family-local identity/revision/dirty/update/read contract；
8. 让 committed capability handlers 与非 Tool Runtime fact observers 共用 Registry update/publish boundary；
9. 在 Registry 上建立 Status snapshot family，而非另造平行缓存；
10. 定义稳定的 Runtime Status view 与 SDK DTO，其中 compact operation progress 与 Context budget 分字段表达；
11. 将 Context decision 映射成 Context budget view；
12. 接入 active Main identity、运行 presentation、workspace 与 current/pending config；
13. 对每个权威事实变化执行更新、revision、dirty publish；
14. compact finished/failed/cancelled/timed-out/skipped/reset 明确更新 operation progress、Activity terminal 与 budget；
15. 不把 Lifecycle/Activity/Task aggregate 复制成第二真相；
16. Task 完整 state 也使用相同 Registry 版本/立即发布基础设施，但保留独立 DTO、revision epoch 与 SDK event。

**验收：** Runtime 侧 Task 4 测试转绿；任何 chunk 调度都不会推进 completed；并发不破坏摘要顺序；所有 compact 路径可靠收敛到 typed finalizing/terminal；Registry 无业务决策权；Task、Status 与 Compact presentation 共享发布基础设施但 family 状态与 revision 不互相污染。

### Task 6：实现心跳与 Live/Resume 交付

**步骤：**

1. 增加 Runtime-owned heartbeat 调度；
2. processing/idle 采用配置化或常量化合理频率；
3. 心跳读取当前完整 snapshot 并重发；
4. Live、Resume、reset、session switch 均发布完整快照；
5. session/stream 结束后可靠停止 heartbeat task，避免泄漏；
6. sink 关闭或发送失败只记录/终止 publisher，不影响 lifecycle terminal。

**验收：** fake clock 驱动下频率、revision、停止与 session 隔离测试通过，不依赖真实 sleep。

### Task 7：贯通 SDK 与 TUI 原子替换

**步骤：**

1. `map_stream_event` 无损映射 Status snapshot 与 compact operation progress；
2. 更新 SDK wire schema/golden；
3. SDK ACL 映射到 typed `TuiRuntimeEvent`，不得把 operation stage 压扁成字符串；
4. reducer/model 按 `(session_id, revision)` 原子替换 Status family，并按 operation identity/revision 更新 Compact presentation；
5. Status assembler 只读取 snapshot，不读取配置 reader、不重算 Context budget；
6. LiveStatus assembler 只选择同一 active Main run 中最新的非 terminal compact operation，stale observation 时不回退展示旧进度；
7. renderer 分别展示 Runtime 提供的 context percentage/threshold 与 stage-weighted operation progress；
8. 将 presentation weights 收口为单一策略，generating/mapping/reducing/refreshing/finalizing 各有稳定区间，indeterminate stage 不伪造完成比例；
9. 退役旧 `last_input_tokens / context_size` 业务推导、旧三阶段 chunk 调度计数假设和无生产消费者的镜像字段。

**验收：** Runtime → SDK → ACL → reducer → assembler → render 每层均有相邻测试；最终 TUI 场景证明真实 map 完成前不会到达 mapping 上限、reduce/refresh 可见、terminal 后进度消失，且 context 指标与 operation progress 不混淆。

### Task 8：更新架构守卫与权威设计文档

**步骤：**

1. 更新 Runtime event pipeline 文档，加入通用 committed side-effect dispatcher、capability handlers、Published State Registry、Task 首批 handler与 Status snapshot/heartbeat；
2. 更新 TUI event flow/ACL 文档，明确原子替换与禁止本地业务推导；
3. 更新 Runtime、Context、TUI 模块 README/index/cross-reference；
4. 新增/更新 guards，阻止：
   - 通用 dispatcher 绑定 Task、Context 或任一具体 capability；
   - 已登记 committed mutation execution path 绕过 dispatcher；
   - capability handler 未完成即释放互斥 mutation lane；
   - Task state 回到 round `results_materialized` 才发布；
   - Task Hook 在 execution 路径重复实现；
   - streaming Task mutation 绕过串行 lane；
   - `CommittedTaskChange` 或其他内部 commit metadata 进入 SDK/Session/LLM wire；
   - heartbeat tick 临时跨 BC 拼装状态而非读取 Registry；
   - TUI 恢复 `last_input_tokens / context_size` compact 指标；
   - Status snapshot 驱动 lifecycle terminal 或 compact decision；
   - Context compact producer 在 future 创建、排队或开始执行时递增 completed；
   - compact operation stage 在 Runtime/SDK/TUI 任一层压扁为字符串或丢失 work semantics；
   - TUI 用 Context utilization 或 compact threshold 推断 operation progress；
   - automatic/provider-retry/manual compact 复制 progress stage mapping 或 Activity terminal cleanup；
   - terminal compact Activity 继续进入 LiveStatus progress selection；
   - heartbeat 增加业务 revision。

**验收：** 正例通过，deliberate negative probe 能被 guard 阻断，文档与 enum/mapper/source 一致。

### Task 9：全量验证、提交与 PR 更新

**验证命令：**

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./.agents/hooks/check-architecture-guards.sh
cargo xtask sdk-wire-schema check
git diff --check
```

另执行：

- Runtime Task ordinary/streaming/approval/cancellation 场景；
- Context compact decision 与 committed-work progress 单元测试；
- Context map/reduce/refresh 并发完成计数、摘要顺序和所有 terminal 路径测试；
- Runtime compact Activity identity/revision/finalizing/terminal 测试；
- SDK mapper/wire contract；
- TUI status 与 LiveStatus compact progress P0/P1 场景；
- fake-clock heartbeat 测试；
- 必要的 live/resume 场景。

完成后：

1. 检查完整 diff、废弃路径和重复逻辑；
2. 提交到现有 feature branch；
3. 推送并更新 PR #1541，说明新增范围和验证证据；
4. 如工作范围需拆分 Issue，先请求用户授权，禁止自行创建或拆分；
5. 不合并 PR，不自行关闭 Issue。

## 8. 分层测试矩阵

| 层级 | Task 实时状态 | Status 快照/心跳 | Compact committed-work progress |
|---|---|---|---|
| L0 单元 | change/fact、单 query、串行顺序、no-op | budget DTO、revision、heartbeat sequence | completed-work 语义、indexed chunk ordering、stage monotonicity、terminal matrix |
| L1 reducer/component | observer、round ordering | snapshot owner、dirty publish、reset | Context producer、Runtime adapter/Activity update、TUI operation model |
| L2 contract | Tools metadata 不泄漏；SDK 完整 Task DTO | Runtime/SDK/TUI DTO 无损、wire schema | stage/work/identity/revision 在 Context→Runtime→SDK→TUI 无损 |
| L3 scenario | 7 次 TaskCreate、混合 tools、approval、streaming | provider actual、fallback、compact/reset/resume | automatic/provider-retry/manual；map/reduce/refresh；fallback/cancel/timeout |
| L4 TUI | 每个 revision 立即替换并重绘 | status 原子替换、stale/duplicate/session epoch | 阶段权重单调、indeterminate、terminal 消失、stale 不回退旧 observation |
| L5 system | 完整 Run Tool round 与历史顺序 | heartbeat lifecycle、Live/Resume、真实 compact 展示 | 慢 chunk/reduce/refresh 下不提前到阶段上限，最终 Finalizing→terminal |

## 9. 不变量清单

### 通用 committed side effect

- 通用 dispatcher 不依赖任一具体 capability 的 read model、Hook 或 SDK mapper；
- 只有 typed committed metadata/receipt 能触发 capability handler；
- `is_error == false`、普通 success 文本或 synthetic outcome 不能推断 commit；
- 每个已登记真实 commit 最多 dispatch 一次；
- capability handler 完成前，对应互斥 mutation lane 不释放；
- committed observation 与 conversation materialization 相互独立；
- 新增有 SDK-visible durable state 的 mutation capability 必须登记 handler 或记录明确不适用理由。

### Published State Registry

- Registry 保存 Published State，不拥有领域状态机；
- family revision 相互独立；
- business change 才增加 revision，heartbeat 只增加 sequence；
- new/resume/reset/session switch 建立或清除 epoch；
- heartbeat 只读取 Registry，不临时跨 BC 查询和拼装。

### Task

- `TaskCommandResult<T>` 仍是 Task BC 原子事务结果；
- `CommittedTaskChange` 仍是内部 metadata；
- SDK 发布完整 state，不发布 delta；
- 每个真实 commit 最多 query/publish/hook 一次；
- failed/no-op/synthetic result 不发布、不触发 Hook；
- provider order 与 LLM batch materialization 不变；
- Resume 只发布一份恢复后的完整 Task state。

### Compact operation progress

- `completed` 只表示对应工作已完成且结果已被当前 operation 接纳；
- 创建 future、排队、获取 semaphore 或开始执行不增加 completed；
- 并发 chunk 可按完成顺序推进计数，但摘要结果必须按原 chunk index 恢复确定性顺序；
- generating、mapping、reducing、refreshing、finalizing 是 typed operation stage，不与 Context utilization 或 auto-compact threshold 混用；
- 未知总量显式 indeterminate，算法上界显式 bounded，禁止伪造精确 total；
- operation identity 与 revision 在 Context→Runtime→SDK→TUI 无损；
- automatic、provider-context-retry、manual 路径共享 stage mapping 与 Activity terminal cleanup；
- success/skip/fallback/error/cancel/timeout 每条路径都必须结束 Running/Waiting compact Activity；
- TUI presentation ratio 单调，terminal 后不再展示，stale observation 不回退旧进度。

### Status

- snapshot 是 presentation read model，不是终态或 compact authority；
- business change 立即发布，heartbeat 只重发；
- TUI 不自行计算 compact threshold；
- current 与 pending config 不混淆；
- session identity/revision 防止 stale 覆盖；
- Lifecycle/Activity/Task 各自保持单一权威来源；
- 心跳 task 随 session/stream 生命周期停止。

## 10. 风险与取舍

### 最小补丁

- Task：在现有 `execute_non_agent` 后立即发一次状态；
- Status：把 TUI 公式改为 `(input + output) / context_size`，compact 后清零；
- Compact：仅把 chunk progress emit 从 future 创建位置移到 `buffer_unordered` completion loop。

优点是改动小；缺点是漏 streaming/approval、revision/state 仍可能错配，Status 仍不等于 Context 决策口径；Compact 虽不再把调度当完成，但 reduce/refresh 仍无结构化进度，UI 仍会在 map 阶段上限长期停留。只适合作为短期止血，不应作为最终实现。

### 根因方案（本计划）

- 通用 committed side-effect dispatcher + capability-specific handlers；
- 所有已登记 mutation execution paths 共享 observation seam；
- 每个 capability 的 commit/query/publish 位于正确的互斥 lane；
- Runtime-owned Published State Registry；
- Task 完整状态与 Status snapshot 在同一批次完整接入；
- dirty immediate publish + heartbeat convergence；
- Context-owned compact committed-work stage model，generating/map/reduce/refresh/finalizing 无损发布；
- automatic/provider-retry/manual compact 共用 progress/terminal contract；
- SDK/TUI 仅原子替换完整 DTO，并将 Context budget 与 operation progress 分离展示。

成本更高，涉及 Tools/Runtime/Context/Config/Project/SDK/TUI/guards/docs；优势是 ordinary、streaming、approval、Resume、非 Tool Runtime facts 和未来新增 committed capability 共享稳定边界，避免同类延迟和语义漂移复发。本计划不接受“Task 专用回调 + Status 临时心跳”的部分实现。

## 11. 完成定义

只有同时满足以下条件才算完成：

- 通用 committed side-effect dispatcher 已落地且不绑定 Task；
- 所有现有 typed committed mutation capability 已完成分类并接入 handler 或记录可验证的不适用理由；
- ordinary、streaming、approval、cancellation-preserved execution 均不能绕过 observation seam；
- 同一 round 的每个真实 Task commit 都按 revision 立即可见；
- LLM Tool Results 仍按原始顺序整批提交；
- Published State Registry 为 Task 和 Status 提供独立 versioned full-state delivery；
- Status Line context 指标与 Runtime compact decision 同源；
- Compact map completed 只在 chunk 完成后推进，调度/入队不推进；
- generating/reduce/refresh/finalizing 使用 typed stage/work，并在 Runtime→SDK→TUI 全链路无损；
- automatic、provider-context-retry、manual compact 的 success/skip/fallback/error/cancel/timeout 均可靠结束 Activity；
- TUI 明确区分 Context utilization、auto-compact threshold 与 stage-weighted operation progress，不再长期停在由提前 `N/N` 造成的 85%；
- 当前/待生效配置、workspace 及运行状态由完整快照一致展示；
- 业务变化立即发布，心跳能幂等重发并在退出时停止；
- Live/Resume/new Session/reset 行为一致；
- 三条 Runtime 事件语言权威规则未被破坏；
- 每一层测试和架构 guard 均通过；
- PR #1541 已更新但未合并。
