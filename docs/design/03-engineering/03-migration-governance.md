# 迁移治理 · Current → Target 追踪

> 层级：03-engineering（横切工程）
> 状态：过渡追踪｜Milestone：v0.1.0｜对应 Issue：#743 / #761（S2 盘点）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> **本文是 Current → Target 差距、迁移责任、进度与退出条件的唯一真相源**。01-system / 02-modules 设计文档只写目标态；已启用守卫的脚本行为、常量与白名单以 [Architecture Guards](01-architecture-guards.md) 为真相源；开发者当前 **MUST** 遵守的 Project 操作约束见 [`specs/project.md`](../../../specs/project.md)。
>
> **#1021 Guard 例外治理基线（2026-07-19）**：`.agents/architecture-guard-registry.json` 已成为 Guard policy / scope / suppression / migration exception 的单一机器可读注册表；`check-guard-registry.sh` 通过 xtask 校验 stable id、必填归责、path stale、Shell 隐式排除引用和仓库/模块预算。#883 删除 Storage-owned `memory_store` / `task_store` 后，Current migration debt 降为 repository `6`：Runtime `5`（4 个层间倒置 + 1 个 shared-adapter bridge）、TUI `1`（#947 承接的 slash async dispatch），Storage `0`。Storage 的 façade/Cargo edge、Composition 唯一装配点均归 Target policy，不计债务；Workflow、Audit、Project 机器报告 migration exception 为 `0`，与人工基线一致。该注册表只治理 Current 例外，capability-first 正式 Guard 与 legacy COLA 退役仍由 #1022 承接。

## 1. 代码组织、装配与守卫 Current → Target（#972）

| # | Current | Target | 责任与退出条件 |
|---|---|---|---|
| O1 | Runtime、Context、Provider 与 Policy 已完成经批准的 Hexagonal 目录迁移；Policy #917 已建立 Policy-owned PL/Port 与唯一 AllowAllPolicy，并恢复真实 `domain + adapters`；#918 已完成 Main/Sub 生产消费切换并退役 Runtime/Tool 的 allow_all 业务传播；Storage 已由 #883 删除 `memory_store` / `task_store`，只保留 `domain + ports + adapters` 的物理机制；Config 已由 #999 将 shared kernel 内纯模型/策略归入 `config/domain`、外部来源与路径解析归入 `config/adapters`，但 active service 与语义接线仍待 #933–#935；其余普通 feature 仍受迁移期固定层目录约束；完整脚本行为与常量见 [Architecture Guards](01-architecture-guards.md) | 按 [代码组织规范](../01-system/06-code-organization.md) 的 Hexagonal 默认依赖方向收敛，按 seam 使用最小必要层；`capabilities/` 仅在独立能力证据与配套 Guard 成立时启用。各模块 Target 判定见 [模块目录结构决策](../02-modules/README.md#目录结构决策) | Policy [#986](https://github.com/rushsinging/aemeath/issues/986) 删除固定 COLA 层并建立精确 crate-root façade；[#915](https://github.com/rushsinging/aemeath/issues/915) 将 warning assessment 归回 Context；[#916](https://github.com/rushsinging/aemeath/issues/916) 将 path/Bash/read-before-write guard 归回 Project/Tool；[#917](https://github.com/rushsinging/aemeath/issues/917) 建立 PolicyRequest/Decision/Port 与 AllowAllPolicy；[#918](https://github.com/rushsinging/aemeath/issues/918) 由 PR #1199 完成 production consumption、Main/Sub 同一 PolicyPort 传播与 Runtime/Tool allow_all 业务传播退役。Storage [#883](https://github.com/rushsinging/aemeath/issues/883) 已删除 `memory_store` / `task_store`，将 legacy Memory source 归 Memory adapter、Task snapshot 解释权归 Task codec，并移除 Storage migration debt；`tool_result` 已由 #884 退役。退出时 Guard **MUST** 证明层间单向依赖、domain 零物理 I/O、adapter 类型不进入 PL、crate-root façade 保持窄。其余 feature **MUST** 在 #743 原生树内由模块 leaf 独立迁移；正式 capability-first Guard 与全局故意违规证明归 [#982](https://github.com/rushsinging/aemeath/issues/982) / #1022 |
| O2 | `check-cola-layer-purity.sh` 仍在 Stop 阶段运行；其检查行为、常量与白名单见 [Architecture Guards](01-architecture-guards.md)。#1002 已确认 Composition Root 当前 `app/provider/runtime/tools/update` 扁平 wiring modules 符合 capability-first；#948 已让 `FeatureGateways` 的 Provider/Tool factory 注入真实进入 Runtime 主 bootstrap，并由 `check-composition-layout.sh` 的零例外正向断言锁定；Composition 不机械复制 feature crate 的 Hexagonal/COLA 层 | 由守卫机械验证 capability-first 新规范的窄公开面、跨 feature 依赖、循环依赖与 Composition Root 装配；Composition 继续按装配职责扁平分片，语义接线完整性不由目录形状冒充 | [#982](https://github.com/rushsinging/aemeath/issues/982) 是 #763 的原生实现 leaf；替代守卫证据齐备前 legacy guard **MUST** 保持运行。#948 只闭合既有 Provider/Tool gateway 的主 bootstrap 消费；Tool 双端口生产切线现已由 #911 完成，但全部 Adapter 构造上移仍由 #950 承接，Provider P1/#907 与正式 capability-first 边界 #1022 继续开放 |
| O3 | `WorkspaceService::new(cwd)` 内部选择 `GitCli`，`with_git(cwd, git)` 作为测试注入特例；`WorkspaceService`、`GitCli`、`GitWorktreeOps` 当前经 Project API 间接暴露；写用例持 `WorkspaceState` lock 执行 Git I/O，且缺少统一 control-operation 串行器 | `WorkspaceService` 只保留 crate-private 注入构造；`wire_production_workspace(cwd)` 是 composition-only opaque factory，在 Project 内构造私有 `GitCli` 并返回 `WorkspaceWiring`。每个 context 只有一个同步 state slot 与一个同步 control-operation mutex；fallible I/O 不持 state lock，成功后一次提交完整 candidate。Project factory 只负责私有构造，**NEVER** 读取全局配置或选择候选实现 | [#892](https://github.com/rushsinging/aemeath/issues/892) 收敛 Project 目标目录、私有 Git seam、opaque wiring、锁模型与 fork 隔离；[#893](https://github.com/rushsinging/aemeath/issues/893) 完成 Composition 唯一消费点、scope 生命周期和窄 view 切换；[#894](https://github.com/rushsinging/aemeath/issues/894) 提供 identity / NonGit / snapshot / restore。#892 的退出证据 **MUST** 覆盖所有写用例共享串行器、Git I/O 期间读者可观察完整旧 state、失败零部分提交、父子锁隔离，以及 Project 公共面零 `WorkspaceService` / `GitCli` / `GitWorktreeOps`；factory/handle 仅 Composition 消费的跨 crate 机械守卫与故意违规证明归 [#982](https://github.com/rushsinging/aemeath/issues/982) / #1022 |
| O4 | Runtime 已有未接线的 `core/ports/workspace_port.rs` 骨架；生产链的 `RuntimeHandle`、`ChatLoopContext` 与 `ToolExecutionContext` 仍持有或转发具体 workspace；当前启动构造的 workspace 跨回合复用 | 删除 Runtime `WorkspacePort` 与 RuntimeContext workspace 字段；Composition 为 active Main session slot 保留跨 Main Run / resume 复用的私有 `CompositionWorkspaceScope`，只在 Main agent 启动时建立 production wiring；Sub 从父 scope 派生 Run-lifetime 隔离 wiring，再把同一实例的窄 view 装配给 Context / Tool backing implementation | [#893](https://github.com/rushsinging/aemeath/issues/893) 负责 Runtime / Tool / Composition 消费方切换与 Main scope 生命周期；完成时 **MUST** 证明 Run N 的 cd / worktree 状态进入 Run N+1，并删除占位 port、旧具体引用与第二状态源。边界守卫实现归 #982，#763 汇总验收 |
| O5 | #910 已将 `ExecutionScope` 固定为八个纯值字段、`ToolExecutionContext` 锁定为私有 `scope + ports`，删除旧资源总包，并把 `WorkspaceViews` 转换移到 Runtime adapter；统一 `WorkspacePorts`（Read+Control+Persist+Isolation）已退役，Runtime 自持 Persist，Control 已按 Bash / EnterWorktree / ExitWorktree constructor 注入，context 不再广播 Control。Agent dispatch 的其余兼容 access 仍随 context 存在 | Composition 按 Tool 实例注入 Project-owned view：只读文件 Tool 只有 `WorkspaceRead`，Bash / EnterWorktree / ExitWorktree 才同时获得 `WorkspaceControl`；Tool **NEVER** 接收 `WorkspaceService` 或 `WorkspaceWiring` | [#893](https://github.com/rushsinging/aemeath/issues/893) 继续完成逐 Tool constructor 注入和测试；[#982](https://github.com/rushsinging/aemeath/issues/982) **MUST** 用故意违规证明第四个 Control 消费者与全 Scope 广播均被拦截。#911 已完成 Catalog/Execution 双端口与 typed suspension 边界；#877/#878 完整 Interaction 状态机、#912/#913 ownership/装配收口仍未完成；#897 正式 `MemoryPort` 已替代临时 Memory compatibility bridge |
| O6 | TUI `UiEvent` 仍携带多种 SDK DTO 与 AskUser `oneshot::Sender`；AskUser 在第二层 ACL 返回空 mapping 后由 `ui_event.rs` 直接写 Model / input 并发送 reply；workspace mapper 同步执行 git；部分 mapper / reducer 可直接产生或执行 Effect；View / Model 尚有重复与越权写入面 | 唯一链路为 SDK event → `event_mapping` TUI DTO → `AgentEventMapping` intents → reducer Change → Coordinator Effect → effect runner → result Intent。Runtime 生成 interaction request id 并保有 waiter / continuation；SDK event 只携可序列化纯值，TUI Effect 经 AgentClient reply / cancel command 回传且全树零 sender / registry。command result 不推进 Run，Run 恢复 / 两阶段取消只投影 SDK 权威事件；六 Context 核心字段私有且 reducer 唯一写；结构化 Conversation 与 timeline 是原子维护的互补投影；Workspace metadata 由带 root + revision 的异步 Effect 回填 | Runtime / SDK identity 与 HardPause 归 [#874](https://github.com/rushsinging/aemeath/issues/874) / [#878](https://github.com/rushsinging/aemeath/issues/878)；TUI [#943](https://github.com/rushsinging/aemeath/issues/943) / [#944](https://github.com/rushsinging/aemeath/issues/944) / [#947](https://github.com/rushsinging/aemeath/issues/947) 的精确责任与退出条件见 §1.2；全局守卫实现归 #982 |
| O7 | Task restore 当前不校验，并依次替换四个独立 async-mutex state；Project 只校验当前 root / path 存在后修改 live state，未完整校验 frame / repo；Config 的 global current / watch、Memory 打开与 Session 恢复缺少统一切换协议；旧 Workspace snapshot 缺少稳定 `WorkspaceId` / `ProjectIdentity`，跨项目 resume 可能继续沿用启动 identity | Task 使用 Task-owned `TaskId` / `BatchId` 与不含派生 `blocks` 的 `PersistedTask`，并把全部字段收进单一同步 `TaskStoreState` slot；Project 以 `ProjectIdentity` / `WorkspaceId` / `WorktreeKind` 表达 Git 与合法 NonGit，并通过无副作用 prepare + 无失败 commit 恢复完整 state。resume 先取消 / join active shared lease holders，调用栈自身不持 shared lease，再取得 owned exclusive session-switch lease；读取 Session、Project → Config → Memory → Task 的 prepare、durable commit 与最终 publish 全部在同一 lease 内完成，Config watch 最后发布 | [#890](https://github.com/rushsinging/aemeath/issues/890) 提供 Task 强类型 PL、单一 state slot / token、删除边清理与 snapshot round-trip；[#894](https://github.com/rushsinging/aemeath/issues/894) 独占 Project identity、NonGit、完整 path / frame / repo 校验、snapshot/prepare/commit 与旧 Session 兼容，**NEVER** 由 #892 复制临时协议；[#893](https://github.com/rushsinging/aemeath/issues/893) 把 Project persist view 接入 Composition/Context backing；[#871](https://github.com/rushsinging/aemeath/issues/871) 实现联合协调器、participant 与唯一 exclusive session-switch gate，Project **NEVER** 自建或声称持有该 gate；[#933](https://github.com/rushsinging/aemeath/issues/933) 定义 ConfigQuery / ConfigWriter delivery seam。退出证据 **MUST** 覆盖 shared → exclusive 升级为零、每个 prepare / durable await / publish 注入失败或取消点、任一 prepare 失败时全状态不变、跨项目恢复后所有消费者只读写目标 backing、Config watch 不早于 backing install、prepare token 与 commit 之间无外部 mutation，以及整个切换窗口不可被 Main Run、query、subscribe 或命令观察 |
| O8 | Memory / Storage / Prompt / Workflow / Interaction / Config 的 Target 文档已有局部方向，但部分 leaf 正文未冻结 revision CAS、typed committed receipt、async materialization、ReasoningPort OHS 与 SDK interaction command | Memory mutation 采用 candidate + dataset CAS + committed receipt；Prompt 只经 Context-private async pipeline 与 supplier seams；Workflow graph 只经 ReasoningPort observe/current；Runtime interaction identity / waiter 权威且 SDK/TUI 只交换纯值；Config 只经 project-aware participant 与 AgentClient delivery | Memory [#895](https://github.com/rushsinging/aemeath/issues/895)–[#900](https://github.com/rushsinging/aemeath/issues/900) / [#984](https://github.com/rushsinging/aemeath/issues/984)，Storage [#880](https://github.com/rushsinging/aemeath/issues/880) / [#882](https://github.com/rushsinging/aemeath/issues/882) / [#983](https://github.com/rushsinging/aemeath/issues/983)，Prompt / Skill / Git [#870](https://github.com/rushsinging/aemeath/issues/870) / [#912](https://github.com/rushsinging/aemeath/issues/912) / [#894](https://github.com/rushsinging/aemeath/issues/894)，Workflow [#919](https://github.com/rushsinging/aemeath/issues/919)–[#921](https://github.com/rushsinging/aemeath/issues/921)（**#921 收缩：Provider resolver 领域迁移完成但未接生产链路；Config `reasoning_graph` 退役；五节点固定默认 effort；是否保留/接线由 v0.2.0 #1142 决策**），Interaction [#874](https://github.com/rushsinging/aemeath/issues/874) / [#878](https://github.com/rushsinging/aemeath/issues/878) / [#911](https://github.com/rushsinging/aemeath/issues/911)，Config [#871](https://github.com/rushsinging/aemeath/issues/871) / [#933](https://github.com/rushsinging/aemeath/issues/933) / [#934](https://github.com/rushsinging/aemeath/issues/934) 承接。**每个能力只有在以下可验证证据齐备后退出 O8**：唯一 owner / OHS 签名已在对应 Target 文档冻结；leaf PR 附契约或场景测试覆盖成功、pre-commit 失败、post-commit warning/取消竞争等其适用分支；旧 public path / duplicate trait / 第二状态源已删除；#982 对该边界的正例与故意违规反例均通过；父 Issue 和 Release Gate 已同步。任一能力未满足时 O8 保持未完成，#972 本身不承载代码 PR |
| O9 | #885–#890 已建立 Task-owned 聚合、严格状态机、原子 DAG / Batch、单一 `TaskStoreState + TaskRevision` backing、`TaskAccess` / `TaskPersist` 能力分离与版本化 snapshot；#883 已删除 Storage-owned `task_store`。#891 删除 Shared Kernel 的重复 Task DTO/lifecycle 与 Tools legacy ACL，Tool result 改由 Task-owned `TaskView` 发布，V1 数字 ID / 0 哨兵兼容只留在 Task-owned codec adapter；Task crate 已物理退役 `business/core` 并落到 `domain + adapters` Target，Runtime/Tools 生产代码禁止直接命名具体 `TaskStore`。Runtime 中 legacy `TaskPort` / `TaskStorePort` 的退役仍由 #879 承接 | Task BC 独占 Published Language、聚合、执行时间事实与 lifecycle 领域策略且不依赖 Agent 身份；正式 backing 只持一个同步 `TaskStoreState + TaskRevision` 状态槽，每次实际成功 mutation 原子提交 state、稳定 events 与单调 revision。Runtime / Tool 只持 `TaskAccess`，Context Management 只持同 backing 的 `TaskPersist`。v0.1.0 不建立 `TaskId ↔ AgentId` / `TaskAssignment`；LLM 决定执行方式，未来只有出现可验证的调度、取消或审计需求时才由独立 Runtime Issue 设计。legacy ID 重用、第二状态源、owner DTO 与任意 update closure 最终全部退役 | [#889](https://github.com/rushsinging/aemeath/issues/889) 完成 Tool/Runtime Access ACL、typed subject/description 命令、直接 Pending→Completed 单提交与 owner 停止消费；[#890](https://github.com/rushsinging/aemeath/issues/890) 完成同一 backing 的 `TaskPersist`、Session snapshot source/restore adapter 与 legacy persistence 切换；[#891](https://github.com/rushsinging/aemeath/issues/891) 删除 Shared/Tools 兼容双轨并注册零 migration exception 的 Task ownership、能力分离与 crate-root façade policy；[#877](https://github.com/rushsinging/aemeath/issues/877) 承接统一 `tool_coordination` 事件投影；[#879](https://github.com/rushsinging/aemeath/issues/879) 退役 Runtime 重复 `TaskPort` / `TaskStorePort` 与旧生产入口；[#1058](https://github.com/rushsinging/aemeath/issues/1058) 在业务叶子完成后核验 L0–L5 测试完整性 |
| O10 | #998 已抽出独立 Workflow crate；#919 冻结节点/effort 语义；#920 已将 Main 收口为 Workflow-owned adaptive `ReasoningPort`，删除 Runtime 重复 trait、graph 直接持有和 `session_reasoning` 第二状态源 | Main 只经 observe/current/set 消费 Workflow requested；统一 RuntimeContext 中 Sub 使用 Fixed/Inherit/NoOp；Provider 独占 model capability clamp | #875/#878 落地 Sub Port 与 shared Loop 接线，#879 删除 legacy 入口；**#921 收缩范围：Provider resolver 领域迁移完成但未接生产链路；Config `reasoning_graph` 退役（含 `max_reasoning`），五节点固定默认 effort；requested→effective clamp 链未端到端接线；是否保留/接线由 v0.2.0 #1142 决策**。完整 Workflow Engine 属于 v0.2.0 |

#972 只对齐设计文档、责任映射与 issue 结构，**NEVER** 修改目录实现、守卫脚本或开始上述迁移。O1–O10 列出的执行 leaf **MUST** 在开工前以本表 Target 同步正文与父 Issue；迁移期固定层级守卫在 #982 替代守卫证据齐备前 **MUST** 保持运行。

### 1.1 Legacy guard 替换退出证据

legacy guard 替换的退出证据 **MUST** 包括：

1. 新守卫已注册到 `check-architecture-guards.sh` 编排与 [Architecture Guards](01-architecture-guards.md) 守卫索引。
2. 故意制造违规时，新守卫以 exit 2 阻断并命中预期规则。
3. 恢复合规状态后，单守卫与完整 `check-architecture-guards.sh` 编排均 clean pass。
4. 本文与 [Architecture Guards](01-architecture-guards.md) 已同步，legacy 引用与白名单已清理。

上述证据未齐备时，现行迁移期守卫 **NEVER** 替换或退役。

### 1.2 O6 TUI 单向事件链责任与退出条件

从 Target 文档移出的 Current 清单集中如下；后续代码盘点发现新旁路时 **MUST** 只增补本表，**NEVER** 回写 `02-modules/tui/`：

| # | Current | Target | 承接 |
|---|---|---|---|
| TUI-1 | SDK / Runtime 事件存在手工转换与类型擦除；UiEvent 直接携带多种 SDK DTO | Published Language 单一来源；第一层 ACL 转为 TUI-owned DTO，UiEvent 之后零 SDK DTO | #943 转换；#947 退役旧 DTO / convert 路径 |
| TUI-2 | AskUser 在第二层 ACL 返回空 mapping，`ui_event.rs` 直接写 Model / input 并持有、发送 `oneshot::Sender`；reply / cancel 完成可与 Run resume / cancel 状态混淆；ToolApproval / PlanApproval / HardPause 无 TUI 闭环 | Runtime / SDK 先生成并注册 InteractionRequestId；processing 穷尽转换 UserQuestions / ToolApproval / PlanApproval / HardPause 纯值 body；DTO / UiEvent / Intent / Model 持 TUI-owned 无损 id；typed reply / cancel 严格走 Change → AgentClient Effect → result Intent，result 只结束 Interaction 块；仅 SDK `RunResumed` 恢复 Run，Interaction cancel 不取消 Run；并发 Tool suspension 由 Runtime 稳定串行发布 | #874/#878 提供 Runtime identity、waiter、continuation 与 command；#943 四类 DTO / 权威事件转换；#944 reducer / AgentClient effect / 状态机；#947 退役全部 TUI sender / registry / 旁路 |
| TUI-3 | `AgentEventMapping` 可混合 Intent / Effect，reducer / update 路径仍可直接执行 runtime、副作用或依赖调用顺序；取消 accepted / terminal 未在投影中严格分离 | mapper 只产六 Context Intent；reducer 只产 Change；Coordinator 只产 Effect；runner 执行并回传 result Intent；SDK `RunCancelling` 投影非终态 Cancelling，只有 `RunCancelled` 投影终态 | #943 穷尽转换；#944 闭环与两阶段状态机；#947 退役旧 Effect 路径 |
| TUI-4 | `WorkingDirectoryChanged` mapper 同步执行 git 补 branch / worktree kind，阻塞 ui_rx | ACL 只产 WorkspaceSnapshot；root + revision Change 驱动异步 metadata Effect，陈旧结果丢弃 | #943 DTO；#944 Effect；#947 同步路径退役 |
| TUI-5 | spinner 业务态在 Model / ViewState / animation 多处同步；`update_ui` 混合 tool、spinner、AskUser、session cwd 与 dirty marking | Run / RunStep 投影是业务事实；ViewState 只存 scroll / selection / collapse / animation / cache 等瞬时状态；各 Context reducer 与 ViewAssembler 各守单一职责 | #944 Model / Change；#947 旧同步 helper 退役 |
| TUI-6 | Conversation 结构化 `chats` 与 timeline 缺重叠事实 invariant；timeline 还包含 system / hook / error / AskUser 等结构化状态无法重建的事实，queued / progress 等重复投影也无关联证明；核心字段公开，resume、Chat / Run 术语及 Config / Workspace / Task 投影仍有越权写或双重真相 | 结构化 Conversation 投影（runs / queued / progress）与 `timeline` 是同一 reducer 事务原子维护的互补投影，只约束重叠稳定 ID、相对顺序、关联与终态；六 Context 核心字段私有，reducer 是唯一 mutation facade 调用方；resume 进入 Completed，六 Context 独立投影 | #944 私有化 / reducer / 互补 invariant；#947 旧字段、调用点与术语退役 |
| TUI-7 | view_state 反向依赖 render；Input render model 与 ViewModel 重复；`follow_tail_hint` 无消费方；collapse 无输入闭环；QueuedUserMessage 被丢弃；BlockCache 无界；ToolResult display data 未参与 cache version；存在 no-op event / effect、无调用模块、临时全局 `allow(dead_code)`，且视图层门禁不完整 | ViewAssembler → ViewModel → Render 单向依赖；ViewState 只含瞬时交互态；queued / collapse / cache invalidation 均有封闭 Target 契约；缓存有容量上限；无重复模型、死字段、静默 event、no-op 变体或全局 dead-code 豁免；视图门禁全部可执行 | #947 统一退役、补闭环并启用守卫 |
| TUI-8 | sub-agent progress 缺稳定 agent_id，部分 ToolOutput 被静默忽略 | Main/Sub 事件带 AgentId 并嵌套路由，所有进度变体显式映射 | #612 产品能力；#943 只保证 ACL 不静默丢弃 |
| TUI-9 | `tui.rs` façade 曾 re-export `InputArea` / `OutputArea` / `StatusBar` 渲染 widget，交付入口暴露 render 实现细节；`app/`、`view_model/`、`view_state/` 仍是语义迁移期物理目录 | façade 只发布 `App` 入口；TUI 保持按 TEA 管线技术目录组织，**NEVER** 按六 Context 建 `capabilities/` 业务竖切；`app/`、`view_model/`、`view_state/` 的最终收敛只随 #944/#947 语义迁移完成 | #1001 已收紧 façade；#944/#947 负责语义收敛，#1022 负责正式 capability-first Guard |

| Issue | Current → Target 责任 | 必须具备的退出证据 |
|---|---|---|
| [#943](https://github.com/rushsinging/aemeath/issues/943) | 把第一层转换收口到 `adapter/event_mapping.rs`，将全部 SDK event / DTO（含 Runtime-owned `InteractionRequestId`、`RunResumed` / `RunCancelling` / `RunCancelled`）穷尽转换为 TUI-owned `UiEvent`；processing 只转发纯值 event，**NEVER** 生成协议 id 或注册 sender；第二层 `adapter/agent_event.rs` 按 Conversation / Input / Diagnostic / Session / Config / Workspace 六个 Context 显式产出 Intent | 每个 SDK event 与 UiEvent 变体都有转换 / mapping 单测；interaction id 可无损映回 AgentClient command；Run resume / 两阶段取消保留 run identity 与事件语义；整个 TUI 零 SDK channel/sender/pending waiter，UiEvent / Intent / Model 零 `sdk::*` DTO；禁止 wildcard、默认空 mapping 与“其余见 ui_event.rs”旁路 |
| [#944](https://github.com/rushsinging/aemeath/issues/944) | 六 Context 核心字段私有，reducer 成为唯一 mutation facade 调用方并只返回 Change；Coordinator 只从 Change 生成 Effect；effect runner 调 `AgentClient::reply_interaction` / `cancel_interaction` 并把 typed outcome 变为 result Intent。四类 Interaction command result 只更新交互块，Run 只投影 Runtime 权威 resume / cancellation 事件；结构化 Conversation / timeline 作为互补投影原子维护；Workspace metadata 使用 root + revision 防陈旧覆盖 | 分层测试逐段覆盖四类 body 的 DTO → Intent、Intent → Change、Change → Effect、AgentClient outcome → result Intent，**NEVER** 只测首尾；有效 reply 恰好一次、InvalidReply 可修正重试、未知 / 重复 id 结构化失败；`InteractionReplySent` / `InteractionCancelled` 不改变 Run，`RunResumed` 才恢复 Running，`RunCancelling` 非终态且仅 `RunCancelled` 终结；两个并发 Tool suspension 按稳定顺序逐个展示且不覆盖；structured Conversation / timeline 仅对重叠 ID、顺序、关联、终态做 invariant；字段私有且非 reducer mutation 调用为零；Model 只接受匹配 request id / revision 的结果；`update/`、reducer、ACL 无 I/O / spawn / await / AgentClient 调用 |
| [#947](https://github.com/rushsinging/aemeath/issues/947) | 退役 `ui_event.rs` 的 AskUser / workspace 特判、TUI 全部 reply sender / `PendingReplyRegistry` / 本地 request-id generator、mapper 直接 Effect、同步 git、错位 processing mapper、非 reducer Model 写入、旧 `chats` / Chat 术语、静默忽略分支、重复 DTO / InputRenderModel、`follow_tail_hint`、no-op / dead event 与临时 `allow(dead_code)`；闭合 collapse / queued message / bounded cache / cache-version 契约并启用 TUI 架构守卫 | legacy 路径、sender / registry / id generator、公开核心字段与非 reducer mutation 调用零引用；ViewState→Render 反向依赖、重复 render model、死字段、无界 cache、静默 queued item 与全局 dead-code 豁免均为零；故意把 SDK DTO / sender 放入 TUI、从 ACL 生成 Effect、从 update 执行 I/O、从非 reducer 写 Model、让 ViewState import Render、增加未映射 UiEvent 时守卫均以 exit 2 阻断；恢复后定向测试、全量 TUI 测试与 architecture guard clean pass |

O6 只有在 Runtime #874/#878 与 TUI 三个 issue 的退出证据全部附于各自 PR、父 Issue 状态同步，且 #982 完成全局故意违规证明后才可标记完成。任何中间 PR **NEVER** 通过保留第二条 UiEvent 直达 Model 或 TUI-owned reply registry 路径换取兼容。

## 2. Agent Runtime Current → Target

[#700](https://github.com/rushsinging/aemeath/issues/700) / PR #823 已完成并由当前源码证明的基线，**NEVER** 再列为待迁移缺口：

| 已完成基线 | Current 证据 |
|---|---|
| Main / Sub 共享唯一 Loop Engine 与显式 `Run` / `RunId` / `RunStatus` | Main `application/chat/looping/loop_runner.rs` 与 Sub `application/agent/runner/loop_run.rs` 都调用 `application::loop_engine::run_loop`；聚合位于 `domain/agent_run` |
| Main / Sub 共享 StuckGuard | `application/loop_engine/engine.rs` 在统一入口建立 `StuckGuard`，内部复用 stall / tool fuse |
| 单一同步 `cancel_run(RunId)` 与 Cancelling → Cancelled | `packages/sdk/src/client.rs` 只发布 `cancel_run`；Runtime active-run registry、Run cancellation transition 与 SDK 两阶段事件已接线；这是 #878/#879 切换前的生产兼容基线，**NEVER** 再当作 Target |

下表只保留当前仍真实存在的结构性缺口；后续实现若改变 Current，**MUST** 在合入时同步本表：

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| R1 | **Run 控制目标域已建立，生产入口仍是旧取消语义** | PR #1036 已在 SDK/Run 聚合加入 `CancelRunStepOutcome`、`TerminateRunOutcome`、绝对 `ControlDeadline`、`DrainingInput/CancellingStep/FinalizingStep/Terminating/Terminated` 与 typed events；生产 `AgentClient` / TUI / Loop 仍只调用旧 `cancel_run`，旧 `Cancelling/Cancelled` 与 compatibility projection 仍在 | #878 原子接入 Run root + per-Step child scope、StepFinalizer、drain-or-seal 与新 AgentClient commands；#879 删除 `cancel_run`、旧状态/事件/projection 与平行 registry 仲裁。退出证据：生产只可达新命令，新旧状态无并存，Main/Sub 同一 absolute deadline 传播，最终只到 Completed/Failed/Terminated | #700 → #878/#879 |
| R2 | **RuntimeContext 三层重叠**（#456）| `ChatRuntimeContext` + `RuntimeResources` + `ChatLoopContext` + `TuiLaunchContext` 字段大量重复复制 | 单一 `RuntimeContext`（出站端口 + config + event） | S5 |
| R4 | **Runtime 出站端口已归位但骨架未接线且含多余 WorkspacePort** | `ports/` 已按六边形目标归位 Context / Tool / Policy / Memory / Workspace / Reasoning / Usage 等契约骨架，但 #873 明确未切换 legacy 路径；旧具体类型和部分历史端口仍在生产链；#995 保留 4 个精确层间迁移例外并由 guard stale 自检 | 接线真实 Runtime seam；删除 `WorkspacePort` 与全部层间迁移例外，由 active-main-session-slot composition scope 把 Project 窄 view 装配给 Context / Tool；补齐 Provider invoke、Hook dispatch 等行为 | S5（Workspace 见 #893 / #894；消费方切换见 #874–#879） |
| R6 | **共享 `Arc<LlmClient>` 隐患** | Sub 改 `reasoning_level`/`max_tokens` 靠 finalize 手动恢复，**并发 sub 互相踩踏** | 共享不可变 Transport；Main/Sub 每次 attempt 使用独立 Invocation Scope | S3/S5 |
| R7 | **Main/Sub PolicyPort 与授权上下文已统一** | #918 统一 Main/Sub `PolicyPort`；#1221 进一步让 Runtime 对每个 ToolCall 只评估一次，并把同一 `AuthorizationContext` 投影给 fuse、Tool、Project 与 permission hooks，删除 Runtime/Tool 的 `allow_all` 业务传播 | 保持 Config committed mode 为唯一真相、Main/Sub/MCP 同一 PolicyPort 与授权上下文；Future Deny/Approval 另行设计 | 已完成（#918/#1221；#1062 的身份、决策分支与 L0～L5 证据见[测试治理 §11.8](04-testing-and-coverage.md#118-1062-policy-l0l5-覆盖证据)） |
| R8 | **SDK identity / Runtime Interaction 基础契约已建立，生产路由仍待切换** | #874 已发布纯值 Interaction PL；#1245 已建立 Run-owned `PendingInteraction`/四类 continuation、Runtime-side waiter bridge 与 `AgentClient` reply/cancel 命令契约。bridge 尚未注入 Main/Sub 生产 adapter，旧 `AskUserBatch.reply_tx` 仍可达 | #1246 切 Main Tool suspension；#943/#944 完成 TUI command；#1247 接 Run control drain；#1248 完成 Sub/Hook/Reasoning 装配；#879 删除旧 AskUser projection。退出证据仍要求 SDK/TUI 生产事件零 sender、Main/Sub identity 无损、projection 唯一 | #874/#1245（契约）→ #1246/#1247/#1248/#943/#944/#879（生产切线与退役） |
| R9 | **RunSpec 配置散 4 处** | `AgentRoleConfig` + `AgentTool` 硬编码 system + 名称排除型 `ToolProfile::SubAgent` + `ModelEntryConfig`(effort) | 收敛为声明式 `RunSpec`，Tool 部分采用 Registry Scope + capability Profile | S3/S5 |
| R10 | **Session `messages`/`chats` 双轨已退役** | #872 删除 live `Session` 类型与旧 writer；`messages/cwd` 只存在于 `LegacySession` reader DTO，canonical writer 只输出 `chats/workspace` | Guard 禁止 Runtime Session 内部引用，codec 契约锁定兼容 reader / canonical writer | 已完成（#872） |

## 3. Tool & Skill & Command 现状缺口（S2 代码盘点）

#993 已完成 Tool crate 的 `domain + adapters` 过渡物理迁移，#909 已落地 Registry Scope / capability Profile 与只收缩规则，#910 已冻结纯值 `ExecutionScope` 与最小 `ToolExecutionContext`。#911 进一步完成真实生产切线：Tools 以同一私有 `ToolBacking` 装配 `ToolCatalogPort` / `ToolExecutionPort`，Runtime Main/Sub 只交换 Descriptor/Invocation/Outcome，不再持有 `ToolRegistry` 或取得、调用 `Tool` 实例；Catalog 投影与 Execution 调用时复验 Scope/Profile，schema 校验实现唯一归 Tools；AskUser 返回 typed suspension，Runtime 通过独立 mapping seam 接回现有 Runtime-owned 等待路径；MCP 仅接入“注册 callable 不自动授权 Scope/Profile”的保守 source seam，不改变连接生命周期。#877 已将 Main/Sub 共用的 guard→catalog→Policy 调用准备、拒绝决策和稳定顺序回收收口到 `application/tool_coordination`，并把 Tool identity 与 ToolLoopGuard 状态迁入该目录；ToolLoopGuard 仍由 Loop Engine 驱动，UI/progress/Hook adapter 机械及 Main/Sub 不同的拒绝 outcome 投影暂留旧路径。

该完成口径不外推：#878 的完整 Interaction identity、continuation、`AwaitingUser` / resume / cancel 状态机仍未完成；#879 继续承接旧 looping UI/Hook adapter 与重复执行 helper 的最终生产退役；#912/#913 仍承接 Runtime/Composition ownership 与装配收口；#914 仍需物理删除 `legacy-no-agent`、历史 `register_all_tools*`、旧内部 Registry/Profile 与 `SkillTool`；MCP Ready 的显式连接状态机、断连撤销/refresh、稳定身份、版本与 Catalog revision 协议仍开放。

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| T1 | **Catalog / Execution 生产双端口已切线** | #911 已由 Tools composition factory 在同一私有 backing 上装配双端口；Runtime Main/Sub 生产代码只持 Catalog/Execution trait object 与 PL，零 `ToolRegistry` / `Tool` 实例消费 | 保持 Descriptor/Invocation/Outcome 唯一跨界语言；#914 再物理退役 Tools 内部旧 Registry 兼容入口 | 已完成（#911） |
| T2 | **Scope/Profile 基础已落地，历史入口待退役** | #909 已用 capability allow-set 与 `derive_restricted` 替代内部 ToolName 黑名单；#911 让 Catalog 投影与 Execution 调用时共同复验 Scope/Profile。`legacy-no-agent`、历史 `register_all_tools*`、旧内部 Profile/Registry 仍在 Tools 内 | #914 删除兼容 Scope、历史入口与无消费者旧内部实现；Profile 始终只收缩、不扩权 | #909/#911（边界完成）/ #914（物理退役） |
| T3 | **内置双端复验完成，MCP 仅保守 source seam** | 内置 Tool 已受 Scope/Profile 双端复验；动态 MCP callable 可进入私有 backing，但注册本身不会授予 Scope 成员资格或 Profile 权限。尚无 MCP Ready 生命周期、断连撤销/refresh、稳定身份或 Catalog revision | MCP Ready 建立显式连接状态机、保守 capability ACL、CatalogChanged/revision 与原子撤销/更新 | #911（保守 seam 完成）/ MCP Ready（生命周期与 revision） |
| T4 | **纯值 Scope、最小 context 与 typed suspension 已切线** | #910 固定八字段 `ExecutionScope` 和私有 `scope + ports`；#911 让 AskUser 返回纯值 typed suspension，并由 Runtime mapping seam 接回现有 waiter。Runtime 继续自持 Persist/semaphore/timeout/Policy/Hook；逐 Tool Project view 与完整 ownership 仍待收口 | 保持 Tools 零 channel/token/semaphore/Runtime handle；完成 Composition 按 Tool 实例注入与 Runtime/Composition ownership；Interaction identity/continuation/await-resume 归 Runtime 完整状态机 | #910/#911（边界完成）/ #877/#878/#912/#913、Workspace #893（状态机与装配收口） |
| T5 | **Tool 调用准备与稳定回收已收口，完整 Interaction 状态机待完成** | #911 已将存在性/Scope/Profile/schema/函数调用收进 Execution；#877 已让 Main/Sub 共用 `tool_coordination` 的 guard→catalog→Policy 准备、拒绝决策和原调用顺序回收，并迁入 Tool identity / ToolLoopGuard 状态。Pre/Post Hook 的 UI adapter、Main/Sub outcome 投影、typed suspension waiter 与旧执行 helper 仍在兼容路径 | Runtime 经唯一 `InteractionPort` 拥有 request identity、continuation、await/resume/cancel/retry；#879 删除旧 looping adapter 和重复执行入口，SDK/TUI 只交换纯值 | #911/#877（调用边界与协调策略完成）/ #878/#879（Interaction 与旧路径退役） |
| T6 | **取消接口绑定实现细节** | Tool 执行依赖具体 cancellation/channel 形态，长进程/网络调用的协作停止不统一 | Tool PL 定义只读 `CancellationSignal`；Runtime 适配 cancellation tree 并拥有 timeout | S5 |
| T7 | **Tool 结果责任混合** | Tool 字符串结果、结构化 data、错误、截断/落盘和 UI 展示边界不统一 | `ToolOutcome` 保留领域结果；token budget/截断归 Context Management，持久化归 Storage，渲染归 TUI | S5 |
| T8 | **Skill Catalog / Materialization 已切线，legacy SkillTool 待物理退役** | #912 已建立 Skill-owned `PromptFragment`、Catalog/Materialization ports 与 filesystem adapter；Main/Sub Context 按每次 live workspace/config/tool snapshot 物化并负责 scan、去重、预算和 cache block；正式 Main/Sub Tool Catalog/Execution 不再发布或执行 Skill。`legacy-no-agent` 与 `SkillTool` 文件仍为 #914 兼容范围 | 保持 Skill 不取得 Tool execution capability；#914 删除 legacy scope、历史 registry façade 与 SkillTool 物理代码 | 已完成（#912 边界）/ #914（物理退役） |
| T9 | **Slash Command Catalog / Router 已切线，目标 BC handler 继续分阶段收口** | #913 已建立 Tools-owned `CommandDescriptor` / `CommandCatalogPort` / `CommandRouterPort` 与三类 typed route；SDK 直接 re-export 唯一 PL，Composition 将同一 Catalog/Router 注入 TUI/no-TUI，动态 Skill alias 也先进入 PromptInjection Descriptor；帮助、补全、`/exit`/`/reflect` 和未知命令共用 Router，旧 `builtin_commands` / 静态帮助 / 独立 parser 已删除。现有 Runtime `ChatInputEvent → PendingCommand` 仍作为目标 BC handler adapter 执行 ApplicationControl，不再承担 slash 名称发现或解析 | 保持 Tools 为 Command PL/目录/路由唯一 owner；PromptInjection 只物化 Skill-owned PromptFragment，SnapshotQuery/ApplicationControl 的最终 typed Snapshot/Outcome handler 随各 owner Issue 收口；#947 完成 TUI Effect 化，#878/#879 退役旧 Loop helper | #913（Catalog/Router 完成）；#947（TUI slash I/O 全 Effect 化）；#878/#879（handler/Loop 退役）；#914（legacy 物理退役） |
| T10 | **MCP 生命周期为隐式 Manager** | 连接状态由多个方法散点修改；health check、tool list diff/refresh 与 resource 路径未完整接线 | 显式 `McpConnection` 状态机；仅 Connected 发布 Catalog 投影，变化原子撤销/更新 | MCP Ready 后 |
| T11 | **MCP Tool Catalog 一致性不足** | disconnect 后目录撤销、动态上下线、annotations capability 映射及事件通知未形成统一契约 | MCP ACL 转 Tool PL；CatalogChanged 通知重新拉取 Snapshot；连接/投影一致 | MCP Ready 后 |
| T12 | **MCP 稳定身份与版本未定** | 动态工具尚未形成可验证的稳定 ID、schema 版本和 Catalog revision 协议 | MCP 正式接线时单独设计 ToolId、rename、版本与 in-flight 兼容；当前不预设 | MCP Ready 后 |

### 3.1 Command #913 开发前切线与验证矩阵

#913 只建立 Command Catalog / Router 与三类机制的生产边界，**NEVER** 把 Tool、Skill、Command 合并为统一执行 trait，也不替目标 BC 定义通用结果基类。唯一签名真相仍是 [Tool & Skill & Command 端口](../02-modules/tools/02-ports-and-lifecycle.md#7-slash-command-端口)；本节只记录 Current → Target 的迁移证据。

| 迁移面 | Current | #913 Target / 退出证据 |
|---|---|---|
| 发现、帮助、补全 | SDK `builtin_commands()` 元组与 TUI `SLASH_HELP_LINES` 双真相；skill alias 另行拼接 | Tools-owned Descriptor/Catalog 唯一来源；SDK、TUI 帮助、补全与 alias 投影字段一致，旧静态清单零生产引用 |
| 解析与分类 | TUI 大型 `match`、no-TUI `/exit`/`/reflect` parser、Runtime `PendingCommand` 分别识别命令 | 所有入口先经同一 Router 得到 `PromptInjection` / `SnapshotQuery` / `ApplicationControl` typed route；未知命令返回 typed error 且不创建 Run |
| PromptInjection | TUI 将 skill 内容拼成字符串并作为普通 UserMessage 提交；`/review` 只有帮助/测试残留 | 复用 Skill-owned `PromptFragment`，Context 独占注入顺序、预算与去重；无 owner 的命令不伪造语义 |
| SnapshotQuery | usage/status/config/stats/context 直接读取 TUI model/config view；reflection 参数重复解析 | handler 只调用目标 BC Query Port并保留 typed Published Snapshot；CLI/TUI/no-TUI 只做展示 ACL |
| ApplicationControl | `ChatInputEvent` 与 Runtime `PendingCommand` 承担第二次业务路由，参数多为原始字符串 | Catalog schema 解析参数；handler 调目标 BC 应用 Command Port并保留 typed Outcome；迁移期 PendingCommand 若存在，不再解析命令名 |
| 结果与展示 | Runtime `idle_commands.rs` 混有 emoji、英文终端文本、`[action:*]` / `[confirm:*]` 控制字符串 | 业务层返回 owner PL/Outcome；terminal formatting 与交互映射留在 delivery ACL，特殊字符串协议零生产依赖或登记精确承接项 |
| 装配与依赖 | Composition 无 Command wiring；CLI 依赖 SDK 静态函数 | Composition 装配唯一 Command capability；CLI 继续只依赖 `composition + sdk`，不直连 Tools |
| 防退化 | `migration.tui.tea-slash-dispatch` 允许 TUI slash 异步 I/O，尚无 Command 唯一真相守卫 | L0 Guard 禁止交付层恢复 builtin 清单/业务 parser、禁止 Runtime 定义 Command PL 副本；#947 承接 Effect 化与该迁移例外最终删除 |

验证按 [测试架构](04-testing-and-coverage.md#2-六层测试模型) 分层：L1 覆盖名称、alias、schema、typed error 与机制映射；L2 覆盖 Catalog/Router 和 handler→fake port；L3 覆盖 PL/Port、SDK/Composition 投影；L4 覆盖三类用户旅程、alias 一致性、未知命令不触发 LLM 与 CLI/TUI 等价结果。#913 不新增真实 PTY、平台或发布资产职责，因此不新增专属 L5，以既有 CLI smoke 作为系统层回归证据。

Out-of-scope 必须保留精确 owner：#947 承接 TUI slash I/O 全 Effect 化，#914 承接 Registry/Profile/SkillTool 兼容物理退役，#878/#879 承接 Interaction/共享 Loop 旧路径退役，#740 承接 `/model` 与 `/resume` 动态补全数据源；MCP Ready 与 Server/WSS command transport 不属于 #913。

## 4. Provider 现状缺口（S2 代码盘点）

#901 冻结了 Runtime-owned `ProviderPort` 与中立 Invocation Published Language；#992 将 Provider 物理结构迁为 `domain/`、`ports.rs`、`adapters/` 与 crate-root 窄 façade。#902 完成不可变 Invocation Scope 生产切线，#903 完成 pull-based `InvocationStream` 跨 BC 切线，#1033 收敛单 attempt HTTP 机械，#904/#906 完成 Driver ACL、RawUsage 与 capability seam。#905 已将跨调用 retry/backoff、stream→non-stream fallback 与错误分类所有权迁至 Runtime。#907 完成最终 Adapter 收口：Runtime 退出具体 client/pool/driver，Composition Root 独占构造，wire DTO 与 `InvocationSink` 留在 Provider adapter 私有边界，legacy callback/setter/pool 路径物理清零。因此 P1-P7、P9、P12 均已关闭；P8/P11 中 resolver 生产接线仍按 v0.2.0 #1142 延期，P10 的 Runtime/Audit 关联仍由后续链路承接。#1061 的 L0-L4 审查矩阵与新增共享 stream contract 证明父项 #852 当前范围内的 Provider 行为闭合，L5 因无真实进程、平台、安装或发布资产职责而不适用。

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| P1 | **Runtime 依赖具体 client/pool（已对齐 #907）** | Runtime Main/Sub/Reflection/Compact 只消费 Runtime-owned `ProviderFactory` / `ProviderBinding` / `ProviderPort`；具体 client/pool/driver 只经 `provider::composition` 由 Composition Root 构造 | 保持具体构造面不回流 Runtime；`check-provider-construction-ownership.sh` 零白名单锁定 | 已完成（#907） |
| P2 | **调用期配置为共享可变状态（已对齐 #902）** | `InvocationScope` 冻结 model / max tokens / requested/effective reasoning；Anthropic、OpenAI-compatible、Ollama 请求编码只读 scope，provider 不再发布调用期 setter | 后续接入完整 `InvocationRequest.options` 与 capability fingerprint，不恢复共享 current state | 已完成（#902） |
| P3 | **Main/Sub client 并发踩踏（已对齐 #902）** | Main/Sub 每次调用各自构造 scope；已删除 `shared_client_lock`、previous/restore 字段和 finalize 恢复分支，取消或 panic 不会修改其他调用配置 | 后续可继续共享不可变 Transport / HTTP pool，并将具体 client 依赖收口到 Runtime-owned port | 已完成（#902） |
| P4 | **流协议依赖多方法回调（已对齐 #903/#907）** | `LlmProvider::invocation_stream` 是内部 driver 实现入口；Runtime 主动 poll 封闭 `InvocationEvent`；legacy sink/callback 已物理删除，`InvocationSink` 仅为 Provider-private decoder seam | 保持跨 BC 只交换 pull stream，Guard 禁止 legacy callback 与 `InvocationSink` 泄漏 | 已完成（#903/#907） |
| P5 | **wire DTO 发布面已收窄（#907）** | request/response/SSE DTO、client config 与具体 driver 类型均留在 Provider adapter；crate-root 只发布中立 Invocation PL，构造面单独归 `provider::composition` | 保持 Runtime/Context/SDK/TUI 不见 vendor DTO，由 crate façade与构造所有权 Guard 锁定 | 已完成（#907） |
| P6 | **跨调用重试已归 Runtime（#905）** | Provider pull-stream 入口只执行单 attempt 并返回 typed retry hint；Runtime 拥有 attempt/backoff/compact/final failure，Guard 禁止 Provider 恢复 retry loop | 保持一次 Provider invoke 对应一次上游语义请求 | 已完成（#905） |
| P7 | **stream → non-stream fallback 已归 Runtime（#905）** | Provider 不再隐式发起第二次请求；任何 fallback 由 Runtime 显式建立新 attempt | 保持 attempt 事件、usage 与取消独立可归因 | 已完成（#905） |
| P8 | **reasoning 能力与 clamp 分散** | driver、provider、Runtime 与 model 配置分别处理上限/字段；Anthropic、OpenAI-compatible、Ollama 路径不统一 | Workflow 固定默认 desired effort（Config `max_reasoning` 已退役，#921）→ Provider/model capability clamp；Provider 统一能力解析与 wire 映射。**v0.1.0 scope**：resolver 领域迁移完成但未接生产链路；是否接线由 v0.2.0 #1142 决策 | S3/S5 |
| P9 | **错误分类已统一（#905）** | HTTP、网络、stream、取消和 context 超限统一为 `ProviderErrorKind + retryable + safe provider code`；Runtime 只按结构化语义编排 | 保持 driver 只分类错误，不拥有跨调用策略 | 已完成（#905） |
| P10 | **Usage 与成本边界已建立 Provider 原始事实 seam（#906）** | pull-stream bridge 直接从 Anthropic/OpenAI Chat/Responses/Ollama wire 事件提取 `RawUsageSnapshot`；未报告保持 `None`、真实零保持 `Some(0)`，完全无 usage 时 completion 为 `None`；legacy `Usage` 与 Runtime/Audit attempt 关联仍待后续切线 | Provider 标准化 RawUsageSnapshot；Runtime 在 retry/fallback 收口后关联逻辑 Model Invocation；Audit MVP 只存 Usage，Cost/Pricing 保留 Future | Provider PL 已完成（#906）；Runtime/Audit 后续 |
| P11 | **能力查询已建立单一 driver reasoning capability（#906）** | OpenAI-compatible driver 以唯一 `ReasoningCapability` 声明 supported levels/mapping，legacy maximum/clamp 从其派生；完整 driver+model+deployment resolver 尚未接生产 | 发布只读 ModelCapability，未知能力保守处理，并在编码前再次复核 | Provider 声明完成（#906）；生产 resolver #1142 |
| P12 | **具体 Provider 构造已集中（#907）** | Composition Root 独占 `provider::composition` 构造面与 Runtime-owned `ProviderFactory` 实现；缺失/非法配置 fail-closed | 保持非 Composition crate 零构造引用，Guard 零白名单 | 已完成（#907） |

## 5. Memory 现状缺口（S2 代码盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| M1 | **MemoryPort 生产链已完成切换** | #895/#896 建立 Memory-owned Port/service/adapter；#897/#984 切换 Context/Tool/Runtime 消费；#883 删除 Storage `MemoryStore`；#900 删除 Composition 第二 active open，生产只经 Main Session `DatasetMemoryOpener` | 保持调用方只消费 `MemoryPort` / `MemoryOpener`，禁止 concrete service/adapter 跨 crate 回流 | 已完成（#895/#896/#897/#984/#883/#900） |
| M2 | **MemoryService 与 adapter 已收回 Memory 内部** | #900 将 `AtomicDatasetMemoryStore`、project opener 与 `MemoryService` 收回 crate 内；adapter contract 测试迁入 crate 内，Composition 无法再直接构造第二 service | 正式唯一构造 capability Guard 与 hooks stale allowlist 清理由 #982/#1022 完成 | 已完成（#896/#883/#897/#900）；Guard #982/#1022 |
| M3 | **检索为子串匹配** | `entry_matches` 朴素小写 contains，无相关性排序 | Tier 1 BM25 关键词相关性排序 | #551 |
| M4 | **similarity_threshold 仅用于去重** | 检索不接入 threshold | 检索也用 threshold 过滤低相关结果 | #551 |
| M5 | **Reflection 领域与执行/TUI 语义已收口** | #898 迁移 PL、prompt/schema/parse/format/apply；#899 将三类 trigger 切入 Runtime 单槽异步协议；#900 删除文档中的旧 `MemoryStore` apply 示例，确认 `ReflectionEngine` 是当前无状态 `ReflectionPromptPort` 实现而非第二 active Memory state | 不得恢复同步执行、第二 store 选择或结果正文投影 | 已完成（#898/#899/#900） |
| M6 | **ReflectionPromptPort 与 durable history adapter 已完成** | #898 发布 Memory-owned ReflectionPromptPort/PL 与 ReflectionHistoryQuery；#899 已实现 Memory-owned `ReflectionHistoryStore` append/query durable adapter，Runtime 完成后持久化 record，SDK/TUI 仅取得 safe summary；日志不含 prompt/raw response/output 正文 | 保持 Memory 不依赖 Provider；后续只允许兼容清理，不新增第二 history owner | #898（契约完成）/ #899（完成） |
| M7 | **只读注入与 legacy top query 已完成退役** | #984 让生产 Run 经 Context `MemoryRetrieveAdapter` 调用 `retrieve_for_inject`；#883 已物理删除 Storage legacy store，因此 `top_for_inject*` 定义与消费者均为零；#900 增加 Composition 静态退役证据 | 不得恢复 Runtime-owned render、mutating query 或 Main/Sub 角色分支 | 已完成（#897/#984/#883/#900） |
| M8 | **SessionReminder 在 Memory** | `share::memory::session_reminder` 是会话级数据 | 迁移到 Context Management（Session 聚合）| #870 |
| M9 | **NoOpMemory、Sub Disabled 与 Reflection 编排已落地** | #897 发布 owner-owned NoOpMemory；Sub ToolResources 明确装配 NoOp，Composition MemoryMode::Shared 仅 clone active Arc；#899 已接 Reflection 单槽生产编排与 history view | #900 删除 Runtime 重复 MemoryPort 骨架；不改变 #899 已完成语义 | #897/#899（完成）/ #900 |
| M10 | **v2 identity key 与 legacy reader 已接入唯一 opener** | #896 建立 v2 key 与 fail-closed legacy migration；#897 接入 production source；#900 移除 Composition 直接 project opener，legacy 读取只作为 `DatasetMemoryOpener` 内部 open 兼容步骤 | 保留旧文件只读兼容，active writer 只写 versioned dataset；未来删除旧文件需独立数据迁移计划 | 已完成（#896/#897/#900） |
| M11 | **查询为已验证内存 state 的纯查询** | MemoryService eager-open 后，`retrieve_for_inject` / search / list / stats 不做 I/O 或 touch；#883/#984/#900 已清零 legacy top query 生产与定义 | 访问统计若需要必须另设显式 fallible command | 已完成（#895/#896/#897/#984/#883/#900） |
| M12 | **active/archive 同代提交且旧 Store 已退役** | Global/Project 各自使用 AtomicDataset generation，层内 active/archive 两 member 原子提交；#883 删除 Storage 旧分文件 writer，#900 清理剩余 façade/术语 | 跨层 compact 保持两个可观察 layer command，失败不伪装全局成功 | 已完成（#983/#896/#883/#900） |
| M13 | **Main Memory 单次 open、shared lease 捕获与同 Arc 分发已建立** | #871/#984 建立 session lease 捕获；#900 删除 bootstrap `_main_memory` 第二 open，并以 Composition 静态测试锁定 `DatasetMemoryOpener::new` 唯一 active opener | 保持 lease 内一致性；正式全局 capability Guard 由 #982/#1022 固化 | 已完成（#897/#899/#871/#984/#900） |
| M14 | **legacy top query 已物理退役** | #984 生产只经 `retrieve_for_inject` + Context render；#883 删除 Storage legacy store 后 `top_for_inject_readonly` / mutating `top_for_inject` 定义为零；#900 搜索验证无生产/测试引用 | 访问统计若需要必须是显式 fallible command | 已完成（#984/#883/#900） |
| M15 | **Memory 共享内核存在重复公开入口** | #997 已删除只做 re-export 的 `share::memory_ops`，Tools 消费方统一经 `share::memory`；`memory.rs` + `memory/` 保持 Rust 2018+ 模块布局。该 PR 不迁移领域所有权、不移动 SessionReminder、不改变检索、去重或评分行为 | #895–#900 将 `share::memory` 中的领域语言与行为迁入独立 Memory capability；#870 承接 SessionReminder 所有权迁移 | #997（目录收口完成）/ #895–#900（语义迁移） |

> **#997 Guard / 白名单审计**：覆盖 `agent/shared/src/memory{.rs,/}` 的 `check-share-no-upstream-deps.sh` 与 `check-share-minimal-kernel.sh` 均按整个 share crate 扫描，不含 Memory 路径 allowlist、整文件豁免、行级 `allow`、`grep -v`、exclude 或 skip；`per_file_exemptions = {}`。本次公开入口收口未修改 Guard，白名单预算保持 `0 → 0`，无需向 #1021 登记迁移例外；`check-cargo-dependency-graph.sh` 仅将 `share::memory::*` 记为 Memory 当前物理落点，事实未变化。

## 6. Context Management / Config 现状缺口

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| CM1 | **Compact execution 与 idle command ownership 已收口，L2/L4 算法仍待实现** | #876 将 Main/Sub 自动 compact 切到 ContextPort；#872 又将 idle `/compact` 切到 `manual_compact`、reset 切到 `clear_session`，删除 Runtime legacy compact helper。两者共享 canonical backing、mutation gate 与 AtomicBlob writer | L1 在 ToolResult 进入 ContextAppend 前完成；L2-L4 只变换 ContextWindow 读模型；L5 经 ContextPort 修改稳定 Session backing | #876/#872（ownership 完成）/ #548/#552/#554（算法） |
| CM2 | **Runtime 自动触发已统一，预算算法仍继续演进** | #876 让 Main/Sub 使用同一冻结 ContextRequest 和 ContextWindow token estimation；#872 保持 manual/auto 入口分离并删除 Runtime 固定 fallback | Provider capability 提供模型上限；TokenBudgetConfig 单一来源；fingerprint 增量估算且 Hook 只在真实 compact 时触发 | #876/#872（切线完成）/ #550/#553（算法） |
| CM3 | **Runtime 的 Prompt/Memory/History/Session 入口已统一** | #872 删除 Runtime `current_chain/frozen_chats/active_summary`、`SaveChainFn`、loop-exit writer 与 `session_storage` façade；Runtime 生产源码不再引用 `context::session::*`。resume、session commands、idle compact/reset 只消费 Context crate-root PL / ContextPort | ContextPort/build_window 与 Context session façade是唯一 Runtime 入口 | 已完成（#876/#872） |
| CM4 | **Guidance / Skill / Git Context 边界不完整** | SKILL.md 扫描缺失；Prompt 散点执行 git / 读 cwd；user guidance 只取首个文件且 alias / canonical 去重不统一 | Skill-owned materialization + 全覆盖扫描；Project WorkspaceRead snapshot 经 ACL 注入；每目录 AGENTS-first / CLAUDE fallback、多层有序、canonical 去重 | #870 / #912 / #894 / #965 |

### 6.1 Context Management 父项测试验收（#1055）

> 审查范围：父 Issue #762 的直接叶子 #868、#869、#870、#871、#872、#994；这些行为叶子均已关闭，#1055 的原生 blocked-by 已解除。算法演进 #548/#550/#552/#553/#554 与 Guidance/Skill/Project 后续能力不属于 #762 已交付行为，继续按上表 owner 跟踪。

| 行为 / 风险 | 必要层 | 可追溯证据 | #1055 结论 |
|---|---|---|---|
| ContextPort Published Language、provider-neutral DTO 与六方法 | L3 | `context/tests/context_port_contract.rs`、`runtime/src/ports/context_port_tests.rs` | 完整；Runtime fake 只消费 Context-owned OHS，finalize cause、typed receipt/conflict 与 Tool/Agent receipt 顺序均有契约证据 |
| Window 组装、Prompt→Memory 顺序与失败短路 | L2-L3 | `context/tests/application_service_contract.rs`、`application_service_failures.rs` | 完整；history/pending/block placement、Prompt typed failure 与 Memory 不被继续调用均覆盖 |
| Runtime finalized Step → ContextAppend 字段与 receipt 闭环 | L2-L3 | `runtime/src/application/context_coordination_tests.rs` | #1055 补齐；覆盖 source/expected revision、三类稳定事实中的取消路径、Tool/Agent receipt、API usage、返回 receipt、冲突不重试与 fingerprint 确定性/敏感性 |
| Session canonical append、durable-before-publish、幂等与 revision conflict | L2-L3 | `context/tests/canonical_session_repository.rs`、`in_memory_session_backing.rs` | 完整；写失败不发布、重复键同 fingerprint 幂等、不同内容 typed conflict、stale compact revision 均覆盖 |
| Envelope current/legacy/future 与 Workspace/Task ACL | L1-L4 | `session_envelope_codec.rs`、`session_recovery_scenarios.rs` | 完整；canonical round-trip、legacy messages/cwd/workspace、missing/empty、future 原字节保留和 canonical reload 均可追溯 |
| AtomicBlob primary/previous/promote/quarantine | L2-L3 | `session_persistence_service.rs`、`session_snapshot_store_contract.rs` | 完整；primary 成功、previous 恢复、双代拒绝、future fail-closed 与 unsafe key 均覆盖 |
| Main Session gate 与跨 BC restore 原子性 | L2-L4 | `main_session_gate.rs`、`main_session_wiring.rs`、`main_session_config_facade.rs` | 完整；shared/exclusive、Workspace missing、Memory prepare failure、Task missing/empty、跨项目 Config/Memory、watch 发布与 caller drop 均覆盖 |
| canonical finalized append 在 resume 后仍可见 | L4 | `context/tests/main_session_wiring.rs::finalized_append_persists_and_is_visible_after_resume` | #1055 补齐相邻链路；证明 ContextPort append 更新 canonical holder，resume 后 history/revision/ledger 保留 |
| Production Composition → AtomicBlob writer → reopen | L4 | `composition/tests/main_session_wiring.rs::production_context_append_reopens_from_atomic_blob` | #1055 补齐；真实 ProductionMainContextFactory、AtomicBlob writer 与 canonical reader 形成生产装配证据 |
| Runtime 不恢复 Session 第二 backing、旧 writer 或索引式归属 | L0 | `check-shared-run-loop.sh`、`check-shared-run-loop-tests.sh`、crate API/Context architecture guards | 完整；负例覆盖 `ChatChain` 与 `projection_start_index`，其余禁止符号由同一生产源码扫描表达式锁定 |
| L5 真进程 / PTY / 发布资产 | L5 | `scripts/check-slow-test-matrix.sh` 的既有 CLI/PTY smoke | 本能力不新增专属 L5；Context/Session 行为由进程内 L2-L4 充分覆盖，真实终端不承担字段/恢复契约 |

测试组织与确定性结论：fixture 跟随 Runtime application、Context application/adapters 与 Composition owning layer；文件测试使用隔离目录，env 由进程级 mutex 保护，异步 gate 使用确定性 permit/owned task 推进；未新增短 `sleep`、万能 `test_utils`、`mod.rs` 或 `include!`。

最终证据（2026-07-20）：

- 定向 Context / Runtime / Composition 测试全部通过；workspace coverage 执行的全测试通过。
- `cargo fmt --check`、production reachability、all-targets clippy、全部架构守卫通过。
- coverage：workspace region/function/line `77.83% / 77.34% / 77.71%`；Context `78.21% / 71.51% / 79.87%`；Runtime `70.40% / 69.94% / 70.64%`；Composition `77.35% / 65.14% / 80.98%`。
- 独立 `cargo test --workspace` 首次运行暴露 Logging scratch fixture 的并行临时目录碰撞；定向复跑通过，coverage 全量运行也通过。该失败与 Context 无关，但首次结果未被隐藏；确定性缺陷由 [#1257](https://github.com/rushsinging/aemeath/issues/1257) 承接。

百分比只作风险信号，不替代上表行为证据。Context Management 在 #762 已交付范围内无未解释测试空白；CM1/CM2/CM4 列明的后续算法与供应边界继续由既有 owner Issue 承接，不阻断 #762 本轮父项验收。
| CFG1 | **Config adapter、durable protocol 与 Env 单一入口** | #934 完成 File/CLI/Compatibility/Claude adapters、validation 与 AtomicBlob durable flow；#935 收口业务 env；#1090 修复 native runtime override 只写不读、连续 update/project commit 使用陈旧 baseline，并补齐优先级、恢复、原子性、订阅与 SDK DTO 的 L1-L4 证据 | adapter 输出 ConfigPatch；Application 只编排 layer / validation；RuntimeOverride 在 CLI 后重放；唯一 active baseline 与 watch 同步提交；Runtime/其他 BC 只消费 ConfigSnapshot | #934 / #935（完成）/ #1090（测试审查与缺陷收口） |
| CFG2 | **reasoning 上限解析已退役（#921 收缩）** | `max_reasoning` 与整个 `reasoning_graph` config 已从 Config 退役 | Config 不再承载 `reasoning_graph`；Workflow 五节点采用固定默认 effort 无 config override；如需 reasoning level 上限控制，由 v0.2.0 [#1142](https://github.com/rushsinging/aemeath/issues/1142) 重新决策 | #921 / #1142 |
| CFG3 | **active Config 非 project-aware 联合切换** | #933 已建立独立 Config BC crate、唯一 ConfigAppService/wiring、ConfigReader/Query/Writer/ProjectConfigParticipant typed seam，并由 Composition 单例注入 Runtime；真实 session-switch shared/exclusive gate、Memory/Task 联合切换与 cancellation-shielded durable commit 尚未实现 | Config 独占 `{location,snapshot}`；Project→Config ACL；Project→Config→Memory→Task prepare；update handoff 后由 owned cancellation-shielded task 完成 durable Config persist、Memory install、Config install、watch 最后发布；非 Run `ConfigQuery::snapshot/subscribe` 先取得 shared permit | #933（seam/单例完成）/ #871（gate/联合协调） |
| CFG4 | **交付层直连 Config 风险（生产构造已收口）** | #683/#696 已收口默认值与裸 Config 消费；#933 已删除 Runtime 散点 `new/load`，model switch/list 复用同一 committed reader，并在 AgentClient 发布 typed `config_view/update_config` SDK DTO；#949 事实复核确认 `ConfigWiring` 的 reader/query/writer/participant 均 clone 同一 `Arc<ConfigAppService>`，Runtime 持有 Arc 因而 service/watch 生命周期不会随局部 wiring drop；同时删除 `trait_reflection.rs` 整文件 Guard 豁免，生产 Config 构造例外归零。TUI/CLI 无 Config 契约泄漏 | #871 提供唯一 gate-aware façade implementation；TUI / CLI 经 AgentClient command + SDK event，CLI args 只作 bootstrap source | #683 / #696 / #933（delivery seam 完成）/ #949（生命周期事实与零例外 Guard 收口）/ #871（gate） |

## 7. Storage 现状缺口（S2 摘要盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| S1 | **Storage 同时拥有业务模型** | Task/Batch 状态、依赖图、Memory 查询与 History 策略寄居 storage crate | Task/Memory/History 所属 BC 独占模型和不变量；Storage 只实现物理端口 | #883 |
| S2 | **Session 已统一复用原子写机制** | #869 建立 Session Envelope + AtomicBlob；#872 删除旧 `session_storage.rs` tmp/fsync/rename writer，list/export/import/metadata/delete 与 resume 全部走 canonical codec / AtomicBlob | 通用 AtomicBlob adapter；数据 BC 的窄持久化端口复用同一机制 | Session 已完成（#869/#872）；Memory 由 #883/#896 收敛 |
| S3 | **backup/恢复协议不完整** | Session 有一代 `.bak`，但备份旋转失败被忽略；其他路径无 previous/quarantine | 原子可见、机械代际读取、领域验证后显式 promote/quarantine | #881 / #882 |
| S4 | **路径与任意物理 Path 耦合** | 多处业务代码拼接 `~/.agents` 路径或直接持有 PathBuf | StorageKey + SafePathSegment；物理根和路径解析只在 adapter | #880 / #883 |
| S5 | **Tool Result 策略落入 Storage（已对齐 #884）** | #884 删除 `storage/src/tool_result.rs`、50K 常量与业务 façade；ConfigSnapshot 发布 validated char policy；Runtime-owned materializer 统一 Main/Sub，写失败保留完整 inline；Runtime adapter 以 write-once/CAS 语义调用 AtomicBlob | 后续只由拥有生命周期的 BC 增加 orphan/retention 清理；Storage 不恢复 Tool Result schema/preview/reference，不发布 AppendLog OHS；旧 `.txt` 引用保持可读但不作为新 AtomicBlob 布局 | 已完成（#884） |
| S6 | **错误与损坏处理不统一** | String/Option/领域错误混用，journal / primary / member digest 歧义可能被当作缺文件、空 dataset 或仅日志 | `StorageErrorKind::CorruptTransaction` + typed reason / transaction scope / quarantine disposition；blob / dataset crash-protocol 矛盾 fail-closed，领域 payload/schema 损坏仍由所属 BC 分类 | #880 / #881 / #882 / #983 |
| S7 | **并发写与临时文件协议未统一** | 固定 `.tmp/.new`，跨实例互斥和残留清扫语义不一致 | 随机 create-new、跨进程锁、commit marker crash recovery | #882 |
| S8 | **只有单 blob 原子性** | #983 已新增独立 `AtomicDatasetPort` / 文件系统 adapter，并以 Prepared journal 为 commit point、读取前 roll-forward、typed corruption quarantine 闭合多 member crash protocol；Memory 仍使用过渡 store，尚未消费该机制 | Storage-owned `AtomicDatasetPort`：dataset lock、全 member stage、Prepared 后只 roll-forward、read-before-recovery；Memory active/archive 与 legacy migration 的领域 adapter 复用同一 primitive | #983（机制已完成）/ #896（Memory 集成 deferred） |
| S9 | **Storage 父项测试完整性已复核，公开面收口仍阻断** | #1057 已补 SafeStorageRoot 契约、Session 相邻映射、owning-layer 外置与确定性握手；Storage L0～L4 机制矩阵见[测试治理 §11.9.5](04-testing-and-coverage.md#1195-1057-实施结果与行为证据矩阵)，L5 不适用。审查发现 crate-root/`storage::api` 双 façade 与 Target `list_primary` 文档—代码漂移 | #1263 决策唯一公开面与 `list_primary` 机械契约，迁移跨 Context/Config/Runtime/Memory/Composition 消费者并更新 Guard；完成前 #1057/#848 不关闭 | #1057（测试缺口已闭合）/ #1263（blocked-by，开放） |

## 8. Logging 现状缺口（S2 摘要盘点）

#1000 已把 Logging 迁入 `domain + adapters` 物理骨架。#937/#940 建立并接通不可变 task-local `LogContext`；#942 已删除 legacy 全局执行上下文、setter/getter 与 formatter fallback，并把 Audit Usage Fact 与 Audit 模块运行诊断从名称和路由上分离。

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| L1 | **Main/Sub 日志上下文隔离已对齐** | Main/Sub 生产链使用 task-local `LogContext`；#942 已删除 Logging crate 内 legacy static、setter/getter 与 formatter fallback；scope Guard 禁止旧符号和未登记进程状态回流 | 执行上下文只通过不可变 scope 传播；scope 外使用空快照 | 已完成（#937/#940/#942） |
| L2 | **sink 失败可观察并可恢复（已对齐 #939）** | `FileSinkLifecycle` 实现 Healthy/Degraded/Recovering；adapter-private I/O/clock/emergency seam 覆盖 open/write/flush/metadata/existence/remove/rename/reopen；每 sink 独立锁、direct stderr fallback 与固定 5 秒惰性 reopen 已落地 | 异步队列/backpressure、跨进程锁、历史 record 重放、全局 shutdown 与配置热更新另行设计；Logging 继续保持 best-effort，不承担 Audit durability | 已完成（#939） |
| L3 | **TargetCatalog 多份真相（已对齐 #936）** | `domain/routing.rs` 唯一定义 target、owner、sink ID 与文件名；File adapter 和 routing guard 共同消费，旧白名单、文件 match、sink 字段与 flush 清单已删除 | 后续新增生产 target 必须只扩展 TargetCatalog，并由唯一性与全仓 Guard 验证 | 已完成（#936） |
| L4 | **全仓 Target 生产消费已对齐 #941** | #941 将生产日志迁到 crate-root owner `LOG_TARGET`，修正 Context 冒用 Runtime/Storage target；Catalog 仅覆盖具有独立运行时边界的 CLI/Composition/Shared 与业务 feature owner，并在真实入口及成功/失败/降级终态消费；SDK、Utils、Logging 自身与 xtask 不制造应用 target；owner-aware Guard 拒绝 bare macro、字符串 target、匿名保活、重复/缺失常量、owner 错配与 workspace 分类漂移 | 后续新增 crate 必须先分类 runtime owner 或 non-runtime member；runtime owner 必须建立真实边界日志，非 runtime member 禁止占位 target | 已完成（#936/#941） |
| L5 | **Logging 与 Audit 已分离** | Audit Usage Fact 只走 Audit-owned append store；Audit 模块运行 warning 使用 `aemeath:diagnostic:audit` → `audit-diagnostic.log`，旧 `aemeath:agent:audit` / `agent-audit.log` 已删除 | Guard 固化 Audit Fact 不得进入 DiagnosticRecord/target 路由 | 已完成（#942） |
| L6 | **Config 参数与 lifecycle 完整接线（已对齐 #938/#939/#941）** | Composition 从 committed `ConfigSnapshot` 构造不可变 `LoggingSettings`；#941 由 CLI typed bootstrap input 选择 File/Stderr，删除 `AEMEATH_LOG_STDERR` 进程 env 旁路，保持 Runtime 前唯一初始化；Logging 单次归一化 filter/max-level 与 `max_bytes=0 → 1`，并消费 retention days | 配置热更新若需要可更新 policy handle 另行设计；output mode 是单次 delivery bootstrap 输入，不新增 Config 持久字段 | 已完成（#938/#939/#941） |
| L7 | **schema/规范漂移** | 实现为 14 字段，部分注释仍称 13 | 14 字段 v1 契约 + consistency guard | S5/S7 |

## 9. Application Version Control 现状缺口（S2 摘要盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| V1 | **Channel 配置未生效** | Config 声明渠道，gateway 固定 `/releases/latest` | Config ACL 映射 typed UpdateChannel | S5 |
| V2 | **检查缓存契约矛盾** | SDK 称 24h cache，spec/实现每次请求 | Cached/ForceRefresh、TTL/max stale/rate-limit | S5 |
| V3 | **Config 未注入装配** | Composition 直接 `UpdateGateway::new()` | 构造 policy、source、cache 与 installer | S5 |
| V4 | **错误同质化** | 全部压成 `Internal(String)` | 稳定 UpdateErrorKind 与结构化元数据 | S5 |
| V5 | **checksum 不证明发布者身份** | artifact 与 checksums 同源 | signed manifest + 固化信任根 | 独立安全 issue |
| V6 | **安装不是受验证的单步提交** | 固定 `.new` 直接 rename；无 target identity/锁 | VerifiedUpdatePlan + digest recheck + atomic commit/helper | 独立安全 issue |
| V7 | **Release Source ACL 不完整** | DTO/URL/状态码直通且缺 host/size 约束 | 私有 DTO + source 安全校验 | 独立安全 issue |
| V8 | **检查与执行端口混合** | 单一 UpdateService，perform 内再次检查 | Runtime ApplicationVersionPort；模块内 plan/apply 分离 | S5 |

## 10. Policy / Hook / Audit 现状缺口（S2 代码盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| PHA1 | **Policy 统一端口与 Config 驱动生产消费已完成** | #917 建立 Policy-owned Request/Decision/Port；#918 统一 Main/Sub 注入；#1221 建立 Standard/AllowAll `AuthorizationContext`、ConfiguredPolicy 动态 mode source 与 Main/Sub/MCP 单次评估链路 | 保持 Policy 契约单一 owner、Config committed mode 唯一真相与授权上下文无损传播；#1062 的 L0～L5 行为—测试矩阵见[测试治理 §11.8](04-testing-and-coverage.md#118-1062-policy-l0l5-覆盖证据) | 已完成（#917/#918/#1221/#1062） |
| PHA2 | **`--yolo` 兼容 ACL 与业务传播退役已完成** | `--yolo` 主名称和 `--allow-all` alias 映射 Config `PermissionModeConfig::AllowAll`；#1221 由 ConfiguredPolicy 生成逐调用 `AuthorizationContext::ALLOW_ALL`，Tool/Project/Runtime/Hook 不再消费业务 `allow_all` bool | 保持兼容入站 ACL，只让 Config committed mode 驱动 Policy；展示投影随 Runtime Context 收敛另行退役 | 已完成（#917/#918/#1221/#1062；L4 覆盖授权旅程，L5 无独立系统边界） |
| PHA3 | **安全 guard 冒充 Policy 风险已收口** | #915/#916 将 content scan、path containment、Bash/read-before-write 归回 Context/Project/Tool；#1221 再由统一 `AuthorizationContext` 控制授权性限制，客观 schema/I/O/取消错误保持独立 | 所有者局部机制保持独立，授权开关只来自 Policy | 已完成（#915/#916/#918/#1221） |
| PHA4 | **Hook 单一公开契约已建立，legacy façade 待退役** | #925 已删除 Runtime 重复 HookPort/Outcome，直接 re-export Hook-owned façade，并建立 HookOutcome→Runtime typed adapter；`adapters::legacy::HookRunner` 仍经迁移期 `hook::api` 提供多个入口 | #926 删除兼容 façade、具体 runner 公开面与 HookUi 推断 | S5 |
| PHA5 | **类型化阻断与投影契约已建立，legacy 发送点待切换** | #924 统一分类；#925 Runtime adapter 只消费 HookOutcome，不二次解析 stdout/JSON，并无损保留结构化 reason、全部 attempts 与 display messages | #926 切换剩余 legacy Runtime 发送点并删除旧解析入口 | S5 |
| PHA6 | **非零 exit 语义已在新 Dispatcher 统一，legacy 注释待退役** | #924 已固定任意非零 exit（含 1/2/127）为主动 Block、只执行一次；shared legacy 配置注释仍保留“exit 2 阻断/其他非阻塞错误”的过期说明 | #926 删除旧注释与 exit code 特判 | S5 |
| PHA7 | **Hook Dispatcher 与 Runtime adapter 已建立，生产 Loop 原子切换待收尾** | #923/#924 已建立受管执行与 Hook-owned Dispatcher；#925 建立 Runtime typed adapter、直接 façade 消费、独立 HookMessage SDK 投影和 updated-input revalidation contract | #926 退役 legacy Runner/HookUi 并切换剩余生产发送点；#879 完成共享 Loop 原子切换；env_clear 与环境白名单差距由 #1216 承接 | S5 |
| PHA8 | **Stop Hook completion/continuation 终态控制待收口** | 当前兼容 Main Loop 已保留并提交 Stop block 前的 assistant Step，且将 feedback 与 hook 执行期间到达的普通用户追问合并为同一 continuation request；仍使用 5 次临时上限、legacy HookRunner，且未具备 CancelRunStep / TerminateRun 的 out-of-band 控制入口 | 依 #743 Target：终态采用 15 次上限与 `StopHookRetryExhausted`，CancelRunStep / TerminateRun 优先于 continuation，并由统一 StepFinalizer 收口 | 当前修复 / S5 终态控制切换 |
| PHA9 | **Main/Sub Hook 行为不统一** | Stop/Hook 路径主要存在 Main loop，Sub 未复用 | 单 Loop Engine + 同一 HookPort；Main/Sub 同规则 | S3 |
| PHA10 | **Hook input/context mutation 契约已建立，legacy 调用点待切换** | #925 已建立 `HookOutcome` typed directive、display messages 与 `tool_coordination` frozen Catalog → Tools-owned schema → Policy 复验 API | #926/#879 在剩余生产 Loop 原子切换时接入该 API，删除 legacy 直接执行路径 | S5 |
| PHA11 | **Audit Usage 写入管线已建立，查询与生产接线待落地** | #927 Usage PL/query port/统一 ID；#928 append store；#929 bounded sender/worker/config/lifecycle/metrics | #930 query → #931 Composition bridge/Invocation 生产接线 → #932 旧 Cost 退役 | S5 |
| PHA12 | **Usage/Cost/Pricing 混在 Runtime** | CostTracker 同时记录 usage、计算 cost、读写全量 cost_history.json | #927 建立无 Cost 的 Audit Usage PL；#931 切生产写链，#932 退役 CostTracker/Price/旧 history；Cost/Pricing 保留 Future | S5/S7 |
| PHA13 | **Usage 统一关联 ID 契约已建立，生产接线待迁移** | #927 通过 SDK 发布唯一 Session/Run/RunStep/ModelInvocation ID，并由 Audit UsageRecord 直接复用；旧 CostTracker 仍缺完整关联 | #931 将 Provider/Runtime Invocation 生产链接入新 ID 与 UsageSink；#932 退役旧 CostTracker | S3/S5 |
| PHA14 | **Usage 写入阻塞且全量重写** | Runtime 直接 fs read/write JSON 数组 | 非阻塞 bounded UsageSink；worker 经 Audit UsageAppendStorePort 写 JSONL | S5 |
| PHA15 | **Usage 与 Session 存储边界不清** | cost_history 为全局混合文件，缺独立 Audit 分区语义 | `~/.agents/audit/usage/{session_id}.jsonl`；Session 删除不级联 | S5 |
| PHA16 | **Audit/Logging 混淆风险** | Usage/Hook 信息依赖诊断日志展示，无事实查询端口 | Logging 只做诊断；UsageQueryPort 读取 Audit 事实，不解析日志 | S5 |

## 11. 死代码 / 退役清单

| 项 | 现状 | 处理 | 阶段 |
|---|---|---|---|
| **Scheduler** | `TaskScheduler` 全仓库仅内部 5 处引用，无生产实例化 | 判定死代码，删除 | S7 |
| 6 个 core 注入闭包 | `ChatLoopContext` 的 `save_chain`/`run_reflection`/`list_models` 等，为打破 business→core 反向依赖的临时注入 | 收敛后由对应 Port 替代 | S5 |
| 旧扁平 `Session.messages` | #872 已从 live 聚合删除；仅 legacy reader DTO 保留 | Guard + codec fixture 防回流 | 已完成（#872） |
| `legacy-no-agent`、历史 `register_all_tools*`、旧内部 Registry/Profile 与 `SkillTool` | #912 已让正式 Main/Sub Scope、Catalog 与 Execution 不再包含 Skill，并删除 Context-owned Skill loader；Tools 内仍保留 legacy-no-agent、兼容 Registry façade、Profile/Scope 与 SkillTool 文件 | #914 删除兼容 Scope、历史入口、无消费者旧内部 Registry/Profile、legacy Skill DTO 与 SkillTool；调用方只用正式 Scope/Skill 装配 | #914 |
| Runtime `idle_commands` 命令聚合 | 三种 Slash 机制混在 Runtime idle 流程 | Command Router 接线后拆除旧生产入口 | S5/S7 |
| MCP 旧 wrapper / diff 孤立路径 | 多套 wrapper、diff/refresh/health check 未形成完整生命周期 | MCP Ready 后统一至 McpConnection + ACL；无消费者代码删除 | MCP Ready 后 |
| 共享 client 的 `set_*` / restore 路径（已退役 #902） | Provider 与 Runtime 已无调用期 setter、shared-client lock、previous/restore 字段；每次调用读取不可变 Invocation Scope | `check-provider-invocation-scope.sh` 阻止 atomics、setter、restore 与 serialization lock 回流 | 已完成（#902） |
| Provider-private `InvocationSink` | #907 已删除旧 `LegacyStreamSink` / callback wrapper；当前 `InvocationSink` 仅在 Provider adapter 内把 vendor stream 归一为 `InvocationDelta` | 保持 crate-private，`check-provider-pull-stream.sh` 禁止 Runtime/Context 引用 | 已完成（#907） |
| Provider wire DTO 公共 re-export | #907 已把 request/response/SSE DTO、client config 和具体 driver 收回 adapter；crate-root 只留 PL，构造面仅 `provider::composition` | crate façade与 construction ownership Guard 防回流 | 已完成（#907） |
| Provider 内部 retry / non-stream fallback | #905 已迁至 Runtime attempt 编排；Provider 入口单 attempt、只分类错误 | `check-provider-retry-ownership.sh` 防回流 | 已完成（#905） |
| `SessionReminders` 在 `share::memory` | 会话级提醒放在 Memory 共享内核，语义不属跨会话记忆 | 迁移到 Context Management 后从 `share::memory` 删除 | S5/S7 |
| **MemoryStore 业务 façade（已退役）** | #883 已删除 Storage-owned store；#900 删除 Composition 第二 active open，将 concrete dataset store/project opener/service 收回 Memory crate 内，并清理旧 apply/注入术语 | 生产只经 `DatasetMemoryOpener` → `MemoryPort`；正式全局构造 Guard 与 hooks stale allowlist 由 #982/#1022 固化 | 已完成（#883/#900） |
| Storage crate 内 Task/Memory 业务实现 | 物理持久化 crate 同时拥有 Task 状态机、依赖图与 Memory 查询行为 | 迁回对应 BC；Storage 仅保留 adapter 与通用机制 | S5/S7 |
| 业务代码散点直接文件写入 | Session/Memory/History/Tool Result 各自实现 IO 语义 | 窄数据端口接 Storage adapter 后删除重复路径 | S5/S7 |
| Logging 进程级 `CURRENT_*` | legacy static、setter/getter 与 formatter fallback 已删除；scope 外使用空 `LogContext` | `check-logging-scope-context.sh` 禁止旧符号和未登记进程状态回流 | 已完成（#942） |
| 普通诊断 `agent-audit.log` 路由 | `aemeath:agent:audit` 与 `agent-audit.log` 已删除；Audit 模块运行诊断改为明确的 diagnostic target/file | routing guard 断言 Audit Fact 名称不存在于 Diagnostic catalog | 已完成（#942） |
| Update 单体 `UpdateService` / Gateway | 检查/缓存/下载/安装混成单对象 | ApplicationVersionPort + 内部 source/cache/installer adapters | S5/S7 |
| `AuditApiMarker` / `gateway::__empty` | #988 已删除；Audit BC 不再以空 COLA 层冒充领域契约 | 后续 Usage leaf 只按真实 domain/application/port/adapter 证据增量建层，禁止恢复占位类型 | 已完成（#988） |
| Runtime `CostTracker` / `pricing` / `CostSummary` | Usage、Cost、Pricing、持久化混合，且不符合 Usage-only MVP | 迁移 raw Usage 后退役；Cost/Pricing 作为 Future 另行设计 | S5/S7 |
| `cost_history.json` 全量写路径 | 每次保存重写数组，记录含派生 cost 且缺 Run IDs | 后续 importer 只迁可验证 raw token；旧路径有计划退役 | S5/S7 |
| Stop Hook 超限强制 Done | Stop 未放行却伪造 Completed | 改为第 16 次 RunFailed 后删除旧 helper | S3/S5 |
| 生产 `allow(dead_code)` baseline | #1015 机械统计出 10 个生产豁免；新增数量已被 Stop 守卫阻断，但历史符号尚未逐项退役 | #649/#947 删除 Runtime/TUI 历史豁免；其他 owner 在相关模块迁移时只减不增；#1018 决定最终执行位置 | S5/S7 |
| 测试 flaky debt | `.agents/flaky-debt.json` 集中记录真实墙钟、固定 `/tmp`、全局 env/cwd 与随机源风险 | owner Issue 按退出条件清理；#1018 runner 保留首次失败，#1050 承接慢速 P1/PTY/platform | S7 |

## 12. 已正确隔离（可作参考范式）

| 项 | 现状 | 说明 |
|---|---|---|
| **Workspace 隔离** | `seed_isolated()`：继承 cwd/root，空栈+新锁，子 worktree 进出不影响父 | ✅ 子资源隔离范式 |

### 12.1 Project / Workspace 测试退出证据（#1059）

- **L0**：production reachability、crate API、Context architecture 与全量 architecture guards 共同证明 Project façade 保持窄，测试 seam **NEVER** 跨 crate 暴露。
- **L1-L3**：`project::adapters::git` 通过 crate-private scripted runner 覆盖 spawn/output 错误分类，并以隔离 system/global config、hooks 与签名的真实 Git fixture 覆盖 Primary、Linked、NonGit、branch、detached HEAD 和 worktree add；单一生产 adapter 下不强造多 adapter factory。
- **L2-L4**：Project service 测试覆盖 candidate commit、写串行、Git I/O 期间旧状态可读和 fork 隔离；Tools 的 Enter/Exit/Switch 测试真实执行 `TypedTool::call()`，断言中间状态、失败零修改与 linked→linked 不压栈；Runtime crate 内契约证明 `from_args_with_workspace` 保留同一 `WorkspaceViews` backing，未新增生产 accessor。
- **L5**：本能力不涉及必须经 CLI/PTY/网络/安装资产验证的行为；真实 `git` 子进程属于 L3 adapter 契约，L5 不适用。
- **范围边界**：跨 BC exclusive session-switch、联合 prepare/commit/publish 与 resume 可观察性继续由 [#871](https://github.com/rushsinging/aemeath/issues/871) 承接；正式 capability-first 边界 Guard 与故意违规证明继续由 [#982](https://github.com/rushsinging/aemeath/issues/982) 承接，#1059 **NEVER** 以测试 accessor 或局部守卫替代这些能力。
| **Task 隔离** | Sub 用全新 `TaskStore::new()` | ✅ |

## 13. 相关文档

- 系统级代码组织规范：[../01-system/06-code-organization.md](../01-system/06-code-organization.md)
- Project 目标端口与代码组织：[../02-modules/project/02-ports-and-adapters.md](../02-modules/project/02-ports-and-adapters.md)
- 守卫运行时真相：[architecture-guards.md](01-architecture-guards.md)
- 领域模型（目标态）：[../02-modules/runtime/01-domain-model.md](../02-modules/runtime/01-domain-model.md)
- 模块边界：[../02-modules/runtime/02-module-boundaries.md](../02-modules/runtime/02-module-boundaries.md)
- Runtime 端口与装配：[../02-modules/runtime/06-ports-and-adapters.md](../02-modules/runtime/06-ports-and-adapters.md)
- Workspace 端口与装配：[../02-modules/project/02-ports-and-adapters.md](../02-modules/project/02-ports-and-adapters.md)
- TUI Model 与 Workspace 投影：[../02-modules/tui/02-model.md](../02-modules/tui/02-model.md)
- Tool & Skill & Command 目标设计：[../02-modules/tools/README.md](../02-modules/tools/README.md)
- Provider 目标设计：[../02-modules/provider/README.md](../02-modules/provider/README.md)
- Memory 目标设计：[../02-modules/memory/README.md](../02-modules/memory/README.md)
- Storage 摘要设计：[../02-modules/storage/README.md](../02-modules/storage/README.md)
- Logging 摘要设计：[../02-modules/logging/README.md](../02-modules/logging/README.md)
- Application Version Control 摘要设计：[../02-modules/application-version-control/README.md](../02-modules/application-version-control/README.md)
- Policy 目标设计：[../02-modules/policy/README.md](../02-modules/policy/README.md)
- Hook 目标设计：[../02-modules/hook/README.md](../02-modules/hook/README.md)
- Audit Usage 目标设计：[../02-modules/audit/README.md](../02-modules/audit/README.md)
- 横切工程总览：[README.md](README.md)

## 修改历史

> **#899 durable lifecycle / compact boundary:** accepted job 先 append `Running`，成功、失败、partial apply、timeout/cancel 均以同 id `upsert` 终态；cancel 不删除 durable fact。PreCompact 只在 compact 成功产生 outcome 后 submit 预先冻结的“将被丢弃”快照；compact 失败不 submit，busy 结构化 warn 后立即 skip，绝不排队。

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-20 | #1266 收紧 filesystem Skill 发现：标准目录与 package 仅解析 `SKILL.md`，直接入口优先于同目录 `skills/`；其他 Markdown 资源不进入 Catalog、Materialization 或 revision；真实入口损坏继续返回 typed error。skills 根目录直接 `*.md` 由 Tools filesystem adapter 暂保留历史兼容，退役前必须另立迁移 Issue 并完成真实使用方审计 | [#1266](https://github.com/rushsinging/aemeath/issues/1266) |
| 2026-07-20 | #1057 完成 Storage 根因级测试审查：补齐 SafeStorageRoot 全公开面路径安全契约、Session durability/promote/delete 相邻映射、domain/adapter/façade owning-layer 外置与 ready/release 锁确定性；production reachability、all-target clippy、架构守卫通过，Storage 定向 coverage 为 regions/functions/lines `83.52% / 91.81% / 86.98%`；workspace 测试/coverage 的 Runtime 卡住首次事实保留；双 façade 与 `list_primary` 漂移由 #1263 承接并阻断关闭 | [#1057](https://github.com/rushsinging/aemeath/issues/1057)、[#1263](https://github.com/rushsinging/aemeath/issues/1263) |
| 2026-07-20 | 冻结 #1057 Storage 根因级测试审查计划：七个业务叶子关闭后，以行为—风险矩阵复核 L0～L5；先治理 owning-layer、日志测试设施与墙钟锁断言，再补 SafeStorageRoot/Blob/Dataset/消费方边界缺口并收口公开面、过渡 Guard 与父项证据 | [#1057](https://github.com/rushsinging/aemeath/issues/1057)、[#848](https://github.com/rushsinging/aemeath/issues/848) |
| 2026-07-19 | #900 删除 Composition bootstrap 的第二 active Memory open，生产只经 Main Session `DatasetMemoryOpener`；concrete dataset store/project opener/service 收回 Memory crate 内；旧 top query 与 Storage business façade 清零，Reflection 文档切到当前 Run 同一 `MemoryPort` apply；未修改 `.agents/hooks/**`，正式 capability Guard 与 stale allowlist 清理由 #982/#1022 承接 | #900 |
| 2026-07-19 | #924 建立 Hook-owned Dispatcher 与 typed attempt classification：按 order + 声明顺序串行匹配、聚合和 Block 短路；仅 ExecutionFailed 最多尝试 3 次，任意非零 exit 一次 Block；Stop 耗尽固定 Block 并 best-effort 派发一次不递归 StopFailure。Runtime 生产接线、legacy 退役和环境白名单分别由 #925/#926/#1216 承接 | [#924](https://github.com/rushsinging/aemeath/issues/924) |
| 2026-07-19 | #912 完成 Skill ownership 生产切线：Tools 独占 PromptFragment 与 Catalog/Materialization 双端口，filesystem adapter 每次消费 project/config/tool snapshot 并发布确定性 revision；Context 删除自有 loader/parser，Main/Sub 负责 scan、stable_key 去重、预算与 cacheable block；正式 Tool Catalog/Execution 不再发布或执行 Skill。legacy-no-agent、历史 Registry/Profile 与 SkillTool 文件仍由 #914 物理退役 | [#912](https://github.com/rushsinging/aemeath/issues/912) |
| 2026-07-18 | #911 完成 Catalog/Execution 生产双端口切线：双 adapter 共享私有 backing，Runtime 生产代码零 Registry/Tool 实例，schema 实现归 Tools，AskUser 以 typed suspension 经 Runtime mapping seam 接回既有 waiter；MCP 仅增加不自动授权的保守 source seam。#877/#878 完整 Interaction 状态机、#912/#913 ownership/装配收口、#914 旧 Registry/Profile/SkillTool 物理退役及 MCP Ready 生命周期/revision 均保持开放 | [#911](https://github.com/rushsinging/aemeath/issues/911) |
| 2026-07-18 | #899 完成三 trigger Runtime 单槽异步、busy skip、静默 TUI、Memory-owned durable history append/query、`/reflect [limit]` 只读安全摘要、安全日志与 Run teardown drain/cancel timeout；仅标记 M5/M6/M9/M13 中 #899 对应项完成，其他 issue 状态不变 | #899 |
| 2026-07-18 | #923 建立 Hook 私有受管 ProcessDriver：Unix 独立进程组、完整 deadline、并发有界管道与 TERM→KILL→wait；PID/后代 marker 证明 timeout/cancel 返回后无存活进程，retry 继续由 #924 承接 | [#923](https://github.com/rushsinging/aemeath/issues/923) |
| 2026-07-20 | #1061 完成父项 #852 的 Provider L0-L4 测试完整性复核：共享 decoder contract 一次覆盖 Anthropic/OpenAI Chat/Responses/Ollama 的顺序、单终结、终结后结束、取消与 consumer drop；crate-root integration test 锁定完整 PL 公开值与关键边界语义；Guard 正向签名匹配收紧并禁止 `InvocationSink` 泄漏。PR #1259 CI 覆盖率为 regions 80.22%、functions 82.80%、lines 81.94%；统一脚本当前不产出 changed-lines，已明确登记工具链边界。`--no-default-features` / `--all-features`、all-targets clippy 和 all-features tests 通过，Provider 无平台专属 cfg，L5 不适用；#1142 resolver 生产接线继续延期且不阻断 #852 当前范围。同步清理 #907/#905 已关闭但本文仍标为开放的 P1/P4-P7/P9/P12 与退役项。 | [#1061](https://github.com/rushsinging/aemeath/issues/1061) |
| 2026-07-19 | #883 删除 Storage-owned `memory_store` / `task_store`、Task/Memory 领域副本与 façade；Context legacy Session 仅保留 opaque JSON 并委托 Task V1 codec，Composition 使用 Memory-owned legacy source factory；Storage Guard migration debt 从 1 降为 0 | #883 |
| 2026-07-18 | #987 将 Hook 从 `api/business/contract/gateway` COLA 固定层迁为 `domain/ports/adapters`：#922 稳定 PL 归 domain，HookPort 归 ports，旧 HookRunner 归 `adapters::legacy`；迁移期 `hook::api` 保留至 #925/#926，业务行为、重试和 Runtime 接线未改变；Hook Guard 白名单与 migration debt 均为 0 | [#987](https://github.com/rushsinging/aemeath/issues/987) |
| 2026-07-18 | #1179 将 Composition→Runtime bootstrap 的 Workspace/Config/Provider/Tool/Task 散点依赖收敛为 typed dependencies value，删除 9 参数签名并恢复 workspace clippy；不改变 #890 Task backing 或 #929 Audit worker 边界 | [#1179](https://github.com/rushsinging/aemeath/issues/1179) |

| 2026-07-17 | #909 落地 RegistryScope、capability ToolProfile 与 `derive_restricted` 只收缩约束；内置工具改用单一注册规格，历史入口保持集合兼容，旧 `NoAgent` 命名为待 #914 退役的 `legacy-no-agent` Scope。#911 Catalog/Execution adapter、Runtime 双端口、ExecutionScope 与 typed suspension 均未完成 | [#909](https://github.com/rushsinging/aemeath/issues/909) |
| 2026-07-17 | #993 完成 Tool crate 物理目录迁移：按证据启用过渡 Hexagonal `domain + adapters`，crate-root `lib.rs` 收窄为 façade，`shared/tool` PL 类型清零并迁入 `domain/types`；`application/` / `ports/` 待语义 leaf 启层，端口 trait 暂居 `domain/`。**NEVER 视为 T1-T12 语义收口**，§3 语义缺口保持全部开放 | [#993](https://github.com/rushsinging/aemeath/issues/993) |
| 2026-07-17 | #927 建立 Audit Usage Contracts 与统一关联 ID：SDK 发布唯一 Session/Run/RunStep/ModelInvocation identity，Audit 拥有 UsageRecord/V1 envelope/emit/query PL，Runtime 仅保留 UsageSink trait；#928/#929/#930/#931/#932 分别承接 IO、worker config/lifecycle、查询行为、生产接线与旧 Cost 退役 | [#927](https://github.com/rushsinging/aemeath/issues/927) |
| 2026-07-17 | #1059 完成 Project / Workspace L0-L4 测试复核：Git adapter 建立私有 scripted runner 与真实 Git 契约，Tools 覆盖 Enter/Exit/Switch 中间态和失败原子性，Runtime 证明传入 WorkspaceViews backing 不被重建；L5 明确不适用，#871/#982 边界保持外移 | [#1059](https://github.com/rushsinging/aemeath/issues/1059) |
| 2026-07-17 | #1000 将 Logging 扁平文件行为等价迁入 `domain + adapters` 物理骨架，隐藏内部模块并把 Runtime 消费切到 crate-root façade；删除 2 个生产 `allow(dead_code)` 及 3 个无消费者 helper（含 `ensure_pid`），未新增依赖、Guard 例外或 allowlist。formatter 因仍读取 legacy 全局 context 暂留 adapter，#937 后迁入 domain schema；真实 `DiagnosticSink` port 由 #939 建立，L1-L7 语义缺口均保持未关闭 | [#1000](https://github.com/rushsinging/aemeath/issues/1000) |
| 2026-07-17 | #983 在 Storage 现有 Hexagonal 层内交付独立 AtomicDataset port/adapter：Prepared durable commit point、post-commit roll-forward、typed corruption quarantine 与 L0–L5 证据闭合；Memory integration deferred 至 #896，未新增 Guard exception 或 allowlist | [#983](https://github.com/rushsinging/aemeath/issues/983) |
| 2026-07-17 | #988 先行删除 Audit 的 `api/contract/gateway` 空 COLA 占位；在 Usage MVP 落地前 crate 仅保留真实 `LOG_TARGET` 入口，后续 #927–#931 按实际能力与 seam 增量建立 Hexagonal 结构，禁止恢复空层 | [#988](https://github.com/rushsinging/aemeath/issues/988) |
| 2026-07-17 | 补齐 #901 文档—代码门禁：确认其交付边界仅为 Runtime-owned `ProviderPort`、中立 Invocation PL 与 fake contract harness；生产 Runtime 仍持有 `LlmClient` / `LlmClientPool`，P1 保持未关闭，不冒充已完成生产切线 | [#901](https://github.com/rushsinging/aemeath/issues/901) |
| 2026-07-17 | #903 完成 P4 跨 BC pull-stream 切线：Provider 生产入口返回 `InvocationStream`，Runtime Main/Sub/Reflection 主动 poll，Runtime/Context 测试替身同步迁移并由 Guard 禁止 legacy sink；Provider decoder 内部 bridge 作为 S7 物理退役残余登记 | [#903](https://github.com/rushsinging/aemeath/issues/903) |
| 2026-07-16 | #1033 交付 crate-private `HttpAttemptExecutor`，收敛 Provider 单 attempt 机械 send/cancel/status、安全 headers、16KiB bounded error body、typed transport failure 分类与单一 diagnostic，新增 `check-provider-http-attempt.sh` 守卫；P6（跨调用 retry/backoff）、P7（stream→non-stream fallback）仍留在 driver 内未迁至 Runtime，保持未关闭，不冒充完成 Runtime 所有权 | [#1033](https://github.com/rushsinging/aemeath/issues/1033) |
| 2026-07-16 | 文档审查：在 §4 缺口表明确后续承接边界——P4（pull-based `InvocationStream`）由 [#903](https://github.com/rushsinging/aemeath/issues/903) 承接；P6（跨调用 retry/backoff）、P7（stream→non-stream fallback）、P9（错误分类统一）由 [#905](https://github.com/rushsinging/aemeath/issues/905) 承接 | [#1033](https://github.com/rushsinging/aemeath/issues/1033) |
| 2026-07-16 | #882 建立单 blob domain-separated digest + Prepared/Committed journal、`previous.next`、同 key OS lock、所有入口恢复前置、持久 promote marker、committed warning 与 typed corruption quarantine；真实进程 smoke 覆盖 OS lock 及 replace 后 abort/reopen；全 fault matrix、无 journal orphan 两分支、并发 writer、协议 symlink 与 UnsupportedDurability contract 已闭合。dataset protocol 归 #983，最终跨模块覆盖复核归 #1057 | #882 |
| 2026-07-16 | #881 在 #880 AtomicBlob 基础上补齐 namespace-owned previous policy、显式 generation read、进程内 previous 轮换、typed promote/quarantine/delete-all 契约与共享 contract tests；跨 reopen 幂等证据、锁、journal、digest 和 committed-warning 仍由 #882 退出 | #881 |
| 2026-07-16 | #880 决策层冻结 Storage Target 为 `domain + ports + adapters` Hexagonal + Clean 结构；以机械 Guard 可证明的层间单向依赖、domain 零物理 I/O、adapter 类型不泄漏和窄 façade 防止长期漂移与劣化，替代先前 `capabilities/` 目标描述 | #880 |
| 2026-07-16 | #991 将 Storage 从 `api/business/contract/gateway` 行为等价迁为 `memory_store` / `task_store` / `tool_result` 顶层过渡模块；旧层、`storage::api` façade 与仅测试可达的旧 History 模块已删除；#883/#884 负责迁出或退役过渡业务语义 | #991 |
| 2026-07-15 | #995 将 Runtime 从 `api/business/contract/core/gateway/utils` 行为等价迁入 `domain/application/ports/adapters`；旧固定层与 façade 已删除。保留的 4 个精确层间迁移例外由 #874–#879 接线后退出 | #995 |
| 2026-07-12 | 补取消现状 R11-R13：Session token 槽、传播缺口、TUI 双路径及退役项 | #700 |
| 2026-07-12 | 新增 Tool/Skill/Command 缺口 T1-T12 与旧 Profile、SkillTool、idle_commands、MCP 路径退役项 | #787 |
| 2026-07-12 | 新增 Provider 缺口 P1-P12 与共享 client、回调流、wire DTO、隐式重试退役项 | #788 |
| 2026-07-12 | 新增 Memory 缺口 M1-M9 与 SessionReminders、MemoryStore 领域方法退役项 | #789 |
| 2026-07-12 | 新增 Storage S1-S7、Logging L1-L7、Application Version Control V1-V8 缺口与退役项 | #793 |
| 2026-07-12 | 新增 Policy/Hook/Audit 缺口 PHA1-PHA16 与 Audit/Cost/Stop Hook 退役项 | #790 |
| 2026-07-14 | 新增代码组织与 legacy guard 迁移记录；将跨 capability Target 映射到执行 leaf；#763 明确为治理父项，#982 承接机械守卫实现与故意违规证明 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 补充 Memory M10-M14、Context / Config CM1-CM4 与 CFG1-CFG4、Storage S8；细化 Task-owned PL、Project → Config → Memory → Task prepare 顺序、Config durable cancellation shield，以及 TUI 单向事件链的逐 issue 退出证据 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-17 | #895 建立独立 Memory crate、Memory-owned PL/OHS、纯 eligibility/scoring/dedup/eviction 基线与 in-memory contract suite；生产 durable adapter、消费方切线和旧契约退役仍由 #896/#897/#900 承接 | #895 |
| 2026-07-17 | #997 删除 `share::memory_ops` 重复公开入口，Tools 统一消费 `share::memory`；确认 `memory.rs` + `memory/` 模块布局有效，Memory Guard 白名单预算保持 `0 → 0`。领域所有权迁移仍由 #895–#900 承接，SessionReminder 所有权迁移仍由 #870 承接 | #997 |
| 2026-07-17 | #921 收缩范围：O8/O10 标注 #921 scope——Provider resolver 领域迁移完成但未接生产链路；Config `reasoning_graph` 全部退役（CFG2 改为退役说明）；P8 Target 移除 Config user max；五节点固定默认 effort；Runtime/Context/TUI 尚未接线；是否保留/接线由 v0.2.0 #1142 决策 | [#921](https://github.com/rushsinging/aemeath/issues/921) |
