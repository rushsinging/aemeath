# 迁移治理 · Current → Target 追踪

> 层级：03-engineering（横切工程）
> 状态：过渡追踪｜Milestone：v0.1.0｜对应 Issue：#743 伞 / #761（S2 盘点）
> **本文是唯一允许记录 Current 现状的文档**。设计文档（01-system / 02-modules）只写目标态，一切"现状缺陷 / 旧路径 / 死代码 / 迁移进度"集中在此追踪，避免设计内容与实现现状混淆。

## 1. Agent Runtime 现状缺口（S2 代码盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| R1 | **两套 loop** | `process_chat_loop`(~1539行,Main) + `SubAgentRun::run_loop`(~209行,Sub) 各自实现 | 单一 `loop_engine`，Main/Sub 零分支 | S3/S5 |
| R2 | **RuntimeContext 三层重叠**（#456）| `ChatRuntimeContext` + `RuntimeResources` + `ChatLoopContext` + `TuiLaunchContext` 字段大量重复复制 | 单一 `RuntimeContext`（出站端口 + config + event） | S5 |
| R3 | **无 Run 聚合** | 一次执行 = `ChatLoopContext` 临时值 + 局部 `ChatLoopFsm`，无 `RunId`、崩溃即丢 | 显式 `Run` 聚合 + `RunId` + 单状态机 | S3 |
| R4 | **Runtime 出站端口不完整** | 有：`TaskStorePort`/`ConfigReader`/`ChatEventSink`/`ProviderInfoPort`(只读)/`HookNotificationPort`(只通知) | 补 ContextPort、ToolCatalogPort、ToolExecutionPort、PolicyPort、MemoryPort、WorkspacePort、ReasoningPort + `UsageSink` + ProviderPort.invoke + HookPort.dispatch | S5 |
| R5 | **Sub loop 无 stall/fuse 保护**（最大安全缺口）| Sub 无 StallDetector、无 ToolCallFuse，仅 3h timeout 兜底 | StuckGuard 内置 loop_engine，Main/Sub 统一 | S3 |
| R6 | **共享 `Arc<LlmClient>` 隐患** | Sub 改 `reasoning_level`/`max_tokens` 靠 finalize 手动恢复，**并发 sub 互相踩踏** | 共享不可变 Transport；Main/Sub 每次 attempt 使用独立 Invocation Scope | S3/S5 |
| R7 | **Sub 绕过统一 PolicyPort** | Sub tool 执行直接继承 `allow_all` bool，无统一决策入口 | v0.1.0 Main/Sub 都调用 AllowAllPolicy；Future Deny/Approval 另行设计 | S3/S5 |
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

## 5. Storage 现状缺口（S2 摘要盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| S1 | **Storage 同时拥有业务模型** | Task/Batch 状态、依赖图、Memory 查询与 History 策略寄居 storage crate | Task/Memory/History 所属 BC 独占模型和不变量；Storage 只实现物理端口 | S5/S7 |
| S2 | **原子写机制未复用** | Session 自带 tmp/fsync/.bak；Memory、History、Tool Result 等路径直接 `fs::write` | 通用 AtomicBlob adapter；数据 BC 的窄持久化端口复用同一机制 | S5 |
| S3 | **backup/恢复协议不完整** | Session 有一代 `.bak`，但备份旋转失败被忽略；其他路径无 previous/quarantine | 原子可见、机械代际读取、领域验证后显式 promote/quarantine | S5 |
| S4 | **路径与任意物理 Path 耦合** | 多处业务代码拼接 `~/.agents` 路径或直接持有 PathBuf | StorageKey + SafePathSegment；物理根和路径解析只在 adapter | S5/S7 |
| S5 | **Tool Result 策略落入 Storage** | 50K 阈值、head/tail preview、inline reference 格式和写盘混在一个模块 | Config 提供阈值；Tool/Context Management 决定替换语义；Storage 只写 blob | S5 |
| S6 | **错误与损坏处理不统一** | String/Option/领域错误混用，部分失败静默跳过或仅日志 | StorageErrorKind + Generation ReadOutcome；数据 BC 验证并显式恢复 | S5 |
| S7 | **并发写与临时文件协议未统一** | 固定 `.tmp/.new`，跨实例互斥和残留清扫语义不一致 | 随机 create-new、跨进程锁、commit marker crash recovery | S5 |

## 6. Logging 现状缺口（S2 摘要盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| L1 | **Main/Sub 日志上下文互相覆盖** | request/model/provider/role/chat 保存在进程级 `CURRENT_*` | LogContextPatch + capture/instrument scope-local 传播 | S3/S5 |
| L2 | **sink 失败被静默吞掉** | write/flush/rotate/reopen 忽略 Result，sink 可永久失效 | Sink lifecycle + stderr emergency fallback + 限频恢复 | S5 |
| L3 | **TargetCatalog 多份真相** | 白名单、文件映射、sink 字段、flush 列表、guard 各自维护 | TargetSpec catalog 一次定义并共同消费 | S5/S7 |
| L4 | **Update target 未注册** | `aemeath:agent:update` 不在合法 catalog，落入兜底 | 注册 Application Version Control 诊断 target 与 sink | S5 |
| L5 | **Logging 与 Audit 混淆** | `agent-audit.log` 是普通诊断 sink | DiagnosticRecord 与 AuditSink 完全分离 | S5/S7 |
| L6 | **Config 参数接线不完整** | retention/logs_dir 未形成单一闭环 | ConfigSnapshot 注入 Filter/Sink/RotationPolicy | S5 |
| L7 | **schema/规范漂移** | 实现为 14 字段，部分注释仍称 13 | 14 字段 v1 契约 + consistency guard | S5/S7 |

## 7. Application Version Control 现状缺口（S2 摘要盘点）

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

## 8. Policy / Hook / Audit 现状缺口（S2 代码盘点）

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| PHA1 | **Policy 无统一端口** | 路径 helper、content warning、allow_all bool 分散；无 PolicyRequest/Decision | v0.1.0 建 PolicyPort + AllowAllPolicy；Deny/RequireApproval 只保留 PL | S3/S5 |
| PHA2 | **`--yolo` 泄漏为业务 bool** | Runtime/ToolContext 直接传播 allow_all | CLI/Config 映射 PolicyMode::AllowAll，Runtime 只依赖 PolicyPort | S5 |
| PHA3 | **安全 guard 冒充 Policy 风险** | path security、bash safety、content scan 各自调用 | 作为独立 Current guard 保留；未形成共同不变量前不并入 Policy Engine | S5/S7 |
| PHA4 | **Hook 公开面膨胀且结果分裂** | HookRunner 具体类型 + 多个 on_xxx / plain / JSON / blocking 入口 | 一个类型化 HookPort.dispatch + HookOutcome | S5 |
| PHA5 | **阻断协议不一致** | 部分路径未统一消费 result.blocked / continue=false / decision=block | exit 0/2/other + JSON directive 统一解析，主动 Block 与 ExecutionFailed 分离 | S5 |
| PHA6 | **非零 exit 语义冲突** | 配置注释称 exit 2 阻断；执行器把所有非零视为 blocked | exit 2=主动 Block；其他非零=ExecutionFailed | S5 |
| PHA7 | **Hook 失败无统一重试/回收** | spawn/timeout/wait 失败 fail-open；timeout 可能未 kill/wait 子进程 | 单 Hook 执行故障最多 3 次；timeout 必须回收旧进程 | S5 |
| PHA8 | **Stop Hook 上限伪造完成** | 连续阻断超过 5 次后强制 Done/Completed | Runtime 上限改 15；第 16 次 Failed(StopHookRetryExhausted) | S3/S5 |
| PHA9 | **Main/Sub Hook 行为不统一** | Stop/Hook 路径主要存在 Main loop，Sub 未复用 | 单 Loop Engine + 同一 HookPort；Main/Sub 同规则 | S3 |
| PHA10 | **Hook input/context mutation 未完整消费** | JSON schema 有 updatedInput/additionalContext，但调用链未统一应用 | HookOutcome 类型化 directive；调用方重新 schema/Policy 校验后应用 | S5 |
| PHA11 | **Audit crate 为空壳** | 只有 AuditApiMarker / empty gateway | MVP 建 UsageRecord、UsageSink、UsageQueryPort、worker | S5 |
| PHA12 | **Usage/Cost/Pricing 混在 Runtime** | CostTracker 同时记录 usage、计算 cost、读写全量 cost_history.json | Audit MVP 只迁 raw Usage；Cost/Pricing 保留 Future，不进入 MVP | S5/S7 |
| PHA13 | **Usage 缺统一关联 ID** | 记录主要含 session/model/tokens/cost，无 Run/Step/Invocation | 使用 SessionId + RunId + RunStepId + ModelInvocationId | S3/S5 |
| PHA14 | **Usage 写入阻塞且全量重写** | Runtime 直接 fs read/write JSON 数组 | 非阻塞 bounded UsageSink；worker 经 Storage AppendLogPort 写 JSONL | S5 |
| PHA15 | **Usage 与 Session 存储边界不清** | cost_history 为全局混合文件，缺独立 Audit 分区语义 | `~/.agents/audit/usage/{session_id}.jsonl`；Session 删除不级联 | S5 |
| PHA16 | **Audit/Logging 混淆风险** | Usage/Hook 信息依赖诊断日志展示，无事实查询端口 | Logging 只做诊断；UsageQueryPort 读取 Audit 事实，不解析日志 | S5 |

## 9. 死代码 / 退役清单

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
| Storage crate 内 Task/Memory 业务实现 | 物理持久化 crate 同时拥有 Task 状态机、依赖图与 Memory 查询行为 | 迁回对应 BC；Storage 仅保留 adapter 与通用机制 | S5/S7 |
| 业务代码散点直接文件写入 | Session/Memory/History/Tool Result 各自实现 IO 语义 | 窄数据端口接 Storage adapter 后删除重复路径 | S5/S7 |
| Logging 进程级 `CURRENT_*` | Main/Sub 并发共享可变上下文 | scope-local LogContext 接线后退役 setter | S3/S5/S7 |
| 普通诊断 `agent-audit.log` 路由 | 将 Audit 误当诊断 sink | AuditSink 接线后重新定义或删除 | S5/S7 |
| Update 单体 `UpdateService` / Gateway | 检查/缓存/下载/安装混成单对象 | ApplicationVersionPort + 内部 source/cache/installer adapters | S5/S7 |
| `AuditApiMarker` / `gateway::__empty` | Audit BC 仅物理占位，无领域契约 | UsageSink/Query 接线后删除占位类型 | S5/S7 |
| Runtime `CostTracker` / `pricing` / `CostSummary` | Usage、Cost、Pricing、持久化混合，且不符合 Usage-only MVP | 迁移 raw Usage 后退役；Cost/Pricing 作为 Future 另行设计 | S5/S7 |
| `cost_history.json` 全量写路径 | 每次保存重写数组，记录含派生 cost 且缺 Run IDs | 后续 importer 只迁可验证 raw token；旧路径有计划退役 | S5/S7 |
| Stop Hook 超限强制 Done | Stop 未放行却伪造 Completed | 改为第 16 次 RunFailed 后删除旧 helper | S3/S5 |

## 10. 已正确隔离（可作参考范式）

| 项 | 现状 | 说明 |
|---|---|---|
| **Workspace 隔离** | `seed_isolated()`：继承 cwd/root，空栈+新锁，子 worktree 进出不影响父 | ✅ 子资源隔离范式 |
| **Task 隔离** | Sub 用全新 `TaskStore::new()` | ✅ |

## 11. 相关文档

- 领域模型（目标态）：[../02-modules/runtime/01-domain-model.md](../02-modules/runtime/01-domain-model.md)
- 模块边界：[../02-modules/runtime/02-module-boundaries.md](../02-modules/runtime/02-module-boundaries.md)
- 端口缺口：[../02-modules/runtime/06-ports-and-adapters.md](../02-modules/runtime/06-ports-and-adapters.md)
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
| 2026-07-11 | 初稿：S2 盘点的 Runtime 现状缺口(R1-R10)、死代码退役清单、已隔离参考范式 | #761 |
| 2026-07-12 | 新增 Tool/Skill/Command 缺口 T1-T12 与旧 Profile、SkillTool、idle_commands、MCP 路径退役项 | #787 |
| 2026-07-12 | 新增 Provider 缺口 P1-P12 与共享 client、回调流、wire DTO、隐式重试退役项 | #788 |
| 2026-07-12 | 新增 Memory 缺口 M1-M9 与 SessionReminders、MemoryStore 领域方法退役项 | #789 |
| 2026-07-12 | 新增 Storage S1-S7、Logging L1-L7、Application Version Control V1-V8 缺口与退役项 | #793 |
| 2026-07-12 | 新增 Policy/Hook/Audit 缺口 PHA1-PHA16 与 Audit/Cost/Stop Hook 退役项 | #790 |
