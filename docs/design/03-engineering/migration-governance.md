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
| R4 | **12 端口只有 4 个有 trait** | 有：`TaskStorePort`/`ConfigReader`/`ChatEventSink`/`ProviderInfoPort`(只读)/`HookNotificationPort`(只通知) | 补 ContextPort/ToolPort/PolicyPort/MemoryPort/WorkspacePort/ReasoningPort + `AuditSink`(全新) + ProviderPort.invoke + HookPort.run | S5 |
| R5 | **Sub loop 无 stall/fuse 保护**（最大安全缺口）| Sub 无 StallDetector、无 ToolCallFuse，仅 3h timeout 兜底 | StuckGuard 内置 loop_engine，Main/Sub 统一 | S3 |
| R6 | **共享 `Arc<LlmClient>` 隐患** | Sub 改 `reasoning_level`/`max_tokens` 靠 finalize 手动恢复，**并发 sub 互相踩踏** | Sub 装配独立 client 副本 | S3/S5 |
| R7 | **Sub 绕过 policy/ask_user** | Sub tool 执行无 PolicyEngine gating，继承 `allow_all` | `DelegatedApproval`（受限转发）——**S2 只设计不实现**，当前保持 allow_all | 设计态（未定实现版本）|
| R8 | **事件无 agent_id** | 事件上下文仅 `chat_id/turn_id`，无 main/sub 区分 | event_projection 补 `agent_id` + 路由 | #612 / S3 |
| R9 | **RunSpec 配置散 4 处** | `AgentRoleConfig` + `AgentTool` 硬编码 system + `ToolProfile::SubAgent` + `ModelEntryConfig`(effort) | 收敛为声明式 `RunSpec` | S3/S5 |
| R10 | **Session `messages`/`chats` 双轨** | 旧扁平 `messages` + 新链 `chats` 并存，加载迁移 | 只保留 `chats`，旧 `messages` 退役 | S5/S7 |
| R11 | **取消所有权跨 Session/Run 混淆** | SDK `CancelHandle` 捕获 Session 级 `Arc<Mutex<CancellationToken>>`，Main 每回合替换 token；另有 `ChatInputEvent::Cancel` 第二入口；旧 FSM 的 Cancel transition 仅测试使用 | `cancel_run(run_id)` 同步幂等入站命令 + `InterruptRequested → Cancelling → Cancelled`；每 Run 独占 scope | S3 (#700) |
| R12 | **取消传播不完整** | Provider/Tool 监听当前 token；compact 内建新 token，Hook 只有 timeout；父/子 Run 没有显式取消树 | Provider/Tool/Compact/Hook 共享或派生 Run scope；父取消传播到全部活动子 Run | S3/S5 (#700) |
| R13 | **TUI 两条取消路径** | Esc 在 update 内直接调用 handle；Ctrl+C 产生 Effect 后再调用；不符合 TEA 单向副作用流 | 统一 `InterruptCurrentRun → Effect::CancelCurrentRun → cancel_run(run_id)`；请求同步、终态 ACK 异步 | S3 (#700) |

## 2. 死代码 / 退役清单

| 项 | 现状 | 处理 | 阶段 |
|---|---|---|---|
| **Scheduler** | `TaskScheduler` 全仓库仅内部 5 处引用，无生产实例化 | 判定死代码，删除 | S7 |
| 旧 `ChatLoopState` FSM | 仅 Main，Sub 无 FSM；Cancel/Abort transition 无生产调用 | 收敛进含 `Cancelling` 的 Run 单状态机后退役 | S3/S5 |
| Session 级可替换 cancel token 槽 | 为常驻 ChatStream 跨多个回合命中“当前 token”而引入，取消后需 reset，存在 stale/race 兜底 | 改为 per-Run scope + `cancel_run(run_id)` 后删除 | S3 (#700) |
| `ChatInputEvent::Cancel` | 与 SDK `CancelHandle` 并存，TUI 无生产调用 | 迁移期映射到唯一 cancel command，随后退役 | S3/S5 (#700) |
| 6 个 core 注入闭包 | `ChatLoopContext` 的 `save_chain`/`run_reflection`/`list_models` 等，为打破 business→core 反向依赖的临时注入 | 收敛后由对应 Port 替代 | S5 |
| 旧扁平 `Session.messages` | 迁移期双轨 | 退役 | S7 |

## 3. 已正确隔离（可作参考范式）

| 项 | 现状 | 说明 |
|---|---|---|
| **Workspace 隔离** | `seed_isolated()`：继承 cwd/root，空栈+新锁，子 worktree 进出不影响父 | ✅ 子资源隔离范式 |
| **Task 隔离** | Sub 用全新 `TaskStore::new()` | ✅ |

## 4. 相关文档

- 领域模型（目标态）：[../02-modules/runtime/01-domain-model.md](../02-modules/runtime/01-domain-model.md)
- 模块边界：[../02-modules/runtime/02-module-boundaries.md](../02-modules/runtime/02-module-boundaries.md)
- 端口缺口：[../02-modules/runtime/06-ports-and-adapters.md](../02-modules/runtime/06-ports-and-adapters.md)
- 横切工程总览：[README.md](README.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：S2 盘点的 Runtime 现状缺口(R1-R10)、死代码退役清单、已隔离参考范式 | #761 |
| 2026-07-12 | 补取消现状 R11-R13：Session token 槽、传播缺口、TUI 双路径及退役项 | #700 |
