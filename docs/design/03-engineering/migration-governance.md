# 迁移治理 · Current → Target 追踪

> 层级：03-engineering（横切工程）
> 状态：过渡追踪｜Milestone：v0.1.0｜对应 Issue：#743 伞 / #761（S2 盘点）
> **本文是唯一允许记录 Current 现状的文档**。设计文档（01-system / 02-modules）只写目标态，一切"现状缺陷 / 旧路径 / 死代码 / 迁移进度"集中在此追踪，避免设计内容与实现现状混淆。

## 1. Agent Runtime 现状缺口（S2 代码盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| R1 | **两套 loop** | `process_chat_loop`(~1539行,Main) + `SubAgentRun::run_loop`(~209行,Sub) 各自实现 | 单一 `loop_engine`，Main/Sub 零分支 | S3/S5 |
| R2 | **RuntimeContext 三层重叠**（#456）| `ChatRuntimeContext` + `RuntimeResources` + `ChatLoopContext` + `TuiLaunchContext` 字段大量重复复制 | 单一 `RuntimeContext`（12 端口 + config + event） | S5 |
| R3 | **无 Run 聚合** | 一次执行 = `ChatLoopContext` 临时值 + 局部 `ChatLoopFsm`，无 `RunId`、崩溃即丢 | 显式 `Run` 聚合 + `RunId` + 单状态机 | S3 |
| R4 | **Runtime 出站端口不完整** | 有：`TaskStorePort`/`ConfigReader`/`ChatEventSink`/`ProviderInfoPort`(只读)/`HookNotificationPort`(只通知) | 补 ContextPort、ToolCatalogPort、ToolExecutionPort、PolicyPort、MemoryPort、WorkspacePort、ReasoningPort + `AuditSink` + ProviderPort.invoke + HookPort.run | S5 |
| R5 | **Sub loop 无 stall/fuse 保护**（最大安全缺口）| Sub 无 StallDetector、无 ToolCallFuse，仅 3h timeout 兜底 | StuckGuard 内置 loop_engine，Main/Sub 统一 | S3 |
| R6 | **共享 `Arc<LlmClient>` 隐患** | Sub 改 `reasoning_level`/`max_tokens` 靠 finalize 手动恢复，**并发 sub 互相踩踏** | Sub 装配独立 client 副本 | S3/S5 |
| R7 | **Sub 绕过 policy/ask_user** | Sub tool 执行无 PolicyEngine gating，继承 `allow_all` | `DelegatedApproval`（受限转发）——**S2 只设计不实现**，当前保持 allow_all | 设计态（未定实现版本）|
| R8 | **事件无 agent_id** | 事件上下文仅 `chat_id/turn_id`，无 main/sub 区分 | event_projection 补 `agent_id` + 路由 | #612 / S3 |
| R9 | **RunSpec 配置散 4 处** | `AgentRoleConfig` + `AgentTool` 硬编码 system + 名称排除型 `ToolProfile::SubAgent` + `ModelEntryConfig`(effort) | 收敛为声明式 `RunSpec`，Tool 部分采用 Registry Scope + capability Profile | S3/S5 |
| R10 | **Session `messages`/`chats` 双轨** | 旧扁平 `messages` + 新链 `chats` 并存，加载迁移 | 只保留 `chats`，旧 `messages` 退役 | S5/S7 |
| R11 | **取消所有权跨 Session/Run 混淆** | SDK `CancelHandle` 捕获 Session 级 `Arc<Mutex<CancellationToken>>`，Main 每回合替换 token；另有 `ChatInputEvent::Cancel` 第二入口；旧 FSM 的 Cancel transition 仅测试使用 | `cancel_run(run_id)` 同步幂等入站命令 + `InterruptRequested → Cancelling → Cancelled`；每 Run 独占 scope | S3 (#700) |
| R12 | **取消传播不完整** | Provider/Tool 监听当前 token；compact 内建新 token，Hook 只有 timeout；父/子 Run 没有显式取消树 | Provider/Tool/Compact/Hook 共享或派生 Run scope；父取消传播到全部活动子 Run | S3/S5 (#700) |
| R13 | **TUI 两条取消路径** | Esc 在 update 内直接调用 handle；Ctrl+C 产生 Effect 后再调用；不符合 TEA 单向副作用流 | 统一 `InterruptCurrentRun → Effect::CancelCurrentRun → cancel_run(run_id)`；请求同步、终态 ACK 异步 | S3 (#700) |

## 2. Tool & Skill & Command 现状缺口（S2 代码盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| T1 | **Runtime 直持 Registry / Tool 实例** | Runtime 从具体 ToolRegistry 按名称取得 Tool 并调用；目录查询与函数执行无端口隔离 | `ToolCatalogPort` + `ToolExecutionPort`；只交换 Descriptor/Invocation/Outcome | S5 |
| T2 | **Profile 依赖 ToolName 黑名单** | `Full/SubAgent/NoAgent` 通过名称排除；NoAgent 与 SubAgent 非包含关系，新 Tool 容易意外扩权 | Registry Scope 表达装配；Profile 用 capability 允许集合，只收缩不扩权 | S3/S5 |
| T3 | **Scope 与授权混合** | 是否有 Agent/Task/AskUser/Worktree 同时由注册列表和 Profile 名称约定表达 | `Effective Tools = Registry Scope ∩ Profile Allowed Capabilities`；Main/Sub 分别装配 Scope | S3/S5 |
| T4 | **ToolExecutionContext 泄漏 Runtime 资源** | 执行上下文包含 registry、store、channel、semaphore 等具体活资源，构造点分散 | 最小 `ExecutionScope` + 对应 BC 的窄资源端口；禁止传 RuntimeContext/Registry/Store | S5 |
| T5 | **Tool 调用职责分散** | schema、timeout、并发、Policy/Hook/审批与实际调用跨 Runtime/Tool 实现散落 | Tool BC 强制存在性/Scope/Profile/schema/函数调用；Runtime 编排 Policy/Hook/审批/timeout/并发/取消/重试 | S3/S5 |
| T6 | **取消接口绑定实现细节** | Tool 执行依赖具体 cancellation/channel 形态，长进程/网络调用的协作停止不统一 | Tool PL 定义只读 `CancellationSignal`；Runtime 适配 cancellation tree 并拥有 timeout | S5 |
| T7 | **Tool 结果责任混合** | Tool 字符串结果、结构化 data、错误、截断/落盘和 UI 展示边界不统一 | `ToolOutcome` 保留领域结果；token budget/截断归 Context Management，持久化归 Storage，渲染归 TUI | S5 |
| T8 | **Skill 被包装成 Tool 且物化跨域** | SkillTool 只返回 loaded/path，实际内容由 prompt 路径物化；Skill 与 Tool 执行语义混合 | 独立 SkillCatalog/Materialization 端口，输出 PromptFragment 给 Context Management | S5 |
| T9 | **Slash Command 堆在 Runtime idle 流程** | 命令 parser/执行散在 idle_commands/input gate，查询、写命令与 prompt 注入混合 | Command Catalog/Router 按 PromptInjection、SnapshotQuery、ApplicationControl 路由至目标 BC | S3/S5 |
| T10 | **MCP 生命周期为隐式 Manager** | 连接状态由多个方法散点修改；health check、tool list diff/refresh 与 resource 路径未完整接线 | 显式 `McpConnection` 状态机；仅 Connected 发布 Catalog 投影，变化原子撤销/更新 | MCP Ready 后 |
| T11 | **MCP Tool Catalog 一致性不足** | disconnect 后目录撤销、动态上下线、annotations capability 映射及事件通知未形成统一契约 | MCP ACL 转 Tool PL；CatalogChanged 通知重新拉取 Snapshot；连接/投影一致 | MCP Ready 后 |
| T12 | **MCP 稳定身份与版本未定** | 动态工具尚未形成可验证的稳定 ID、schema 版本和 Catalog revision 协议 | MCP 正式接线时单独设计 ToolId、rename、版本与 in-flight 兼容；当前不预设 | MCP Ready 后 |

## 3. 死代码 / 退役清单

| 项 | 现状 | 处理 | 阶段 |
|---|---|---|---|
| **Scheduler** | `TaskScheduler` 全仓库仅内部 5 处引用，无生产实例化 | 判定死代码，删除 | S7 |
| 旧 `ChatLoopState` FSM | 仅 Main，Sub 无 FSM；Cancel/Abort transition 无生产调用 | 收敛进含 `Cancelling` 的 Run 单状态机后退役 | S3/S5 |
| Session 级可替换 cancel token 槽 | 为常驻 ChatStream 跨多个回合命中“当前 token”而引入，取消后需 reset，存在 stale/race 兜底 | 改为 per-Run scope + `cancel_run(run_id)` 后删除 | S3 (#700) |
| `ChatInputEvent::Cancel` | 与 SDK `CancelHandle` 并存，TUI 无生产调用 | 迁移期映射到唯一 cancel command，随后退役 | S3/S5 (#700) |
| 6 个 core 注入闭包 | `ChatLoopContext` 的 `save_chain`/`run_reflection`/`list_models` 等，为打破 business→core 反向依赖的临时注入 | 收敛后由对应 Port 替代 | S5 |
| 旧扁平 `Session.messages` | 迁移期双轨 | 退役 | S7 |
| ToolName 排除型 `ToolProfile` | 新 Tool 易意外扩权，`NoAgent` 与 `SubAgent` 语义正交 | 用 Registry Scope + capability Profile 替代后删除 | S5/S7 |
| `SkillTool` 伪执行入口 | 只报告 loaded/path，内容在 prompt 路径物化 | SkillMaterializationPort 接线后退役 | S5/S7 |
| Runtime `idle_commands` 命令聚合 | 三种 Slash 机制混在 Runtime idle 流程 | Command Router 接线后拆除旧生产入口 | S5/S7 |
| MCP 旧 wrapper / diff 孤立路径 | 多套 wrapper、diff/refresh/health check 未形成完整生命周期 | MCP Ready 后统一至 McpConnection + ACL；无消费者代码删除 | MCP Ready 后 |

## 4. 已正确隔离（可作参考范式）

| 项 | 现状 | 说明 |
|---|---|---|
| **Workspace 隔离** | `seed_isolated()`：继承 cwd/root，空栈+新锁，子 worktree 进出不影响父 | ✅ 子资源隔离范式 |
| **Task 隔离** | Sub 用全新 `TaskStore::new()` | ✅ |

## 5. 相关文档

- 领域模型（目标态）：[../02-modules/runtime/01-domain-model.md](../02-modules/runtime/01-domain-model.md)
- 模块边界：[../02-modules/runtime/02-module-boundaries.md](../02-modules/runtime/02-module-boundaries.md)
- 端口缺口：[../02-modules/runtime/06-ports-and-adapters.md](../02-modules/runtime/06-ports-and-adapters.md)
- Tool & Skill & Command 目标设计：[../02-modules/tools/README.md](../02-modules/tools/README.md)
- 横切工程总览：[README.md](README.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：S2 盘点的 Runtime 现状缺口(R1-R10)、死代码退役清单、已隔离参考范式 | #761 |
| 2026-07-12 | 补取消现状 R11-R13：Session token 槽、传播缺口、TUI 双路径及退役项 | #700 |
| 2026-07-12 | 新增 Tool/Skill/Command 缺口 T1-T12 与旧 Profile、SkillTool、idle_commands、MCP 路径退役项 | #787 |
