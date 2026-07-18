# 迁移治理 · Current → Target 追踪

> 层级：03-engineering（横切工程）
> 状态：过渡追踪｜Milestone：v0.1.0｜对应 Issue：#743 / #761（S2 盘点）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> **本文是 Current → Target 差距、迁移责任、进度与退出条件的唯一真相源**。01-system / 02-modules 设计文档只写目标态；已启用守卫的脚本行为、常量与白名单以 [Architecture Guards](01-architecture-guards.md) 为真相源；开发者当前 **MUST** 遵守的 Project 操作约束见 [`specs/project.md`](../../../specs/project.md)。
>
> **#1021 Guard 例外治理基线（2026-07-18）**：`.agents/architecture-guard-registry.json` 已成为 Guard policy / scope / suppression / migration exception 的单一机器可读注册表；`check-guard-registry.sh` 通过 xtask 校验 stable id、必填归责、path stale、Shell 隐式排除引用和仓库/模块预算。Current migration debt 冻结为 repository `7`：Runtime `5`（4 个层间倒置 + 1 个 shared-adapter bridge）、Storage `1`（#883 承接的 transitional business modules）、TUI `1`（#947 承接的 slash async dispatch）。Storage 的 façade/Cargo edge、Composition 唯一装配点均归 Target policy，不计债务；Workflow、Audit、Project 机器报告 migration exception 为 `0`，与人工基线一致。该注册表只治理 Current 例外，capability-first 正式 Guard 与 legacy COLA 退役仍由 #1022 承接。

## 1. 代码组织、装配与守卫 Current → Target（#972）

| # | Current | Target | 责任与退出条件 |
|---|---|---|---|
| O1 | Runtime、Context、Provider 与 Policy 已完成经批准的 Hexagonal 目录迁移；Policy #917 已建立 Policy-owned PL/Port 与唯一 AllowAllPolicy，并恢复真实 `domain + adapters`；#918 仍需切换生产消费并退役 allow_all 传播；Storage 由 #991 消除固定 COLA 层并由 #880/#983 在 `domain + ports + adapters` 内落地 SafePath/AtomicBlob/AtomicDataset 机制，但过渡业务模块仍待迁出；Config 已由 #999 将 shared kernel 内纯模型/策略归入 `config/domain`、外部来源与路径解析归入 `config/adapters`，但 active service 与语义接线仍待 #933–#935；其余普通 feature 仍受迁移期固定层目录约束；完整脚本行为与常量见 [Architecture Guards](01-architecture-guards.md) | 按 [代码组织规范](../01-system/06-code-organization.md) 的 Hexagonal 默认依赖方向收敛，按 seam 使用最小必要层；`capabilities/` 仅在独立能力证据与配套 Guard 成立时启用。各模块 Target 判定见 [模块目录结构决策](../02-modules/README.md#目录结构决策) | Policy [#986](https://github.com/rushsinging/aemeath/issues/986) 删除 `api/business/contract/gateway` 并建立精确 crate-root façade 与零 exception 结构守卫；[#915](https://github.com/rushsinging/aemeath/issues/915) 已删除 Policy warning façade、重复 scanner 与空 adapter 层，Context `guidance` 成为唯一 assessment owner；[#916](https://github.com/rushsinging/aemeath/issues/916) 已删除剩余 path façade与 Tool path metadata，Project `WorkspaceRead` 独占安全解析、Tool adapters 独占 Bash/read-before-write；[#917](https://github.com/rushsinging/aemeath/issues/917) 已建立 PolicyRequest/Decision/Port 与 AllowAllPolicy，CLI `--yolo`/`--allow-all` 完成 alias ACL；[#918](https://github.com/rushsinging/aemeath/issues/918) 负责 production consumption 与 allow_all 退役。Storage 由 [#991](https://github.com/rushsinging/aemeath/issues/991) 先行为等价提升为 `memory_store` / `task_store` / `tool_result` 顶层过渡模块，删除 `api/business/contract/gateway`、仅测试可达的旧 History 与 `storage::api`；该布局 **NEVER** 视为 Storage Target。#880 建立 Storage `domain + ports + adapters` 骨架与 SafePath/AtomicBlob 基础，#983 已在同一 Hexagonal 层内增加独立 AtomicDataset port/adapter 与 Prepared→roll-forward 协议，未新增 Guard exception；#883/#884 迁出或退役过渡业务语义，Memory integration 由 #896 承接。退出时 Guard **MUST** 证明层间单向依赖、domain 零物理 I/O、adapter 类型不进入 PL、crate-root façade 保持窄。其余 feature **MUST** 在 #743 原生树内由模块 leaf 独立迁移；Project 由 [#892](https://github.com/rushsinging/aemeath/issues/892) 承担；Config 的目录前置由 [#999](https://github.com/rushsinging/aemeath/issues/999) 完成，后续语义收口不得恢复旧 `config/adapter` 或根部散点模型路径。[#763](https://github.com/rushsinging/aemeath/issues/763) 是治理父项，正式 Guard 与全局故意违规证明归 [#982](https://github.com/rushsinging/aemeath/issues/982) / #1022 |
| O2 | `check-cola-layer-purity.sh` 仍在 Stop 阶段运行；其检查行为、常量与白名单见 [Architecture Guards](01-architecture-guards.md)。#1002 已确认 Composition Root 当前 `app/provider/runtime/tools/update` 扁平 wiring modules 符合 capability-first；#948 已让 `FeatureGateways` 的 Provider/Tool factory 注入真实进入 Runtime 主 bootstrap，并由 `check-composition-layout.sh` 的零例外正向断言锁定；Composition 不机械复制 feature crate 的 Hexagonal/COLA 层 | 由守卫机械验证 capability-first 新规范的窄公开面、跨 feature 依赖、循环依赖与 Composition Root 装配；Composition 继续按装配职责扁平分片，语义接线完整性不由目录形状冒充 | [#982](https://github.com/rushsinging/aemeath/issues/982) 是 #763 的原生实现 leaf；替代守卫证据齐备前 legacy guard **MUST** 保持运行。#948 只闭合现有 Provider/Tool gateway 的主 bootstrap 消费，不宣称 Runtime 已退出 `LlmClient/LlmClientPool` 或 Tool 已完成双端口；全部 Adapter 构造上移由 #950 承接，Provider P1/#907 与正式 capability-first 边界 #1022 继续开放 |
| O3 | `WorkspaceService::new(cwd)` 内部选择 `GitCli`，`with_git(cwd, git)` 作为测试注入特例；`WorkspaceService`、`GitCli`、`GitWorktreeOps` 当前经 Project API 间接暴露；写用例持 `WorkspaceState` lock 执行 Git I/O，且缺少统一 control-operation 串行器 | `WorkspaceService` 只保留 crate-private 注入构造；`wire_production_workspace(cwd)` 是 composition-only opaque factory，在 Project 内构造私有 `GitCli` 并返回 `WorkspaceWiring`。每个 context 只有一个同步 state slot 与一个同步 control-operation mutex；fallible I/O 不持 state lock，成功后一次提交完整 candidate。Project factory 只负责私有构造，**NEVER** 读取全局配置或选择候选实现 | [#892](https://github.com/rushsinging/aemeath/issues/892) 收敛 Project 目标目录、私有 Git seam、opaque wiring、锁模型与 fork 隔离；[#893](https://github.com/rushsinging/aemeath/issues/893) 完成 Composition 唯一消费点、scope 生命周期和窄 view 切换；[#894](https://github.com/rushsinging/aemeath/issues/894) 提供 identity / NonGit / snapshot / restore。#892 的退出证据 **MUST** 覆盖所有写用例共享串行器、Git I/O 期间读者可观察完整旧 state、失败零部分提交、父子锁隔离，以及 Project 公共面零 `WorkspaceService` / `GitCli` / `GitWorktreeOps`；factory/handle 仅 Composition 消费的跨 crate 机械守卫与故意违规证明归 [#982](https://github.com/rushsinging/aemeath/issues/982) / #1022 |
| O4 | Runtime 已有未接线的 `core/ports/workspace_port.rs` 骨架；生产链的 `RuntimeHandle`、`ChatLoopContext` 与 `ToolExecutionContext` 仍持有或转发具体 workspace；当前启动构造的 workspace 跨回合复用 | 删除 Runtime `WorkspacePort` 与 RuntimeContext workspace 字段；Composition 为 active Main session slot 保留跨 Main Run / resume 复用的私有 `CompositionWorkspaceScope`，只在 Main agent 启动时建立 production wiring；Sub 从父 scope 派生 Run-lifetime 隔离 wiring，再把同一实例的窄 view 装配给 Context / Tool backing implementation | [#893](https://github.com/rushsinging/aemeath/issues/893) 负责 Runtime / Tool / Composition 消费方切换与 Main scope 生命周期；完成时 **MUST** 证明 Run N 的 cd / worktree 状态进入 Run N+1，并删除占位 port、旧具体引用与第二状态源。边界守卫实现归 #982，#763 汇总验收 |
| O5 | #910 已将 `ExecutionScope` 固定为八个纯值字段、`ToolExecutionContext` 锁定为私有 `scope + ports`，删除旧资源总包，并把 `WorkspaceViews` 转换移到 Runtime adapter；统一 `WorkspacePorts`（Read+Control+Persist+Isolation）已退役，Runtime 自持 Persist，Control 已按 Bash / EnterWorktree / ExitWorktree constructor 注入，context 不再广播 Control。Agent dispatch 的其余兼容 access 仍随 context 存在 | Composition 按 Tool 实例注入 Project-owned view：只读文件 Tool 只有 `WorkspaceRead`，Bash / EnterWorktree / ExitWorktree 才同时获得 `WorkspaceControl`；Tool **NEVER** 接收 `WorkspaceService` 或 `WorkspaceWiring` | [#893](https://github.com/rushsinging/aemeath/issues/893) 继续完成逐 Tool constructor 注入和测试；[#982](https://github.com/rushsinging/aemeath/issues/982) **MUST** 用故意违规证明第四个 Control 消费者与全 Scope 广播均被拦截。#911 Catalog/Execution adapter、#877 typed suspension；#897 正式 `MemoryPort` 已替代临时 Memory compatibility bridge、#912 Runtime scope ownership 均明确未完成 |
| O6 | TUI `UiEvent` 仍携带多种 SDK DTO 与 AskUser `oneshot::Sender`；AskUser 在第二层 ACL 返回空 mapping 后由 `ui_event.rs` 直接写 Model / input 并发送 reply；workspace mapper 同步执行 git；部分 mapper / reducer 可直接产生或执行 Effect；View / Model 尚有重复与越权写入面 | 唯一链路为 SDK event → `event_mapping` TUI DTO → `AgentEventMapping` intents → reducer Change → Coordinator Effect → effect runner → result Intent。Runtime 生成 interaction request id 并保有 waiter / continuation；SDK event 只携可序列化纯值，TUI Effect 经 AgentClient reply / cancel command 回传且全树零 sender / registry。command result 不推进 Run，Run 恢复 / 两阶段取消只投影 SDK 权威事件；六 Context 核心字段私有且 reducer 唯一写；结构化 Conversation 与 timeline 是原子维护的互补投影；Workspace metadata 由带 root + revision 的异步 Effect 回填 | Runtime / SDK identity 与 HardPause 归 [#874](https://github.com/rushsinging/aemeath/issues/874) / [#878](https://github.com/rushsinging/aemeath/issues/878)；TUI [#943](https://github.com/rushsinging/aemeath/issues/943) / [#944](https://github.com/rushsinging/aemeath/issues/944) / [#947](https://github.com/rushsinging/aemeath/issues/947) 的精确责任与退出条件见 §1.2；全局守卫实现归 #982 |
| O7 | Task restore 当前不校验，并依次替换四个独立 async-mutex state；Project 只校验当前 root / path 存在后修改 live state，未完整校验 frame / repo；Config 的 global current / watch、Memory 打开与 Session 恢复缺少统一切换协议；旧 Workspace snapshot 缺少稳定 `WorkspaceId` / `ProjectIdentity`，跨项目 resume 可能继续沿用启动 identity | Task 使用 Task-owned `TaskId` / `BatchId` 与不含派生 `blocks` 的 `PersistedTask`，并把全部字段收进单一同步 `TaskStoreState` slot；Project 以 `ProjectIdentity` / `WorkspaceId` / `WorktreeKind` 表达 Git 与合法 NonGit，并通过无副作用 prepare + 无失败 commit 恢复完整 state。resume 先取消 / join active shared lease holders，调用栈自身不持 shared lease，再取得 owned exclusive session-switch lease；读取 Session、Project → Config → Memory → Task 的 prepare、durable commit 与最终 publish 全部在同一 lease 内完成，Config watch 最后发布 | [#890](https://github.com/rushsinging/aemeath/issues/890) 提供 Task 强类型 PL、单一 state slot / token、删除边清理与 snapshot round-trip；[#894](https://github.com/rushsinging/aemeath/issues/894) 独占 Project identity、NonGit、完整 path / frame / repo 校验、snapshot/prepare/commit 与旧 Session 兼容，**NEVER** 由 #892 复制临时协议；[#893](https://github.com/rushsinging/aemeath/issues/893) 把 Project persist view 接入 Composition/Context backing；[#871](https://github.com/rushsinging/aemeath/issues/871) 实现联合协调器、participant 与唯一 exclusive session-switch gate，Project **NEVER** 自建或声称持有该 gate；[#933](https://github.com/rushsinging/aemeath/issues/933) 定义 ConfigQuery / ConfigWriter delivery seam。退出证据 **MUST** 覆盖 shared → exclusive 升级为零、每个 prepare / durable await / publish 注入失败或取消点、任一 prepare 失败时全状态不变、跨项目恢复后所有消费者只读写目标 backing、Config watch 不早于 backing install、prepare token 与 commit 之间无外部 mutation，以及整个切换窗口不可被 Main Run、query、subscribe 或命令观察 |
| O8 | Memory / Storage / Prompt / Workflow / Interaction / Config 的 Target 文档已有局部方向，但部分 leaf 正文未冻结 revision CAS、typed committed receipt、async materialization、ReasoningPort OHS 与 SDK interaction command | Memory mutation 采用 candidate + dataset CAS + committed receipt；Prompt 只经 Context-private async pipeline 与 supplier seams；Workflow graph 只经 ReasoningPort observe/current；Runtime interaction identity / waiter 权威且 SDK/TUI 只交换纯值；Config 只经 project-aware participant 与 AgentClient delivery | Memory [#895](https://github.com/rushsinging/aemeath/issues/895)–[#900](https://github.com/rushsinging/aemeath/issues/900) / [#984](https://github.com/rushsinging/aemeath/issues/984)，Storage [#880](https://github.com/rushsinging/aemeath/issues/880) / [#882](https://github.com/rushsinging/aemeath/issues/882) / [#983](https://github.com/rushsinging/aemeath/issues/983)，Prompt / Skill / Git [#870](https://github.com/rushsinging/aemeath/issues/870) / [#912](https://github.com/rushsinging/aemeath/issues/912) / [#894](https://github.com/rushsinging/aemeath/issues/894)，Workflow [#919](https://github.com/rushsinging/aemeath/issues/919)–[#921](https://github.com/rushsinging/aemeath/issues/921)（**#921 收缩：Provider resolver 领域迁移完成但未接生产链路；Config `reasoning_graph` 退役；五节点固定默认 effort；是否保留/接线由 v0.2.0 #1142 决策**），Interaction [#874](https://github.com/rushsinging/aemeath/issues/874) / [#878](https://github.com/rushsinging/aemeath/issues/878) / [#911](https://github.com/rushsinging/aemeath/issues/911)，Config [#871](https://github.com/rushsinging/aemeath/issues/871) / [#933](https://github.com/rushsinging/aemeath/issues/933) / [#934](https://github.com/rushsinging/aemeath/issues/934) 承接。**每个能力只有在以下可验证证据齐备后退出 O8**：唯一 owner / OHS 签名已在对应 Target 文档冻结；leaf PR 附契约或场景测试覆盖成功、pre-commit 失败、post-commit warning/取消竞争等其适用分支；旧 public path / duplicate trait / 第二状态源已删除；#982 对该边界的正例与故意违规反例均通过；父 Issue 和 Release Gate 已同步。任一能力未满足时 O8 保持未完成，#972 本身不承载代码 PR |
| O9 | #885–#888 已建立 `agent/features/task` 所有者 crate、单一 `TaskStoreState + TaskRevision` backing、同 Batch 原子 DAG、严格状态机、Batch lifecycle 与版本化 snapshot codec。#889 已把 Task Tool、Runtime reminder/status/finalize 的日常访问切到 Task-owned `TaskAccess`，停止发布/消费 Tool `owner`，并以短期 ACL 将 Task PL 投影为 legacy Tool result DTO。旧 Shared/Storage backing 仍供 Session persistence/input-gate 兼容路径使用；Runtime 中 legacy `TaskPort` / `TaskStorePort` 与第二状态源仍待后续切断 | Task BC 独占 Published Language、聚合、执行时间事实与 lifecycle 领域策略且不依赖 Agent 身份；正式 backing 只持一个同步 `TaskStoreState + TaskRevision` 状态槽，每次实际成功 mutation 原子提交 state、稳定 events 与单调 revision。Runtime / Tool 只持 `TaskAccess`，Context Management 只持同 backing 的 `TaskPersist`。v0.1.0 不建立 `TaskId ↔ AgentId` / `TaskAssignment`；LLM 决定执行方式，未来只有出现可验证的调度、取消或审计需求时才由独立 Runtime Issue 设计。legacy ID 重用、第二状态源、owner DTO 与任意 update closure 最终全部退役 | [#889](https://github.com/rushsinging/aemeath/issues/889) 完成 Tool/Runtime Access ACL、typed subject/description 命令、直接 Pending→Completed 单提交、owner 停止发布与 `runtime/tools → task` 依赖守卫；[#890](https://github.com/rushsinging/aemeath/issues/890) 承接同一 backing 的 `TaskPersist`、Session snapshot source/restore adapter 与 legacy persistence 切换；[#877](https://github.com/rushsinging/aemeath/issues/877) 承接统一 `tool_coordination` 事件投影；[#879](https://github.com/rushsinging/aemeath/issues/879) 退役 Runtime 重复 `TaskPort` / `TaskStorePort` 与旧生产入口；[#891](https://github.com/rushsinging/aemeath/issues/891) 删除 shared/storage legacy façade、0 哨兵、ID 重用路径、owner DTO 与任意 update closure，并收口 Guard |
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
| R7 | **Sub 绕过统一 PolicyPort** | Sub tool 执行直接继承 `allow_all` bool，无统一决策入口 | v0.1.0 Main/Sub 都调用 AllowAllPolicy；Future Deny/Approval 另行设计 | S3/S5 |
| R8 | **SDK identity / projection 契约已建立，生产 Interaction 路由仍待切换** | #874 已发布 `RunStepId` / `AgentId` / `InteractionRequestId` 与纯值 Interaction request/reply/cancel/outcome PL，并把领域事件与 stream event 的纯转换收口到 `adapters/event_projection`；`ChangeSet`/channel send 仅在 sink adapter。当前 `RuntimeTurnContext` 仍只有 `chat_id/turn_id`，旧 `AskUserBatch.reply_tx` 仍作为生产兼容路径可达 | #878 将 `AgentId` 与 request id 接入 Run/PendingInteraction，切换 `ChatEvent::InteractionRequested` 与 AgentClient reply/cancel；#943/#944 删除 TUI sender；#879 删除旧 AskUser/取消 compatibility projection。退出证据：SDK/TUI 生产事件零 sender，Main/Sub identity 无损，projection 是唯一纯 ACL | #874（契约/ACL）→ #878/#943/#944/#879（生产切换） |
| R9 | **RunSpec 配置散 4 处** | `AgentRoleConfig` + `AgentTool` 硬编码 system + 名称排除型 `ToolProfile::SubAgent` + `ModelEntryConfig`(effort) | 收敛为声明式 `RunSpec`，Tool 部分采用 Registry Scope + capability Profile | S3/S5 |
| R10 | **Session `messages`/`chats` 双轨** | 旧扁平 `messages` + 新链 `chats` 并存，加载迁移 | 只保留 `chats`，旧 `messages` 退役 | S5/S7 |

## 3. Tool & Skill & Command 现状缺口（S2 代码盘点）

#993 已完成 Tool crate 的 `domain + adapters` 过渡物理迁移。#909 进一步落地 `RegistryScope`、capability allow-set `ToolProfile` 与 `derive_restricted` 只收缩约束，并将内置工具的名称、required capabilities、Scope 成员关系和 factory 收敛为单一注册规格；Main 使用 `all()` baseline，Sub 与兼容 `legacy-no-agent` 均从 Main parent 经 `derive_restricted` 构造；LSP 按其真实外部进程副作用声明 `ExecuteProcess`，TaskGet/TaskList 使用独立 `TaskRead`。历史 `register_all_tools*` 入口保持集合兼容，其中旧 `NoAgent` 明确映射为 `legacy-no-agent` Scope，待 #914 退役。该实现目前仍在具体 `ToolRegistry` 注册路径做 Scope/Profile 过滤，**不代表** #911 所规划的 Catalog/Execution adapter、Runtime 双端口接线、`ExecutionScope` 或 typed suspension 已完成。尤其 MCP 动态注册当前仍绕过 `RegistryScopeBuilder` 与 Scope/Profile 校验；该缺口属于 #911 / MCP Ready，#909 **没有**建立“所有 Tool 全局注册均受 Scope/Profile 约束”的不变量，且在 #911 adapter 落地前不提前改写 MCP/ToolRegistry 调用链。

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| T1 | **Runtime 直持 Registry / Tool 实例** | Runtime 从具体 ToolRegistry 按名称取得 Tool 并调用；目录查询与函数执行无端口隔离 | `ToolCatalogPort` + `ToolExecutionPort`；只交换 Descriptor/Invocation/Outcome | S5 |
| T2 | **Scope/Profile 基础已落地，历史入口待退役** | #909 已用 capability allow-set 与 `derive_restricted` 替代内部 ToolName 黑名单，并从单一内置规格构造 `main` / `sub-agent` / `legacy-no-agent`；旧 `register_all_tools*` façade 仍供生产兼容 | #914 删除 `legacy-no-agent` 与历史注册入口；Profile 始终只收缩、不扩权 | #909（基础完成）/ #914（退役） |
| T3 | **内置 Scope/Profile 已进入具体 Registry 注册路径，MCP 与双端口未接线** | #909 已按 `Registry Scope ∩ Profile Allowed Capabilities` 过滤内置工具，但 MCP 动态注册仍绕过 `RegistryScopeBuilder`/Scope/Profile，且 Scope/Profile 尚未成为 Runtime Catalog/Execution 两端的完整调用契约；不得声称全局注册不变量完成 | #911 / MCP Ready 将动态工具经保守 capability ACL 接入 Scope；Main/Sub 按 RunSpec 装配 Scope；Catalog 与 Execution 各自复验 Scope/Profile | #911 / MCP Ready |
| T4 | **ToolExecutionContext 泄漏 Runtime / Project 资源** | 执行上下文包含 registry、store、channel、semaphore 与具体 `WorkspaceService` 等活资源，构造点分散 | 最小 `ExecutionScope` + 对应 BC 的窄资源端口；Project view 按 Tool 实例注入；AskUser 返回 typed suspension 而非注入 channel / `UserInteraction`；禁止传 RuntimeContext / Registry / Store / WorkspaceService | S5（Workspace 见 #893） |
| T5 | **Tool 调用职责分散** | schema、timeout、并发、Policy/Hook/审批与实际调用跨 Runtime/Tool 实现散落 | Tool BC 强制存在性/Scope/Profile/schema/函数调用并可产生 `ToolSuspension`；Runtime 经唯一 `InteractionPort` 编排 Policy/Hook/审批/await-resume/timeout/并发/取消/重试 | S3/S5 |
| T6 | **取消接口绑定实现细节** | Tool 执行依赖具体 cancellation/channel 形态，长进程/网络调用的协作停止不统一 | Tool PL 定义只读 `CancellationSignal`；Runtime 适配 cancellation tree 并拥有 timeout | S5 |
| T7 | **Tool 结果责任混合** | Tool 字符串结果、结构化 data、错误、截断/落盘和 UI 展示边界不统一 | `ToolOutcome` 保留领域结果；token budget/截断归 Context Management，持久化归 Storage，渲染归 TUI | S5 |
| T8 | **Skill 被包装成 Tool 且物化跨域** | SkillTool 只返回 loaded/path，实际内容由 prompt 路径物化；Skill 与 Tool 执行语义混合 | 独立 SkillCatalog/Materialization 端口，输出 PromptFragment 给 Context Management | S5 |
| T9 | **Slash Command 堆在 Runtime idle 流程** | 命令 parser/执行散在 idle_commands/input gate，查询、写命令与 prompt 注入混合 | Command Catalog/Router 按 PromptInjection、SnapshotQuery、ApplicationControl 路由至目标 BC | S3/S5 |
| T10 | **MCP 生命周期为隐式 Manager** | 连接状态由多个方法散点修改；health check、tool list diff/refresh 与 resource 路径未完整接线 | 显式 `McpConnection` 状态机；仅 Connected 发布 Catalog 投影，变化原子撤销/更新 | MCP Ready 后 |
| T11 | **MCP Tool Catalog 一致性不足** | disconnect 后目录撤销、动态上下线、annotations capability 映射及事件通知未形成统一契约 | MCP ACL 转 Tool PL；CatalogChanged 通知重新拉取 Snapshot；连接/投影一致 | MCP Ready 后 |
| T12 | **MCP 稳定身份与版本未定** | 动态工具尚未形成可验证的稳定 ID、schema 版本和 Catalog revision 协议 | MCP 正式接线时单独设计 ToolId、rename、版本与 in-flight 兼容；当前不预设 | MCP Ready 后 |

## 4. Provider 现状缺口（S2 代码盘点）

#901 已冻结 Runtime-owned `ProviderPort`、中立 Invocation Published Language 与 fake contract harness，但只完成契约定义，未完成生产链切线：Runtime 当前仍直接持有 `LlmClient` / `LlmClientPool`，因此 P1 保持未关闭，**NEVER** 将 #901 解释为具体 client 已退出 Runtime。#992 已将 Provider 物理结构迁为 `domain/`、`ports.rs`、`adapters/` 与 crate-root 窄 façade，并删除 13 个 COLA 迁移例外。#902 已完成不可变 Invocation Scope 生产切线：Provider 请求构造只读 scope，删除 provider 侧调用期 atomics / setter、Sub finalize restore 与 shared-client serialization lock。#903 已完成 P4 跨 BC 生产切线：Provider 返回 pull-based `InvocationStream`，Runtime Main/Sub/Reflection 主动 poll `InvocationEvent`，Runtime/Context 的生产代码与测试替身禁止消费 legacy sink；Provider driver 内部 decoder 暂留隐藏迁移桥，登记在退役清单而不冒充已物理清零。P2、P3、P4 已关闭；#1033 交付 crate-private `HttpAttemptExecutor`，收敛单 attempt 机械（cancellation-aware send/status 判定、安全 headers、16KiB bounded error body、typed network/HTTP transport failure 分类、单一 diagnostic），Anthropic/OpenAI-compatible/Ollama 全量迁入；但跨调用 retry/backoff 与 stream→non-stream fallback 的所有权仍未迁至 Runtime，P6、P7 保持未关闭，**不冒充** Runtime 已完整拥有该所有权。跨调用 retry/backoff（P6）、stream→non-stream fallback（P7）与错误分类统一（P9）由 [#905](https://github.com/rushsinging/aemeath/issues/905) 承接。其余语义缺口继续按下表治理。

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| P1 | **Runtime 依赖具体 client/pool** | Runtime 直接持有并调用 `LlmClient` / `LlmClientPool`，ProviderInfoPort 只覆盖元数据 | Runtime 只依赖自有 `ProviderPort` 与稳定 Invocation PL；具体 provider 仅在 Composition Root 装配 | S3/S5 |
| P2 | **调用期配置为共享可变状态（已对齐 #902）** | `InvocationScope` 冻结 model / max tokens / requested/effective reasoning；Anthropic、OpenAI-compatible、Ollama 请求编码只读 scope，provider 不再发布调用期 setter | 后续接入完整 `InvocationRequest.options` 与 capability fingerprint，不恢复共享 current state | 已完成（#902） |
| P3 | **Main/Sub client 并发踩踏（已对齐 #902）** | Main/Sub 每次调用各自构造 scope；已删除 `shared_client_lock`、previous/restore 字段和 finalize 恢复分支，取消或 panic 不会修改其他调用配置 | 后续可继续共享不可变 Transport / HTTP pool，并将具体 client 依赖收口到 Runtime-owned port | 已完成（#902） |
| P4 | **流协议依赖多方法回调（已对齐 #903）** | `LlmProvider::invocation_stream` 是生产实现与测试替身的必实现入口；Runtime Main/Sub/Reflection 主动 poll 封闭 `InvocationEvent`，`Completed/Failed` 单终结，取消为 `Failed(Cancelled)`；架构守卫禁止 Runtime/Context 引用 legacy sink | Provider decoder 内部迁移桥继续保持 crate 内且不得跨 BC；最终物理退役见下方清单 | 已完成（#903） |
| P5 | **wire DTO 发布面过宽** | Provider contract/api re-export 含供应商 request/stream payload、client config 和具体构造类型 | wire request/response/SSE DTO 全部留在 driver adapter；跨 BC 只交换 Invocation PL、ModelCapability 与 Message | S5/S7 |
| P6 | **跨调用重试下沉到 Provider**（由 [#905](https://github.com/rushsinging/aemeath/issues/905) 承接） | 各 provider 内部自行 attempt/backoff，策略与日志不一致，Runtime 无法完整拥有 attempt 事件；#1033 已收敛单 attempt 机械（send/cancel/status/诊断）到 crate-private `HttpAttemptExecutor`，但 attempt 计数、退避与是否重试的决策仍留在各 driver，未迁入 Runtime，P6 未关闭 | Provider 一次 invoke 只做一次上游语义请求并分类错误；Runtime 统一 retry/backoff/compact/final failure | S3/S5 |
| P7 | **stream → non-stream fallback 隐式重发**（由 [#905](https://github.com/rushsinging/aemeath/issues/905) 承接） | 部分 driver 在流失败后于 Provider 内再次请求；已输出内容时存在重复或归因不清风险；#1033 引入的 `AttemptDisposition` 只统一了失败的日志级别与 retryable 标记，fallback 的实际发起逻辑仍在 driver 内部，P7 未关闭 | fallback 必须由 Runtime 作为新 attempt 显式编排；每次 attempt 独立事件、usage 与取消 | S5 |
| P8 | **reasoning 能力与 clamp 分散** | driver、provider、Runtime 与 model 配置分别处理上限/字段；Anthropic、OpenAI-compatible、Ollama 路径不统一 | Workflow 固定默认 desired effort（Config `max_reasoning` 已退役，#921）→ Provider/model capability clamp；Provider 统一能力解析与 wire 映射。**v0.1.0 scope**：resolver 领域迁移完成但未接生产链路；是否接线由 v0.2.0 #1142 决策 | S3/S5 |
| P9 | **错误分类不稳定**（由 [#905](https://github.com/rushsinging/aemeath/issues/905) 承接） | HTTP、网络、stream、取消和 context 超限在多路径转换，部分上层依赖字符串判断 | `ProviderErrorKind + retryable + safe provider code`；Runtime 只按结构化语义编排 | S5 |
| P10 | **Usage 与成本边界已建立 Provider 原始事实 seam（#906）** | pull-stream bridge 直接从 Anthropic/OpenAI Chat/Responses/Ollama wire 事件提取 `RawUsageSnapshot`；未报告保持 `None`、真实零保持 `Some(0)`，完全无 usage 时 completion 为 `None`；legacy `Usage` 与 Runtime/Audit attempt 关联仍待后续切线 | Provider 标准化 RawUsageSnapshot；Runtime 在 retry/fallback 收口后关联逻辑 Model Invocation；Audit MVP 只存 Usage，Cost/Pricing 保留 Future | Provider PL 已完成（#906）；Runtime/Audit 后续 |
| P11 | **能力查询已建立单一 driver reasoning capability（#906）** | OpenAI-compatible driver 以唯一 `ReasoningCapability` 声明 supported levels/mapping，legacy maximum/clamp 从其派生；完整 driver+model+deployment resolver 尚未接生产 | 发布只读 ModelCapability，未知能力保守处理，并在编码前再次复核 | Provider 声明完成（#906）；生产 resolver #1142 |
| P12 | **具体 Provider 构造点分散** | client/provider/pool 工厂与默认 fallback 可在 Provider/Runtime 路径内发生，缺少唯一装配边界 | Composition Root 独占 Transport、driver、凭证与 ProviderPort adapter 构造；缺失配置显式失败 | S5/S7 |

## 5. Memory 现状缺口（S2 代码盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| M1 | **MemoryPort 契约已建立但生产链未切换** | #895 已在独立 `memory` crate 发布 `MemoryId`、typed envelope/error、MemoryPort 与 in-memory contract baseline；Runtime 旧骨架和 Storage `MemoryStore` 仍在生产链 | #896 实现 durable MemoryService/adapter，#897 切 Context/Tool/Composition，#900 删除 Runtime 旧骨架与跨模块 façade | #895（契约完成）/ #896/#897/#900（切线退役） |
| M2 | **MemoryService 与 adapter 已建立，旧 Store 待切除** | #896 已在 Memory crate 建立 candidate/CAS/receipt `MemoryService` 与 Memory-owned AtomicDataset adapter；Storage 旧 `MemoryStore` 仍承载旧消费者 | #883 切换 Storage 内部路径，#897 切 Context/Tool/Composition，#900 删除旧 façade 与重复规则 | #896（完成）/ #883/#897/#900 |
| M3 | **检索为子串匹配** | `entry_matches` 朴素小写 contains，无相关性排序 | Tier 1 BM25 关键词相关性排序 | #551 |
| M4 | **similarity_threshold 仅用于去重** | 检索不接入 threshold | 检索也用 threshold 过滤低相关结果 | #551 |
| M5 | **Reflection 领域已归 Memory，执行/TUI 链待收口** | #898 已迁移 PL、prompt/schema/parse/format/apply，Runtime runner 只保留触发与 Provider 调用；旧 TUI 主动展示链暂留至 history adapter 可用 | #899 交付异步单槽/history store并切 `/reflect` 查询；#900 清理最终旧符号 | #898（领域完成）/ #899/#900 |
| M6 | **ReflectionPromptPort 与 history query 已发布** | #898 发布 Memory-owned ReflectionPromptPort/PL 与 ReflectionHistoryQuery；Runtime 已消费 prompt/parse/format/apply | #899 实现执行协议、history persistence/query adapter；Memory 不依赖 Provider | #898（契约完成）/ #899（adapter） |
| M7 | **Memory 配置已进入唯一 open/retrieve 路径，旧注入待切** | #897 Composition 从 committed ConfigSnapshot 构造 MemoryPolicy，Context adapter 使用 enabled/inject_count；Runtime 旧 `memory_inject` 仍有硬编码路径 | #984 切自动注入生产路径并删除硬编码；#900 清理旧 helper | #897（新路径完成）/ #984/#900 |
| M8 | **SessionReminder 在 Memory** | `share::memory::session_reminder` 是会话级数据 | 迁移到 Context Management（Session 聚合）| #870 |
| M9 | **NoOpMemory 与 Sub Disabled 已落地** | #897 发布 owner-owned NoOpMemory；Sub ToolResources 明确装配 NoOp，Composition MemoryMode::Shared 仅 clone active Arc | #899 接 Reflection/Sub 生产编排；#900 删除 Runtime 重复 MemoryPort 骨架 | #897（完成）/ #899/#900 |
| M10 | **v2 identity key 与 legacy 迁移 seam 已建立，生产 source 待接** | #896 已按 git common dir / non-git initial cwd 派生不泄漏路径的 `v2_` key，并由 `ProjectMemoryOpener` 对 new/legacy 冲突 fail-closed、通过 AtomicDataset CAS 迁移裸 legacy active/archive；物理旧文件 source 尚未在 Composition 接线 | #897 注入 ProjectIdentity 并绑定 production LegacyMemorySource；#900 在迁移完成后退役旧文件路径 | #896（协议完成）/ #897/#900 |
| M11 | **查询契约已冻结但生产仍夹带 I/O** | #895 已冻结纯内存查询、统一 `MemorySearchResult` envelope 与 typed mutation error，并由 in-memory fake contract 证明 revision 不变；旧 concrete store 仍在查询时读文件，生产 adapter 尚未切线 | #896 交付 eager-read + candidate/CAS/receipt MemoryService；#897/#984 切消费方；#900 退役旧 top query | #895（契约完成）/ #896/#897/#984/#900 |
| M12 | **active/archive 已同代提交，旧 Store 待退役** | #896 的 global/project 各自使用独立 AtomicDataset generation，层内 active/archive 两 member 原子提交；旧 `MemoryStore` 仍顺序写文件 | #883/#900 删除旧分文件写路径；跨层 compact 保持两个可观察 layer command，失败不伪装全局成功 | #983/#896（机制与 adapter 完成）/ #883/#900 |
| M13 | **Main Memory 单次 open 与同 Arc 分发已建立，resume gate 待接** | #897 Composition 按 ProjectIdentity + committed MemoryConfig 打开真实 service，RuntimeResources/ToolResources 复用同一 Arc；ActiveMemoryWiring 提供 prepare/install 与 Arc identity contract | #871 在 exclusive gate 内安装 candidate；#984 切 Context production injection；#899 接 Reflection view | #897（启动/wiring完成）/ #871/#984/#899 |
| M14 | **mutating 注入路径与死方法** | PR #575 已交付的 `top_for_inject_readonly` 仍绑定 legacy `MemoryStore`，而 `top_for_inject` 会 touch / 写盘且只被测试引用 | #984 将主动注入切到 active session lease 上同一只读 `retrieve_for_inject`；#900 删除两个 legacy top query。访问统计若需要必须是显式 fallible command | #984 / #900 |
| M15 | **Memory 共享内核存在重复公开入口** | #997 已删除只做 re-export 的 `share::memory_ops`，Tools 消费方统一经 `share::memory`；`memory.rs` + `memory/` 保持 Rust 2018+ 模块布局。该 PR 不迁移领域所有权、不移动 SessionReminder、不改变检索、去重或评分行为 | #895–#900 将 `share::memory` 中的领域语言与行为迁入独立 Memory capability；#870 承接 SessionReminder 所有权迁移 | #997（目录收口完成）/ #895–#900（语义迁移） |

> **#997 Guard / 白名单审计**：覆盖 `agent/shared/src/memory{.rs,/}` 的 `check-share-no-upstream-deps.sh` 与 `check-share-minimal-kernel.sh` 均按整个 share crate 扫描，不含 Memory 路径 allowlist、整文件豁免、行级 `allow`、`grep -v`、exclude 或 skip；`per_file_exemptions = {}`。本次公开入口收口未修改 Guard，白名单预算保持 `0 → 0`，无需向 #1021 登记迁移例外；`check-cargo-dependency-graph.sh` 仅将 `share::memory::*` 记为 Memory 当前物理落点，事实未变化。

## 6. Context Management / Config 现状缺口

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| CM1 | **Compact 管线完成度与所有权混杂** | #870 已建立 `WindowProjector` 纯 projection seam 与 Context-owned candidate；L2/L4 算法尚未落地，旧 Runtime L3 仍直接改 `ChatChain` | L1 在 ToolResult 进入 `ChatChain` 前完成；L2-L4 只变换 `ContextWindow` 读模型，L5 经 ContextPort 修改稳定 Session backing；fingerprint / circuit breaker / manual bypass 语义唯一 | #870（seam）/ #548 / #552 / #554（算法）/ #876（旧路径删除） |
| CM2 | **Token budget 常量和触发散落** | #870 Application Service 已只使用 `ContextRequest.max_output_tokens` 计算 candidate decision；Runtime 旧路径仍有固定 `8192` 与重复阈值检查 | Provider capability 提供模型上限；TokenBudgetConfig 单一来源；manual / auto 入口分明；fingerprint 增量估算且 Hook 只在真实 compact 时触发 | #870（Context 侧）/ #550 / #553（算法）/ #876（Runtime 切线） |
| CM3 | **Prompt 多入口且无私有 capability seam** | #994 删除了旧 COLA 空壳；#870 将 Prompt 技术 detail 迁入 `adapters/prompt` 并建立 Application Service/purpose seam；Runtime 仍直接调用 guidance/skill/compact/session façade | Prompt 策略进入 domain，I/O detail 终止于 adapter；ContextPort build_window 是唯一 Runtime 入口 | #870（Context 侧完成）/ #876（Runtime 切线） |
| CM4 | **Guidance / Skill / Git Context 边界不完整** | SKILL.md 扫描缺失；Prompt 散点执行 git / 读 cwd；user guidance 只取首个文件且 alias / canonical 去重不统一 | Skill-owned materialization + 全覆盖扫描；Project WorkspaceRead snapshot 经 ACL 注入；每目录 AGENTS-first / CLAUDE fallback、多层有序、canonical 去重 | #870 / #912 / #894 / #965 |
| CFG1 | **Config adapter 与 durable protocol** | #934 已实现 File/CLI/Compatibility/Claude adapters、稳定格式检测、ConfigValidator 与 AtomicBlob-backed NativeConfigStore；ConfigAppService application 不再直接 fs/JSON 解析，CLI bootstrap layer 已在 Composition wiring 前接入。provider key/env 后处理仍待最终合流 | adapter 输出 ConfigPatch；Application 只编排 layer / validation；外部 CLI 经 translator ACL；Env 只在唯一 adapter 读取 | #934（adapter/durable 完成）/ #935（Env 退役） |
| CFG2 | **reasoning 上限解析已退役（#921 收缩）** | `max_reasoning` 与整个 `reasoning_graph` config 已从 Config 退役 | Config 不再承载 `reasoning_graph`；Workflow 五节点采用固定默认 effort 无 config override；如需 reasoning level 上限控制，由 v0.2.0 [#1142](https://github.com/rushsinging/aemeath/issues/1142) 重新决策 | #921 / #1142 |
| CFG3 | **active Config 非 project-aware 联合切换** | #933 已建立独立 Config BC crate、唯一 ConfigAppService/wiring、ConfigReader/Query/Writer/ProjectConfigParticipant typed seam，并由 Composition 单例注入 Runtime；真实 session-switch shared/exclusive gate、Memory/Task 联合切换与 cancellation-shielded durable commit 尚未实现 | Config 独占 `{location,snapshot}`；Project→Config ACL；Project→Config→Memory→Task prepare；update handoff 后由 owned cancellation-shielded task 完成 durable Config persist、Memory install、Config install、watch 最后发布；非 Run `ConfigQuery::snapshot/subscribe` 先取得 shared permit | #933（seam/单例完成）/ #871（gate/联合协调） |
| CFG4 | **交付层直连 Config 风险（生产构造已收口）** | #683/#696 已收口默认值与裸 Config 消费；#933 已删除 Runtime 散点 `new/load`，model switch/list 复用同一 committed reader，并在 AgentClient 发布 typed `config_view/update_config` SDK DTO；#949 事实复核确认 `ConfigWiring` 的 reader/query/writer/participant 均 clone 同一 `Arc<ConfigAppService>`，Runtime 持有 Arc 因而 service/watch 生命周期不会随局部 wiring drop；同时删除 `trait_reflection.rs` 整文件 Guard 豁免，生产 Config 构造例外归零。TUI/CLI 无 Config 契约泄漏 | #871 提供唯一 gate-aware façade implementation；TUI / CLI 经 AgentClient command + SDK event，CLI args 只作 bootstrap source | #683 / #696 / #933（delivery seam 完成）/ #949（生命周期事实与零例外 Guard 收口）/ #871（gate） |

## 7. Storage 现状缺口（S2 摘要盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| S1 | **Storage 同时拥有业务模型** | Task/Batch 状态、依赖图、Memory 查询与 History 策略寄居 storage crate | Task/Memory/History 所属 BC 独占模型和不变量；Storage 只实现物理端口 | #883 |
| S2 | **原子写机制未复用** | #869 已让新版 Session Envelope adapter 复用 AtomicBlob；#884 已让 Tool Result 经 Runtime-owned 窄 adapter 复用 AtomicBlob；旧 `session_storage.rs` 与 Memory 仍保留重复文件 I/O | 通用 AtomicBlob adapter；数据 BC 的窄持久化端口复用同一机制；Session 旧路径由 #876/#872 切除，Memory 由 #883/#896 收敛 | #869 / #876 / #872 / #883 / #884 / #896 |
| S3 | **backup/恢复协议不完整** | Session 有一代 `.bak`，但备份旋转失败被忽略；其他路径无 previous/quarantine | 原子可见、机械代际读取、领域验证后显式 promote/quarantine | #881 / #882 |
| S4 | **路径与任意物理 Path 耦合** | 多处业务代码拼接 `~/.agents` 路径或直接持有 PathBuf | StorageKey + SafePathSegment；物理根和路径解析只在 adapter | #880 / #883 |
| S5 | **Tool Result 策略落入 Storage（已对齐 #884）** | #884 删除 `storage/src/tool_result.rs`、50K 常量与业务 façade；ConfigSnapshot 发布 validated char policy；Runtime-owned materializer 统一 Main/Sub，写失败保留完整 inline；Runtime adapter 以 write-once/CAS 语义调用 AtomicBlob | 后续只由拥有生命周期的 BC 增加 orphan/retention 清理；Storage 不恢复 Tool Result schema/preview/reference，不发布 AppendLog OHS；旧 `.txt` 引用保持可读但不作为新 AtomicBlob 布局 | 已完成（#884） |
| S6 | **错误与损坏处理不统一** | String/Option/领域错误混用，journal / primary / member digest 歧义可能被当作缺文件、空 dataset 或仅日志 | `StorageErrorKind::CorruptTransaction` + typed reason / transaction scope / quarantine disposition；blob / dataset crash-protocol 矛盾 fail-closed，领域 payload/schema 损坏仍由所属 BC 分类 | #880 / #881 / #882 / #983 |
| S7 | **并发写与临时文件协议未统一** | 固定 `.tmp/.new`，跨实例互斥和残留清扫语义不一致 | 随机 create-new、跨进程锁、commit marker crash recovery | #882 |
| S8 | **只有单 blob 原子性** | #983 已新增独立 `AtomicDatasetPort` / 文件系统 adapter，并以 Prepared journal 为 commit point、读取前 roll-forward、typed corruption quarantine 闭合多 member crash protocol；Memory 仍使用过渡 store，尚未消费该机制 | Storage-owned `AtomicDatasetPort`：dataset lock、全 member stage、Prepared 后只 roll-forward、read-before-recovery；Memory active/archive 与 legacy migration 的领域 adapter 复用同一 primitive | #983（机制已完成）/ #896（Memory 集成 deferred） |

## 8. Logging 现状缺口（S2 摘要盘点）

#1000 已把 Logging 从扁平文件行为等价迁入 `domain + adapters` 物理骨架：crate-root façade 隐藏内部模块，legacy 全局 context、formatter、file sink 与 lifecycle detail 位于 `adapters`，测试期 routing guard 位于 `domain` 入口。由于 formatter 仍读取进程级 context，它在 #937 前保持 adapter 身份；`ports.rs` 只在 #939 建立真实 `DiagnosticSink` seam 时新增。该物理迁移不关闭下列语义缺口。

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| L1 | **Main/Sub 日志上下文互相覆盖（领域与 adapter 已对齐 #937）** | #937 已建立不可变 `LogContext` / `LogContextPatch` / `FieldPatch`、task-local `capture/within/instrument`、formatter 单快照与同步显式 context 路径；legacy setters / `CURRENT_*` 仅为现有生产消费者保留 | #940 迁移 Main/Sub/Provider 生产调用点；#942 删除 legacy 全局状态，并将 scope Guard 精确基线收敛为空 | #937 已完成 / #940 / #942 |
| L2 | **sink 失败可观察并可恢复（已对齐 #939）** | `FileSinkLifecycle` 实现 Healthy/Degraded/Recovering；adapter-private I/O/clock/emergency seam 覆盖 open/write/flush/metadata/existence/remove/rename/reopen；每 sink 独立锁、direct stderr fallback 与固定 5 秒惰性 reopen 已落地 | 异步队列/backpressure、跨进程锁、历史 record 重放、全局 shutdown 与配置热更新另行设计；Logging 继续保持 best-effort，不承担 Audit durability | 已完成（#939） |
| L3 | **TargetCatalog 多份真相（已对齐 #936）** | `domain/routing.rs` 唯一定义 target、owner、sink ID 与文件名；File adapter 和 routing guard 共同消费，旧白名单、文件 match、sink 字段与 flush 清单已删除 | 后续新增生产 target 必须只扩展 TargetCatalog，并由唯一性与全仓 Guard 验证 | 已完成（#936） |
| L4 | **Update target 未注册（已对齐 #936）** | `aemeath:agent:update`、`aemeath:agent:workflow`、`aemeath:context` 已注册独立 sink；未知 target 进入 `aemeath.log` 并直接限频 stderr 报告 | #941 迁移其余真实消费方 target；不为无调用点的 Config/Memory/Task 预建空 target | 已完成（#936）/ #941 |
| L5 | **Logging 与 Audit 混淆** | `agent-audit.log` 是普通诊断 sink | DiagnosticRecord 与 AuditSink 完全分离 | S5/S7 |
| L6 | **Config 参数与 lifecycle 完整接线（已对齐 #938/#939）** | Composition 从 committed `ConfigSnapshot` 构造不可变 `LoggingSettings`；Logging 单次归一化 filter/max-level 与 `max_bytes=0 → 1`，并在初始化和成功轮转后消费 retention days，仅清理同 sink 合法普通非 symlink backup | 配置热更新若需要可更新 policy handle 另行设计 | 已完成（#938/#939） |
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
| PHA1 | **Policy 统一端口已建立，生产消费待切换** | #917 已建立 Policy-owned Request/Decision/Port 与 AllowAllPolicy；Runtime 临时模块只 re-export | #918 切换 Main/Sub production consumption 并删除 legacy approval gate | S3/S5 |
| PHA2 | **`--yolo` 兼容 ACL 已建立，bool 传播待退役** | #917 已以 `--yolo` 主名称 / `--allow-all` alias 映射兼容 bool；Runtime/ToolContext 仍传播 allow_all | #918 将 Runtime 只依赖 PolicyPort 并删除 ACL 之后 bool | S5 |
| PHA3 | **安全 guard 冒充 Policy 风险已收口** | #915/#916 已分别将 content scan、path containment、Bash/read-before-write 归回 Context/Project/Tool；Runtime 仅留 approval 兼容 gate | 所有者局部 guard 保持独立；#917 Policy 只实现 AllowAll decision | S5/S7 |
| PHA4 | **Hook 公开面膨胀且结果分裂** | HookRunner 具体类型 + 多个 on_xxx / plain / JSON / blocking 入口 | 一个类型化 HookPort.dispatch + HookOutcome | S5 |
| PHA5 | **阻断协议不一致** | 部分路径未统一消费 result.blocked / continue=false / decision=block | exit 0/2/other + JSON directive 统一解析，主动 Block 与 ExecutionFailed 分离 | S5 |
| PHA6 | **非零 exit 语义冲突** | 配置注释称 exit 2 阻断；执行器把所有非零视为 blocked | exit 2=主动 Block；其他非零=ExecutionFailed | S5 |
| PHA7 | **Hook 失败无统一重试/回收** | spawn/timeout/wait 失败 fail-open；timeout 可能未 kill/wait 子进程 | 单 Hook 执行故障最多 3 次；timeout 必须回收旧进程 | S5 |
| PHA8 | **Stop Hook 上限伪造完成** | 连续阻断超过 5 次后强制 Done/Completed | Runtime 上限改 15；第 16 次 Failed(StopHookRetryExhausted) | S3/S5 |
| PHA9 | **Main/Sub Hook 行为不统一** | Stop/Hook 路径主要存在 Main loop，Sub 未复用 | 单 Loop Engine + 同一 HookPort；Main/Sub 同规则 | S3 |
| PHA10 | **Hook input/context mutation 未完整消费** | JSON schema 有 updatedInput/additionalContext，但调用链未统一应用 | HookOutcome 类型化 directive；调用方重新 schema/Policy 校验后应用 | S5 |
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
| 旧扁平 `Session.messages` | 迁移期双轨 | 退役 | S7 |
| `legacy-no-agent` Scope 与历史 `register_all_tools*` façade | #909 已把旧 `NoAgent` 精确定义为兼容 Scope；其集合与旧入口保持不变，不再作为 capability Profile 语义 | #914 删除兼容 Scope 与历史入口，调用方改用正式 Scope 装配 | #914 |
| `SkillTool` 伪执行入口 | 只报告 loaded/path，内容在 prompt 路径物化 | SkillMaterializationPort 接线后退役 | S5/S7 |
| Runtime `idle_commands` 命令聚合 | 三种 Slash 机制混在 Runtime idle 流程 | Command Router 接线后拆除旧生产入口 | S5/S7 |
| MCP 旧 wrapper / diff 孤立路径 | 多套 wrapper、diff/refresh/health check 未形成完整生命周期 | MCP Ready 后统一至 McpConnection + ACL；无消费者代码删除 | MCP Ready 后 |
| 共享 client 的 `set_*` / restore 路径（已退役 #902） | Provider 与 Runtime 已无调用期 setter、shared-client lock、previous/restore 字段；每次调用读取不可变 Invocation Scope | `check-provider-invocation-scope.sh` 阻止 atomics、setter、restore 与 serialization lock 回流 | 已完成（#902） |
| Provider 内部 legacy decoder sink | Provider→Runtime 的 `StreamHandler`/gateway/client callback wrapper 已删除；driver decoder 内仍有隐藏 `LegacyStreamSink` 迁移桥，Runtime/Context 无消费者且守卫禁止跨 crate 使用 | decoder 原生发出 `InvocationDelta` 后删除隐藏 legacy trait/re-export；在此之前不得扩张消费者 | S7 |
| Provider wire DTO 公共 re-export | request/stream payload、client config 等由 contract/api 对外发布 | Runtime 迁至 Invocation PL 后收窄可见性并删除无消费者 re-export | S5/S7 |
| Provider 内部 retry / non-stream fallback | driver 内部执行跨调用重试与隐式第二次请求 | Runtime model_invocation 统一 attempt 编排后删除 | S5/S7 |
| `SessionReminders` 在 `share::memory` | 会话级提醒放在 Memory 共享内核，语义不属跨会话记忆 | 迁移到 Context Management 后从 `share::memory` 删除 | S5/S7 |
| `MemoryStore` 领域方法 | scoring/dedup/retrieval 混在 Storage crate 的 MemoryStore 中 | 拆分后领域方法迁到 MemoryService，MemoryStore 降为 Storage adapter | S7 |
| Storage crate 内 Task/Memory 业务实现 | 物理持久化 crate 同时拥有 Task 状态机、依赖图与 Memory 查询行为 | 迁回对应 BC；Storage 仅保留 adapter 与通用机制 | S5/S7 |
| 业务代码散点直接文件写入 | Session/Memory/History/Tool Result 各自实现 IO 语义 | 窄数据端口接 Storage adapter 后删除重复路径 | S5/S7 |
| Logging 进程级 `CURRENT_*` | #937 已建立 scope-local 新路径；legacy 全局状态仅服务尚未迁移的生产消费者，且 Guard 禁止扩张 | #940 切换生产接线后由 #942 删除 setter / `CURRENT_*` / `SESSION_ID` | #940 / #942 |
| 普通诊断 `agent-audit.log` 路由 | 将 Audit 误当诊断 sink | AuditSink 接线后重新定义或删除 | S5/S7 |
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

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-18 | #1179 将 Composition→Runtime bootstrap 的 Workspace/Config/Provider/Tool/Task 散点依赖收敛为 typed dependencies value，删除 9 参数签名并恢复 workspace clippy；不改变 #890 Task backing 或 #929 Audit worker 边界 | [#1179](https://github.com/rushsinging/aemeath/issues/1179) |
| 2026-07-17 | #936 建立 Logging TargetCatalog 单一真相：domain 唯一定义 target/owner/sink/file，File adapter 与 routing guard 共同消费；注册 Update/Workflow/Context 真实 target，未知 target 限频 direct stderr 后写 fallback；L3/L4 关闭，Config/Memory/Task 的真实消费迁移仍由 #941 承接 | [#936](https://github.com/rushsinging/aemeath/issues/936) |
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
