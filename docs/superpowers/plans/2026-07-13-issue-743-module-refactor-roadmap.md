# Issue #743 全模块目标架构重构路线图

> 对应父 Issue：[#743](https://github.com/rushsinging/aemeath/issues/743)
>
> Context Engineering 父 Issue：[#547](https://github.com/rushsinging/aemeath/issues/547)
>
> 流程规则：[#840](https://github.com/rushsinging/aemeath/issues/840)
>
> 基线：`release/v0.1.0@d8159d40`
>
> 状态：规划完成，待创建和整理 GitHub 原生 sub-issues

## 1. 目标与治理规则

本路线图将已完成的全模块战术设计落实为可执行重构顺序。#743 是架构重构的父 Issue；#547 只管理 Context Engineering 的算法和质量任务。

大型模块使用 GitHub 原生 parent / sub-issue 层级，禁止新建或使用“伞 Issue”概念：

- 模块重构 Issue 必须位于 #743 的原生 sub-issue 树中；直接子项超过 7 个时，先按稳定能力边界建立中间父 Issue。
- 模块内原子交付任务是该模块 Issue 的原生 sub-issues。
- 叶子 sub-issue 对应一个独立 PR 和独立验收边界。
- 每层直接 sub-issues 原则上不超过 7 个。
- Runtime Port、SDK Event 和 Composition 接线只保留一份全局任务，禁止各模块重复创建。
- Guard + Verify、退役和大文件拆分放在最合适的公共父层级，避免每个模块重复建设。

## 2. 范围边界

### #743 负责

- 领域所有权和模块边界迁移。
- Port、Published Language、Adapter 与 Composition Root。
- Session/Task/Workspace 数据兼容和持久化安全。
- Runtime 单 Run 状态机和唯一 Loop Engine 的最终模块化。
- SDK/TUI 状态投影单一来源。
- 旧路径退役和架构守卫。

### #547 负责

- compact 阈值、触发公式和摘要质量。
- TokenBudget 估算精度。
- microcompact、snip、collapse 策略。
- Prompt cache 质量。
- Memory 检索、BM25、语义检索和评分质量。

判断标准：改变“最终给模型什么内容、何时 compact、选中哪些 Memory”归 #547；只改变所有权、依赖、持久化安全和装配方式归 #743。

## 3. 已确认的关键缺口

1. `--resume` 与 `/resume` 尚未形成 Session、Task、Workspace 的统一恢复事务；恢复后持久化身份可能漂移。
2. Session 保存可能重建 metadata，丢失 `created_at`、title、tags、notes、favorite。
3. 当前 Session `.tmp/.bak` 协议不满足 AtomicBlob 崩溃一致性。
4. Memory 注入仍按启动 cwd 直接构造 Store，可能忽略恢复后的 Project identity。
5. Runtime 仍直接持有 Provider、ToolRegistry、TaskStore、HookRunner、WorkspaceService 等具体实现。
6. Provider 共享 client 的 setter/restore 存在 Main/Sub 并发污染风险。
7. Task 仍允许闭包任意修改状态和依赖，DAG 检查与写入不原子。
8. Hook 结果仍被 Runtime 二次推断，进程 timeout/cancel 缺少完整 kill + wait 回收。
9. Sub Run 的取消、usage 和事件身份链路尚未完全闭合。
10. TUI spinner 仍独立维护执行生命周期，是 Runtime Run 状态机之外的第二状态源。
11. Logging 使用进程级 `CURRENT_*`，Main/Sub 并发会覆盖上下文。
12. Composition Root 目前只是门面，真实 adapter 构造仍散落在 Runtime。

## 4. #743 原生 Sub-issue 树

为满足每层直接 sub-issues 不超过 7 个，#743 采用三级结构。中间父 Issue 只管理稳定能力组，不直接承载代码 PR。

### 4.1 #743 直接 Sub-issues（7 项）

| 直接 sub-issue | 类型 | 内容 |
|---|---|---|
| #762 Context Management 架构迁移 | 模块父 Issue | 5 个原子 sub-issues |
| #649 Runtime 八模块迁移 | 模块父 Issue | 7 个原子 sub-issues |
| 数据与持久化边界收敛 | 中间父 Issue | Storage、Task、Project、Memory |
| 执行能力边界收敛 | 中间父 Issue | Provider、Tool/Skill/Command、Policy、Workflow、Hook、Audit |
| 通用能力与交付层收敛 | 中间父 Issue | Config、Logging、TUI、AVC、Composition/SDK 集成 |
| #763 全局 Guard + Verify | 叶子 Issue | 正式 Guard、故意违规验证、端到端验收 |
| #648 全局退役与大文件收尾 | 叶子 Issue | 旧路径、兼容层、死代码、warning 和大文件拆分 |

#762、#649 不再被称为伞；它们是 #743 的模块级父 sub-issues，并继续通过原生 sub-issues 分解。

### 4.2 数据与持久化边界收敛

直接 sub-issues：

| 模块级 sub-issue | 规模 | v0.1.0 优先级 |
|---|---:|---|
| Storage 纯机制与崩溃恢复 | 5 个子任务 | Must |
| Task Management 聚合与快照 | 7 个子任务 | Must |
| Project / Workspace 边界 | 3 个子任务 | Must |
| Memory / Reflection 所有权迁移 | 6 个子任务 | Must，检索质量归 #547 |

该中间父 Issue 负责数据权威、Schema 所有权和依赖顺序；Session 端到端 checkpoint/restore 的唯一集成任务仍归 #762，避免在四个模块重复创建。

### 4.3 执行能力边界收敛

直接 sub-issues：

| 模块级 sub-issue | 规模 | v0.1.0 优先级 |
|---|---:|---|
| Provider ACL 与不可变 Invocation | 7 个子任务 | Must |
| Tool / Skill / Command 边界 | 7 个子任务 | Must，MCP 生命周期 Future |
| Policy AllowAll 与边界归位 | 4 个叶子任务 | Must |
| Workflow / ReasoningPort | 约 3 个 PR | Must |
| Hook 执行协议与受管进程 | 5 个子任务 | Must |
| Audit Usage-only MVP | 6 个子任务 | Should，Cost Future |

Runtime 的 model/tool/context/interaction 消费方切换统一归 #649，不在各模块重复创建 Runtime 集成父任务。

### 4.4 通用能力与交付层收敛

直接 sub-issues：

| 模块级 sub-issue | 规模 | v0.1.0 优先级 |
|---|---:|---|
| Config 目标态收口 | 5 个子任务 | Must，复用 #683/#696 |
| Logging scope-local 与 sink 生命周期 | 7 个子任务 | Must |
| TUI 目标架构实施 | 7 个子任务 | Must，复用 #742/#612 |
| Composition Root 唯一生产装配 | 3 个子任务 | Must |
| Application Version Control 目标态 | 6 个子任务 | Future；完整安全链不阻断 v0.1.0 |

SDK/Event Projection 不另建模块父 Issue：身份和投影契约归 #649 的 event_projection；#612 负责 Main/Sub 实时事件；#674 只负责文件整理。Server 在 v0.1.0 不实施，只保留传输透明 Guard。

## 5. 各模块 Sub-issue 规划

### 5.1 #762 Context Management

1. 定型 ContextPort、SessionId、ContextMessage、错误与 revision 契约。
2. 迁移 Session 聚合，建立版本化 envelope、legacy reader 和 canonical writer。
3. 行为等价收口 Compact、Token Budget、Prompt/Guidance、Memory Injection。
4. Runtime 切换；实现 Task/Project prepare-commit restore、Session 身份原子切换和统一 resume。
5. 删除旧 Session 双轨、Runtime 直接访问与临时 re-export；增加模块 Guard。

AtomicBlob 是新 writer、自动迁移写回和 previous recovery 的硬前置。

### 5.2 #649 Runtime

1. Runtime-owned Ports、私有 RuntimeContext、agent_run/api 骨架。
2. event_projection 与 SDK Run/Step/ToolCall/Agent 身份契约。
3. model_invocation：Provider 调用、流汇聚、usage 与 retry 门禁。
4. context_coordination：只通过 ContextPort 使用历史、compact 和 memory。
5. tool_coordination：Tool、Policy、Hook、审批、并发、取消和结果回收。
6. interaction 与共享 Loop 最终切换，Main/Sub 只通过 RunSpec + RuntimeContext 区分。
7. 删除 RuntimeResources service locator、MainRunPort 业务逻辑和旧 looping 生产入口。

### 5.3 Storage

1. StorageKey、SafePath、AtomicBlobPort 和文件 adapter。
2. Generation、primary/previous、promote、quarantine。
3. crash journal、锁、durability 和 fault injection。
4. Task/Memory 等业务模型迁出，内部路径切换到纯机制。
5. AppendLog、Tool Result blob、History 瘦身和边界 Guard。

Storage 只处理 bytes、key 和物理一致性，不解析领域 schema。

### 5.4 Provider

1. Runtime-owned ProviderPort 与 Provider Published Language。
2. 不可变 Invocation Scope 和共享 transport。
3. pull-based Invocation Stream 与取消/backpressure。
4. Anthropic、OpenAI-compatible、Ollama 私有 driver ACL。
5. ProviderError 分类；Runtime 拥有 retry、compact 和 fallback。
6. RawUsageSnapshot、capability resolution 和 reasoning 最终 clamp。
7. 生产切换，删除 callback、setter/restore 和散点构造；增加 Guard。

### 5.5 Tool / Skill / Command

1. Tool PL、ToolCatalogPort、ToolExecutionPort。
2. Registry Scope 与 capability Profile，权限只能收缩。
3. 跨域工具依赖端口化，ExecutionScope 最小化。
4. Runtime Tool Coordination 切换。
5. SkillCatalog/Materialization 输出 PromptFragment，由 Context 决定注入。
6. Command Catalog/Router 按 PromptInjection、SnapshotQuery、ApplicationControl 路由。
7. 删除名称黑名单、SkillTool、ToolResources 和 Runtime 直接 Registry 访问。

### 5.6 Memory / Reflection

1. MemoryPort、MemoryPersistence 与 Tier 0 行为基线。
2. MemoryService 和 Storage adapter 分离。
3. Context 注入与 Sub Run NoOpMemory。
4. Reflection prompt/schema/parse/apply 迁回 Memory。
5. Runtime 只保留触发、Provider 调用、取消和并发编排。
6. 删除 `storage::api::MemoryStore` 业务 facade 与旧直接构造。

### 5.7 Task Management

1. Task 聚合、严格 `Pending → InProgress → Completed` 状态机和 PL。
2. DAG 原子操作、反向边和 Batch 生命周期。
3. TaskPort、应用服务和统一事务状态。
4. 版本化 TaskSnapshot、validate-then-swap restore。
5. Runtime/Tool 消费方切换。
6. 与 #762 的 Session snapshot/restore 集成。
7. 删除任意 update 闭包、旧 TaskStore 与 Storage 业务所有权。

### 5.8 Hook

1. HookPort、HookOutcome、FailureKind 和 HookPoint 能力矩阵。
2. 受管进程组、完整 timeout/cancel、kill + wait 和输出上限。
3. Dispatcher、协议解析和仅故障重试。
4. Runtime Run 状态接线与类型化事件投影。
5. 删除 HookRunner 多入口、HookUi 二次推断和旧 DTO。

协议固定：任意非零 exit 是主动 Block，不因 exit code 重试；仅 spawn、wait、IO、timeout、非法 JSON 等 ExecutionFailed 重试。

### 5.9 Audit

1. UsageRecord、UsageSink、UsageQueryPort 和统一关联 ID。
2. Storage AppendLogPort。
3. bounded worker、非阻塞 try_record、shutdown drain 和指标。
4. 查询、分页、损坏行 warning 和 token summary。
5. Provider/Runtime/Composition 接线。
6. CostTracker/cost_history 旧链退役和 Guard。

### 5.10 Logging

1. TargetCatalog 单一真相。
2. scope-local LogContext 与 inherit/set/clear。
3. ConfigSnapshot → LoggingSettings。
4. FileSinkLifecycle、rotation、retention、故障降级和恢复。
5. Main/Sub/Provider scope 接线。
6. TUI/Composition 和全仓 target 迁移。
7. Audit 隔离、`CURRENT_*` 和固定 sink 结构退役。

### 5.11 TUI

1. SDK Run/Event 身份契约，关联 #612。
2. AgentEventMapper ACL 收口，禁止 SDK DTO 进入 Model。
3. TEA 核心及 Model/ViewState 分离。
4. 复用 #742：RunProjection 纯派生 spinner，删除第二状态源。
5. 唯一 Cancel Effect 调用 `cancel_run(run_id)`，终态由 Runtime 确认。
6. ViewAssembler/ViewModel 与 Main/Sub 嵌套展示。
7. 删除旧 UiEvent、mapper、spinner 写入口和 Model→Render 旁路；增加 Guard。

### 5.12 Config

1. 复用 #683：默认值单一真相。
2. 复用 #696：消费方与子结构 accessor 审计。
3. Composition 构造单一 ConfigAppService/ConfigReader。
4. File/Cli/Claude adapter 与 ACL 完整实现。
5. Env 合流、散点 `ConfigAppService::new` 和旧直读路径退役。

### 5.13 Project / Policy / Workflow / Composition

Project 三条执行线：BC 内部收敛；Runtime/Tool 消费方切换；Context snapshot/git context 集成。

Policy 四条叶子任务：Context warning 归位；Tool safety guard 归位；PolicyPort + AllowAllPolicy；Runtime 删除 `allow_all` 传播。`--yolo` 为主名称，`--allow-all` 为兼容 alias，均只映射为 `PolicyMode::AllowAll`，不再绕过 path/Bash/read-before-write guard。

Workflow：对齐 effort 节点；建立 ReasoningPort；统一 Config user max 与 Provider capability clamp。

Composition 三条全局任务：FeatureGateways 注入生效；单一 ConfigReader 注入；全部 concrete adapter 构造上移并启用 composition-only Guard。

## 6. 实施波次

### Wave 0：治理与契约冻结

- 建立 #743 的三级原生 sub-issue 树。
- 冻结 PL/Port 所有权和 Runtime/Event 身份。
- 创建统一 Composition 接线任务。

### Wave 1：基础机制

并行推进 Storage AtomicBlob、Config、Logging、Runtime Ports、Provider/Tool/Task/Memory/Hook PL。

### Wave 2：数据域

并行推进 Task Snapshot、Project restore、MemoryService、Session schema；随后由 #762 完成统一 checkpoint、prepare/commit restore、Session identity 切换和恢复后 Memory 注入。

### Wave 3：执行边界

并行推进 Provider Invocation、Tool 双端口、Policy AllowAll、Workflow Reasoning、Hook process；随后接入 Runtime 的 model/context/tool/interaction 模块。

### Wave 4：Usage、事件和交付层

推进 Audit Usage、event_projection、#612 Main/Sub 路由、TUI ACL/RunProjection/spinner/cancel/ViewModel。

### Wave 5：Composition 切换

依次接入 Config+Logging、Context+Storage、Provider+Tool、Policy+Workflow、Memory+Task+Project、Hook+Audit；AVC 不阻断 v0.1.0 主链。

### Wave 6：全局收尾

- #763：正式 Guard、故意违规验证和端到端验收。
- #648：兼容层、旧路径、Scheduler、旧 FSM、setter/restore、旧 Store 和 warning 清理。

## 7. v0.1.0 Release Gate

### Must Have

- Session/Task/Workspace 恢复正确性和 AtomicBlob 最小生产能力。
- Provider 不可变 Invocation Scope。
- Tool 双端口与 Scope/Profile。
- Policy AllowAll 和 Hook 最新执行协议。
- Runtime 八模块边界与唯一 Run/Loop。
- SDK/TUI Run 状态单一投影。
- Config 单一读取链和 Logging 并发上下文正确性。
- 全局 Guard、退役和 workspace test/clippy 通过。

### Should Have

- Audit Usage-only MVP。
- Reflection 后台并发语义完整收口。
- TUI Main/Sub 完整嵌套展示。

### Future / Out of Scope

- Server、WS、控制面和 worker launcher。
- MCP 完整动态生命周期。
- Audit Cost/Pricing 与原文审计。
- Context collapse、embedding retrieval。
- AVC 完整 signed manifest + 全平台安全安装事务；在可信链未完成的平台只保留检查/手动更新。
- Deny/RequireApproval Policy 引擎。

## 8. 全局验收

- Runtime 核心不直接依赖具体 Provider、ToolRegistry、TaskStore、MemoryStore、WorkspaceService、HookRunner。
- Composition Root 是 concrete adapter 唯一生产构造点。
- Storage 不拥有领域模型，领域层不直接访问文件系统。
- Main/Sub 共享同一 Run 与 Loop Engine，但使用独立 cancellation、Invocation Scope 和 LogContext。
- Session 恢复实现全有或全无的身份、历史、Task、Workspace 切换。
- TUI 不维护第二套执行状态，取消和终态以 Runtime 为唯一事实。
- 每个跨层链路在 Runtime、SDK、ACL、Model/Adapter 各层都有测试。
- `cargo test --workspace` 通过。
- `cargo clippy --workspace --all-targets` 无 warning。
- #763 的每条 Guard 均通过故意违规证明可拦截。
