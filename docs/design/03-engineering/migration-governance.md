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
| R6 | **共享 `Arc<LlmClient>` 隐患** | Sub 改 `reasoning_level`/`max_tokens` 靠 finalize 手动恢复，**并发 sub 互相踩踏** | 共享不可变 Transport；Main/Sub 每次 attempt 使用独立 Invocation Scope | S3/S5 |
| R7 | **Sub 绕过 policy/ask_user** | Sub tool 执行无 PolicyEngine gating，继承 `allow_all` | `DelegatedApproval`（受限转发）——**S2 只设计不实现**，当前保持 allow_all | 设计态（未定实现版本）|
| R8 | **事件无 agent_id** | 事件上下文仅 `chat_id/turn_id`，无 main/sub 区分 | event_projection 补 `agent_id` + 路由 | #612 / S3 |
| R9 | **RunSpec 配置散 4 处** | `AgentRoleConfig` + `AgentTool` 硬编码 system + 名称排除型 `ToolProfile::SubAgent` + `ModelEntryConfig`(effort) | 收敛为声明式 `RunSpec`，Tool 部分采用 Registry Scope + capability Profile | S3/S5 |
| R10 | **Session `messages`/`chats` 双轨** | 旧扁平 `messages` + 新链 `chats` 并存，加载迁移 | 只保留 `chats`，旧 `messages` 退役 | S5/S7 |

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

## 3. Provider 现状缺口（S2 代码盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| P1 | **Runtime 依赖具体 client/pool** | Runtime 直接持有并调用 `LlmClient` / `LlmClientPool`，ProviderInfoPort 只覆盖元数据 | Runtime 只依赖自有 `ProviderPort` 与稳定 Invocation PL；具体 provider 仅在 Composition Root 装配 | S3/S5 |
| P2 | **调用期配置为共享可变状态** | model client 上以 `set_max_tokens` / `set_reasoning_level` 原地修改，再由 Sub finalize 手动恢复 | 共享不可变 Transport；每次 attempt 构造不可变 Invocation scope，无 `set_*` 与 restore | S3/S5 |
| P3 | **Main/Sub client 并发踩踏** | 多个 Sub 可拿到同一 `Arc<LlmClient>`，setup/read-modify/finalize restore 非原子，取消或 panic 会遗留污染 | Main/Sub 只共享只读 Transport，各自持有调用期状态；任一调用 drop 不影响其他调用 | S3/S5 |
| P4 | **流协议依赖多方法回调** | `StreamHandler` 通过 text/thinking/tool/error 等回调把 Provider 与 Runtime handler 生命周期耦合，错误主要为字符串 | pull-based `InvocationStream` + 封闭 `InvocationDelta` + 结构化终结错误；Runtime 自行组装 ModelInvocation | S5 |
| P5 | **wire DTO 发布面过宽** | Provider contract/api re-export 含供应商 request/stream payload、client config 和具体构造类型 | wire request/response/SSE DTO 全部留在 driver adapter；跨 BC 只交换 Invocation PL、ModelCapability 与 Message | S5/S7 |
| P6 | **跨调用重试下沉到 Provider** | 各 provider 内部自行 attempt/backoff，策略与日志不一致，Runtime 无法完整拥有 attempt 事件 | Provider 一次 invoke 只做一次上游语义请求并分类错误；Runtime 统一 retry/backoff/compact/final failure | S3/S5 |
| P7 | **stream → non-stream fallback 隐式重发** | 部分 driver 在流失败后于 Provider 内再次请求；已输出内容时存在重复或归因不清风险 | fallback 必须由 Runtime 作为新 attempt 显式编排；每次 attempt 独立事件、usage 与取消 | S5 |
| P8 | **reasoning 能力与 clamp 分散** | driver、provider、Runtime 与 model 配置分别处理上限/字段；Anthropic、OpenAI-compatible、Ollama 路径不统一 | Workflow desired ∩ Config user max ∩ Provider/model max；Provider 统一能力解析与 wire 映射 | S3/S5 |
| P9 | **错误分类不稳定** | HTTP、网络、stream、取消和 context 超限在多路径转换，部分上层依赖字符串判断 | `ProviderErrorKind + retryable + safe provider code`；Runtime 只按结构化语义编排 | S5 |
| P10 | **Usage 与成本边界未显式** | Provider 返回 usage，但流中累计语义、cache/reasoning token 与 Audit 成本归属未形成统一契约 | Provider 标准化 RawUsageSnapshot；Runtime 关联 attempt；Audit 独占 pricing、cost 与聚合 | S5 |
| P11 | **能力查询粒度不足** | reasoning 上限主要按 driver 固定返回，缺少 driver + model + 配置覆盖的完整解析 | 发布只读 ModelCapability，未知能力保守处理，并在编码前再次复核 | S3/S5 |
| P12 | **具体 Provider 构造点分散** | client/provider/pool 工厂与默认 fallback 可在 Provider/Runtime 路径内发生，缺少唯一装配边界 | Composition Root 独占 Transport、driver、凭证与 ProviderPort adapter 构造；缺失配置显式失败 | S5/S7 |

## 4. Memory 现状缺口（S2 代码盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| M1 | **无 MemoryPort trait** | Runtime 直调 `MemoryStore` 具体类型（`storage::api::MemoryStore`）| 抽 `MemoryPort` trait，实现移到 adapter；Runtime 不接触 MemoryStore | S5 |
| M2 | **领域逻辑与 I/O 混合** | `MemoryStore` 同时做 scoring/dedup/retrieval 和文件读写 | 拆分 MemoryService（领域）+ MemoryStorageAdapter（I/O）| S7 |
| M3 | **检索为子串匹配** | `entry_matches` 朴素小写 contains，无相关性排序 | Tier 1 BM25 关键词相关性排序 | #551 |
| M4 | **similarity_threshold 仅用于去重** | 检索不接入 threshold | 检索也用 threshold 过滤低相关结果 | #551 |
| M5 | **Reflection 代码在 Runtime** | `runtime/business/reflection/` 含 prompt/output/apply 领域逻辑 | 领域逻辑迁回 Memory BC，Runtime 只编排触发 + LLM 调用 | S5 |
| M6 | **无 ReflectionPromptPort** | Runtime 直接调 reflection 模块函数 | 抽 trait，Memory BC 暴露领域服务 | S5 |
| M7 | **memory_inject 硬编码参数** | `open_memory_store` 硬编码 `max_entries=100, threshold=0.8` | 从 ConfigSnapshot 读取 | S5 |
| M8 | **SessionReminder 在 Memory** | `share::memory::session_reminder` 是会话级数据 | 迁移到 Context Management（Session 聚合）| S5/S7 |
| M9 | **无 NoOpMemory** | Sub 无 Memory 隔离（可读写主记忆）| Sub 装配 NoOpMemory，不读不写不 reflection | S3/S5 |

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| P1 | **Runtime 依赖具体 client/pool** | Runtime 直接持有并调用 `LlmClient` / `LlmClientPool`，ProviderInfoPort 只覆盖元数据 | Runtime 只依赖自有 `ProviderPort` 与稳定 Invocation PL；具体 provider 仅在 Composition Root 装配 | S3/S5 |
| P2 | **调用期配置为共享可变状态** | model client 上以 `set_max_tokens` / `set_reasoning_level` 原地修改，再由 Sub finalize 手动恢复 | 共享不可变 Transport；每次 attempt 构造不可变 Invocation Scope，无 `set_*` 与 restore | S3/S5 |
| P3 | **Main/Sub client 并发踩踏** | 多个 Sub 可拿到同一 `Arc<LlmClient>`，setup/read-modify/finalize restore 非原子，取消或 panic 会遗留污染 | Main/Sub 只共享只读 Transport，各自持有调用期状态；任一调用 drop 不影响其他调用 | S3/S5 |
| P4 | **流协议依赖多方法回调** | `StreamHandler` 通过 text/thinking/tool/error 等回调把 Provider 与 Runtime handler 生命周期耦合，错误主要为字符串 | pull-based `InvocationStream` + 封闭 `InvocationDelta` + 结构化终结错误；Runtime 自行组装 ModelInvocation | S5 |
| P5 | **wire DTO 发布面过宽** | Provider contract/api re-export 含供应商 request/stream payload、client config 和具体构造类型 | wire request/response/SSE DTO 全部留在 driver adapter；跨 BC 只交换 Invocation PL、ModelCapability 与 Message | S5/S7 |
| P6 | **跨调用重试下沉到 Provider** | 各 provider 内部自行 attempt/backoff，策略与日志不一致，Runtime 无法完整拥有 attempt 事件 | Provider 一次 invoke 只做一次上游语义请求并分类错误；Runtime 统一 retry/backoff/compact/final failure | S3/S5 |
| P7 | **stream → non-stream fallback 隐式重发** | 部分 driver 在流失败后于 Provider 内再次请求；已输出内容时存在重复或归因不清风险 | fallback 必须由 Runtime 作为新 attempt 显式编排；每次 attempt 独立事件、usage 与取消 | S5 |
| P8 | **reasoning 能力与 clamp 分散** | driver、provider、Runtime 与 model 配置分别处理上限/字段；Anthropic、OpenAI-compatible、Ollama 路径不统一 | Workflow desired ∩ Config user max ∩ Provider/model max；Provider 统一能力解析与 wire 映射 | S3/S5 |
| P9 | **错误分类不稳定** | HTTP、网络、stream、取消和 context 超限在多路径转换，部分上层依赖字符串判断 | `ProviderErrorKind + retryable + safe provider code`；Runtime 只按结构化语义编排 | S5 |
| P10 | **Usage 与成本边界未显式** | Provider 返回 usage，但流中累计语义、cache/reasoning token 与 Audit 成本归属未形成统一契约 | Provider 标准化 RawUsageSnapshot；Runtime 关联 attempt；Audit 独占 pricing、cost 与聚合 | S5 |
| P11 | **能力查询粒度不足** | reasoning 上限主要按 driver 固定返回，缺少 driver + model + 配置覆盖的完整解析 | 发布只读 ModelCapability，未知能力保守处理，并在编码前再次复核 | S3/S5 |
| P12 | **具体 Provider 构造点分散** | client/provider/pool 工厂与默认 fallback 可在 Provider/Runtime 路径内发生，缺少唯一装配边界 | Composition Root 独占 Transport、driver、凭证与 ProviderPort adapter 构造；缺失配置显式失败 | S5/S7 |

## 4. Memory 现状缺口（S2 代码盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| M1 | **无 MemoryPort trait** | Runtime 直调 `MemoryStore` 具体类型（`storage::api::MemoryStore`）| 抽 `MemoryPort` trait，实现移到 adapter；Runtime 不接触 MemoryStore | S5 |
| M2 | **领域逻辑与 I/O 混合** | `MemoryStore` 同时做 scoring/dedup/retrieval 和文件读写 | 拆分 MemoryService（领域）+ MemoryStorageAdapter（I/O）| S7 |
| M3 | **检索为子串匹配** | `entry_matches` 朴素小写 contains，无相关性排序 | Tier 1 BM25 关键词相关性排序 | #551 |
| M4 | **similarity_threshold 仅用于去重** | 检索不接入 threshold | 检索也用 threshold 过滤低相关结果 | #551 |
| M5 | **Reflection 代码在 Runtime** | `runtime/business/reflection/` 含 prompt/output/apply 领域逻辑 | 领域逻辑迁回 Memory BC，Runtime 只编排触发 + LLM 调用 | S5 |
| M6 | **无 ReflectionPromptPort** | Runtime 直接调 reflection 模块函数 | 抽 trait，Memory BC 暴露领域服务 | S5 |
| M7 | **memory_inject 硬编码参数** | `open_memory_store` 硬编码 `max_entries=100, threshold=0.8` | 从 ConfigSnapshot 读取 | S5 |
| M8 | **SessionReminder 在 Memory** | `share::memory::session_reminder` 是会话级数据 | 迁移到 Context Management（Session 聚合）| S5/S7 |
| M9 | **无 NoOpMemory** | Sub 无 Memory 隔离（可读写主记忆）| Sub 装配 NoOpMemory，不读不写不 reflection | S3/S5 |

## 5. 死代码 / 退役清单

| 项 | 现状 | 处理 | 阶段 |
|---|---|---|---|
| **Scheduler** | `TaskScheduler` 全仓库仅内部 5 处引用，无生产实例化 | 判定死代码，删除 | S7 |
| 旧 `ChatLoopState` FSM | 仅 Main，Sub 无 FSM | 收敛进 Run 单状态机后退役 | S5 |
| 6 个 core 注入闭包 | `ChatLoopContext` 的 `save_chain`/`run_reflection`/`list_models` 等，为打破 business→core 反向依赖的临时注入 | 收敛后由对应 Port 替代 | S5 |
| 旧扁平 `Session.messages` | 迁移期双轨 | 退役 | S7 |
| ToolName 排除型 `ToolProfile` | 新 Tool 易意外扩权，`NoAgent` 与 `SubAgent` 语义正交 | 用 Registry Scope + capability Profile 替代后删除 | S5/S7 |
| `SkillTool` 伪执行入口 | 只报告 loaded/path，内容在 prompt 路径物化 | SkillMaterializationPort 接线后退役 | S5/S7 |
| Runtime `idle_commands` 命令聚合 | 三种 Slash 机制混在 Runtime idle 流程 | Command Router 接线后拆除旧生产入口 | S5/S7 |
| MCP 旧 wrapper / diff 孤立路径 | 多套 wrapper、diff/refresh/health check 未形成完整生命周期 | MCP Ready 后统一至 McpConnection + ACL；无消费者代码删除 | MCP Ready 后 |
| 共享 client 的 `set_*` / restore 路径 | 调用期配置以共享原子值/锁修改，Sub 完成时尝试恢复 | Invocation Scope 接线后删除 setter、previous/restore 字段及 finalize 恢复分支 | S5/S7 |
| `StreamHandler` 多方法回调 | Provider 主动调用 Runtime handler，错误和重试提示混入字符串回调 | InvocationStream 接线后删除 callback trait 与桥接 wrapper | S5/S7 |
| Provider wire DTO 公共 re-export | request/stream payload、client config 等由 contract/api 对外发布 | Runtime 迁至 Invocation PL 后收窄可见性并删除无消费者 re-export | S5/S7 |
| Provider 内部 retry / non-stream fallback | driver 内部执行跨调用重试与隐式第二次请求 | Runtime model_invocation 统一 attempt 编排后删除 | S5/S7 |
| `SessionReminders` 在 `share::memory` | 会话级提醒放在 Memory 共享内核，语义不属跨会话记忆 | 迁移到 Context Management 后从 `share::memory` 删除 | S5/S7 |
| `MemoryStore` 领域方法 | scoring/dedup/retrieval 混在 Storage crate 的 MemoryStore 中 | 拆分后领域方法迁到 MemoryService，MemoryStore 降为 Storage adapter | S7 |

## 6. 已正确隔离（可作参考范式）

| 项 | 现状 | 说明 |
|---|---|---|
| **Workspace 隔离** | `seed_isolated()`：继承 cwd/root，空栈+新锁，子 worktree 进出不影响父 | ✅ 子资源隔离范式 |
| **Task 隔离** | Sub 用全新 `TaskStore::new()` | ✅ |

## 7. 相关文档

- 领域模型（目标态）：[../02-modules/runtime/01-domain-model.md](../02-modules/runtime/01-domain-model.md)
- 模块边界：[../02-modules/runtime/02-module-boundaries.md](../02-modules/runtime/02-module-boundaries.md)
- 端口缺口：[../02-modules/runtime/06-ports-and-adapters.md](../02-modules/runtime/06-ports-and-adapters.md)
- Tool & Skill & Command 目标设计：[../02-modules/tools/README.md](../02-modules/tools/README.md)
- Provider 目标设计：[../02-modules/provider/README.md](../02-modules/provider/README.md)
- Memory 目标设计：[../02-modules/memory/README.md](../02-modules/memory/README.md)
- 横切工程总览：[README.md](README.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：S2 盘点的 Runtime 现状缺口(R1-R10)、死代码退役清单、已隔离参考范式 | #761 |
| 2026-07-12 | 新增 Tool/Skill/Command 缺口 T1-T12 与旧 Profile、SkillTool、idle_commands、MCP 路径退役项 | #787 |
| 2026-07-12 | 新增 Provider 缺口 P1-P12 与共享 client、回调流、wire DTO、隐式重试退役项 | #788 |
| 2026-07-12 | 新增 Memory 缺口 M1-M9 与 SessionReminders、MemoryStore 领域方法退役项 | #789 |
