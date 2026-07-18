# 架构守卫与白名单

> 状态：**已落地** · 维护人：架构组
> 对应实现：`.agents/aemeath.json` + `.agents/hooks/check-*.sh` + `.agents/hooks/no_mod_rs.sh`
>
> 守卫脚本本身是**可执行的运行时真相**——真正的行为、常量与白名单以脚本代码为准。本文档是配套的**人类可读索引**，梳理已启用守卫的脚本行为、常量与白名单，便于查阅、评审与 PR 描述引用；它不覆盖脚本、也不是脚本之外的第二真相源。任何守卫脚本行为、常量或白名单的变更，**MUST** 同步更新本文档对应小节；本文档与脚本不一致时，**以脚本的可执行语义为准**，并在本文档 PR 中说明差异原因。Current → Target 差距、责任、进度与退出条件以 [Migration Governance](03-migration-governance.md) 为唯一治理真相。

## 概述

架构守卫是仓库的"机械式宪法"——把 [依赖铁律](../01-system/05-dependency-rules.md)、[能力优先代码组织](../01-system/06-code-organization.md)、薄入口和单一真相等规则固化为可执行的静态检查。已启用但只反映迁移期实现的守卫 **MUST** 在本文单独标记，**NEVER** 冒充 Target 原则。所有守卫通过 `.agents/aemeath.json` 的 `Stop` 钩子触发，串联执行，**任一失败即阻断会话**。

```
┌─────────────────────────────────────────────────────────────┐
│ PreToolUse（Edit/Write）                                    │
│   └─ reject-main-edit.sh    拦截在主工作区直接改代码；对不   │
│                             存在父目录向上解析最近祖先，区分 │
│                             主工作区 / worktree / git 上下文 │
│                             不可解析三类诊断                 │
│                                                              │
│ Stop（任务结束）                                              │
│   └─ check-architecture-guards.sh    串行执行 35 个守卫       │
│   └─ check-unit-tests.sh            cargo test --lib         │
└─────────────────────────────────────────────────────────────┘
```

`check-architecture-guards.sh` 本身**不是**守卫，它只做编排（34 个独立脚本 + 1 个内联 `run_tui_single_source_structure_guard`，合计 35 个守卫）。下表才是真正的守卫集合；历史序号未逐次重排，实际调用顺序以 `check-architecture-guards.sh` 源码为准。

## 守卫索引

| # | 守卫脚本 | 类别 | 守护不变量 |
|---|---|---|---|
| 0 | `check-guard-registry.sh` | Guard 治理 | 校验机器注册表、stable id、分类、迁移债务预算、stale scope 与 Shell 隐式排除引用 |
| 1 | `check-cargo-dependency-graph.sh` | DDD 边界 | Cargo workspace 依赖方向白名单 |
| 2 | `check-cli-thin-entry.sh` | DDD 边界 | CLI 仅 `composition + sdk`，禁止穿入 runtime |
| 3 | `check-share-no-upstream-deps.sh` | DDD 边界 | share 不依赖任何业务 feature |
| 4 | `check-share-minimal-kernel.sh` | DDD 边界 | share kernel 禁行为/IO/并发/时钟 + 依赖白名单 |
| 4a | `check-composition-layout.sh` | Composition Root | Composition 只使用扁平 capability-first wiring modules，禁止 Hexagonal/COLA 层与未登记顶层源码 |
| 5 | `check-cola-layer-purity.sh` | 迁移期固定层级与 Tools scope/profile 边界 | 未迁移 Feature 继续受 COLA 依赖方向约束；已迁移 Feature 锁定各自目标目录；Tools 额外锁定 capability-only 授权、`ToolProfile` shrink-only API 与 registry/domain/façade 边界 |
| 6 | `check-crate-api-boundary.sh` | Feature 边界 | 未迁移 feature 经 `::<crate>::api`；Runtime、Context、Storage 仅开放登记的 crate-root 窄 façade |
| 6t | `check-task-persistence-capability.sh` | Task 能力隔离 | Runtime/Tools 仅可消费 `TaskAccess`；Task persistence/restore authority 仅限 Context/Composition |
| 6a | `check-provider-invocation-scope.sh` | Provider 调用隔离 | Provider 禁调用期 atomics/setter，Runtime 禁 shared-client lock/restore；`invocation_stream` 必须显式接收不可变 Invocation Scope |
| 6b | `check-provider-pull-stream.sh` | Provider 流边界 | 生产路径禁止恢复 `CallbackHandler` / `StreamHandler` / `RuntimeStreamHandler` / `stream_message_raw` / callback `stream_message`；Runtime 与 Context 只能主动 poll `InvocationStream` |
| 6c | `check-provider-http-attempt.sh` | Provider 调用隔离 | 单 attempt 机械 send/cancel/status 只能经 crate-private `HttpAttemptExecutor`；HTTP/network 诊断日志 API（`log_network_error`/`log_http_error`/`ErrorLogContext`/`LlmApiErrorRecord`）仅限 `http_attempt.rs` + `error_log.rs` 调用 |
| 6d | `check-provider-retry-ownership.sh` | Provider 策略所有权 | Provider 生产 stream adapter 禁止恢复 retry loop、backoff sleep、`FallbackPlanned` 或 stream→non-stream fallback；跨 attempt 策略只属于 Runtime |
| 6e | `check-provider-usage-capability.sh` | Provider PL 语义 | pull-stream usage 禁止把未报告字段默认成零；OpenAI-compatible reasoning maximum 与 legacy clamp 必须从唯一 `ReasoningCapability` 派生 |
| 6f | `check-provider-driver-acl.sh` | Provider Driver ACL | driver 解析、协议族/API style 选择与实现配置必须留在 Provider；Runtime/Composition/CLI 禁止解析 driver 或引用内部配置 |
| 7 | `check-context-architecture.sh` | 业务约束 | agent context 所有权 CTX-R1–CTX-R6 |
| 8 | `check-forbidden-imports.sh` | 业务约束 | `share::adapter` 仅 composition 可引用 |
| 9 | `check-tui-tea-purity.sh` | TUI 架构 | update 纯函数、副作用走 Effect |
| 10 | `check-tui-toplevel-layout.sh` | TUI 架构 | 顶层模块白名单 + feature #57 旧路径守卫 |
| 11 | `check-tui-effect-boundary.sh` | TUI 架构 | model/update 不直接执行 Effect |
| 12 | `check-tui-model-view-boundaries.sh` | TUI 架构 | model/render/view 边界 + 物理遗留 |
| 13 | `check-tui-output-legacy-guards.sh` | TUI 遗留 | TUI M2 后选区/工具状态旁路守卫 |
| 14 | `check-tui-block-nesting.sh` | TUI 组件 | gutter 仅由 document_renderer 注入 |
| 15a | `check-render-pure.sh` | TUI 渲染 | render 禁止直读 conversation/runtime domain model，测试与登记 display bridge 除外 |
| 15 | `check-render-isolation.sh` | TUI 渲染 | render/output 纯函数边界 |
| 16 | `check-unsafe-text-ops.sh` | 安全/IO | 禁非 char 边界 str 切片 |
| 17 | `check-log-target-prefix.sh` | 日志架构 | log target 字符串字面量必须以 `aemeath:` 开头 |
| 17a | `check-logging-scope-context.sh` | 日志架构 | 禁止在 legacy 精确基线外新增进程级执行上下文状态；新路径必须使用 `LogContext` task-local scope |
| 17b | `check-logging-settings-injection.sh` | 日志架构 | Logging 禁止读取 env；Runtime 禁止装配或初始化 Logging；`UnifiedLogger::init` 只能由 Composition 单一入口调用 |
| 18 | `no_mod_rs.sh` | 文件约定 | 禁止 `mod.rs` |
| 19 | `check-config-env-guard.sh` | 配置架构 | 禁止 config 包外读业务 env（`AEMEATH_*`、`*_API_KEY`、`LLM_*`） |
| 20 | `run_tui_single_source_structure_guard`（内联） | TUI 结构 | feature #70 结构化单一真相规则 |
| 21 | `check-agent-client-trait-minimal.sh` | SDK 边界 | `AgentClient` trait 仅 `chat()` + 同步 `cancel_run(run_id)`；禁止恢复 `ChatInputEvent::Cancel` |
| 22 | `check-shared-run-loop.sh` | Runtime 架构 | Main/Sub 只调用唯一共享 Loop Engine；禁止旧 FSM、Session token 槽与 `max_turns` |
| 23 | `check-run-control-boundary.sh` | SDK 边界 | SDK run control Published Language（`packages/sdk/src/run.rs`）只能是纯值 DTO；`packages/sdk/src/client.rs` 禁止在 #878 atomic cutover 前提前出现 `cancel_run_step` / `terminate_run` |
| 24 | `check-config-reader-injection.sh` | 配置架构 | ConfigAppService 仅由 Config/Composition 构造；Runtime/TUI/CLI 禁止散点构造或持 Config 契约 |
| 24a | `check-config-workflow-boundary.sh` | 配置架构 | Config 生产代码禁止重新拥有 Workflow Reasoning Graph 配置语义；仅兼容测试可引用退役字段 |
| 25 | `check-production-reachability.sh` | 测试治理 | Rust xtask 拦截生产 test-only API、未保护 testing/fixture/fake 模块与新增 `allow(dead_code)`；可输出 deterministic public surface |

另有 `check-architecture-guards.sh` 内联 `run_tui_single_source_structure_guard` 守卫（#70 TUI 单一真相 + InputModel 写入约束），见 §20。

## 0. check-guard-registry.sh

- **功能**：调用 `cargo run -p xtask -- guard-registry check`，以 `.agents/architecture-guard-registry.json` 为单一机器可读治理注册表。
- **分类**：`target_capability_policy`、`target_hexagonal_policy`、`scope_exclusion`、`false_positive_suppression`、`migration_exception`；只有最后一类计入迁移债务。
- **schema**：每项使用全局唯一 stable id，并记录 guard、module、scope、owner、reason、tracking issue、introduced baseline、exit condition 与 status。迁移例外缺失归责或退出信息时 fail-closed。
- **预算**：Current 冻结迁移债务为 repository `7`，其中 Runtime `5`、Storage `1`、TUI `1`；模块和仓库预算均只允许下降。
- **stale / 隐式排除**：精确 path/path-prefix 不存在即 stale；每个注册项必须被其声明的 Guard 以精确 `guard-registry:<stable-id>` 引用；Shell 中 `grep -v`、`--exclude`、`--exclude-dir`、`EXEMPT_FILES`、migration exception 集合和自由格式 inline allow 必须在同一行或前一行引用同 Guard 下已登记 stable id。
- **expiry**：每次执行通过 GitHub CLI 核验所有 migration exception 的 tracking Issue 仍为 OPEN；查询失败或 Issue 已关闭均 fail-closed。
- **报告**：`cargo run -p xtask -- guard-registry report . <output>` 按 stable id 确定性输出 classification、module、guard、scope kind 与 lifecycle 维度，用于模块开发前/完成后预算复核。
- **Current 基线复核**：Storage 的 Target policy 仍不计债务，但 `STORAGE_TRANSITIONAL_MODULES` 是 #883 承接的真实迁移债务 1；Composition 仅有合法唯一装配 policy；Workflow、Audit、Project 未发现 migration exception，与人工基线一致。
- **边界**：本守卫只治理例外和 policy 元数据，NEVER 替代 #1022 的 capability-first 正式边界，也不退役 legacy COLA Guard。
- **故意违规证据**：缺 owner、重复 id、stale path、超预算、未登记 `grep -v` 均被定向元守卫阻断；恢复后元守卫及总编排 clean pass。

## 1. check-cargo-dependency-graph.sh

- **功能**：基于 `cargo metadata` 校验各 crate 的业务依赖是否落在显式白名单内。
- **守护**：[05-dependency-rules.md](../01-system/05-dependency-rules.md) §2 R3 / R4 / R6——固化当前 feature 依赖白名单、薄外部驱动与唯一生产装配入口。默认拒绝未声明的业务依赖，防双向/横向乱依赖。
- **白名单（`business_allow`）**：

| Crate | 允许依赖（workspace crate） |
|---|---|
| `cli` | `composition`, `sdk` |
| `composition` | 全部 FEATURE_CRATES + `share` + `sdk` + `logging` |
| `runtime` | `project`, `policy`, `context`, `provider`, `tools`, `storage`, `task`, `hook`, `audit`, `workflow`, `share`, `sdk`, `logging` |
| `share` | `logging`, `utils` |
| `project` | `share` |
| `policy` | `share` |
| `context` | `share`, `provider`, `storage`, `task`, `sdk` |
| `memory` | `storage`, `utils` |
| `provider` | `share` |
| `tools` | `share`, `project`, `storage`, `task` |
| `storage` | `share` |
| `task` | ∅ |
| `hook` | `share` |
| `audit` | `share`, `sdk`, `storage` |
| `workflow` | `share` |
| `update` | `share`, `sdk`, `logging` |
| `sdk` | `share`, `utils` |
| `logging` | ∅ |
| `utils` | ∅ |

> **Memory BC 当前物理落点**：#895 已建立独立 `memory` crate 的 owner-owned PL/`MemoryPort`；#896 新增 Memory-owned `MemoryDatasetStore`、AtomicDataset integration adapter 与 `utils` key hash。`memory → storage` 只允许 adapter 消费 Storage crate-root OHS，domain/ports/service 的层间方向由 `check-cola-layer-purity.sh` 守卫；旧业务实现和生产消费者仍由 #883/#897/#900 迁移退役。
>
> **Workflow BC 当前物理落点**：Workflow（Reasoning Graph）已位于独立 `agent/features/workflow` crate。Runtime 仅依赖 Workflow crate-root 窄 façade；Workflow 只依赖 Shared Kernel，不依赖 Runtime 或 Provider。

- **例外 / 已批准跨 BC 依赖**：
  - `runtime/tools → task`：消费 Task-owned `TaskAccess` OHS 与 Published Language；`task` 反向依赖消费者仍被拒绝。
  - `context → task`：Context Session adapter 消费 Task-owned `TaskPersist` 与 snapshot Published Language；Runtime/Tools 的 persistence/restore authority 由 `check-task-persistence-capability.sh` 机械拒绝。
  - `tools → {project, storage}`：Current 横向依赖登记；按 [05-dependency-rules.md](../01-system/05-dependency-rules.md) §2 R3 只能经各自窄 façade 接入。脚本中的 `api` 名称是迁移期物理事实，不是 Target 通用目录规范。
  - `composition →` 全部 feature：唯一装配根。
- **失败模式**：违反时输出 `{"decision":"block", "reason": "Cargo workspace dependency graph violates strict DDD boundaries: ..."}` 并以 exit code 2 退出。

## 2. check-cli-thin-entry.sh

- **功能**：检查 `apps/cli` 只直接依赖 `composition + sdk + 纯技术库`。
- **守护**：[05-dependency-rules.md](../01-system/05-dependency-rules.md) §2 R4 / R6——CLI 不得直连 Runtime 内部或 supporting capability，业务能力经 Composition 装配与 `AgentClient` 契约接入。
- **白名单**：
  - `ALLOWED_CLI_WORKSPACE_DEPS = {composition, sdk}`
  - `FORBIDDEN_DOMAIN_CRATES = {runtime, project, policy, context, provider, tools, storage, hook, audit, share, update}`
  - `BOOTSTRAP_DETAIL` 正则：拦截 `AgentClientImpl` / `from_args` / `wire_runtime` / `runtime::(api::)?(gateway|core|business|utils|contract|AgentClientImpl)` 等实现细节。
- **例外**：无。
- **检查范围**：
  - `apps/cli/Cargo.toml` 不能声明对 FORBIDDEN_DOMAIN_CRATES 的 path 依赖；
  - 必须在 `apps/cli/src/**/*.rs` 中检查 `use` 语句；
  - 经 `cargo metadata` 二次确认工作区依赖闭包。

## 3. check-share-no-upstream-deps.sh

- **功能**：检查 `agent/shared/Cargo.toml` 不依赖任何业务 feature。
- **守护**：[05-dependency-rules.md](../01-system/05-dependency-rules.md) §2 R3——shared kernel 只能发布经证明的共享语言，禁止反依赖业务 capability。
- **被禁上游 crate 列表**：`runtime, project, policy, context, provider, tools, storage, hook, audit, composition, cli, sdk`。
- **例外**：无。
- **检查方式**：单文件清单匹配 `[dependencies]` 段；命中即失败。

## 4. check-share-minimal-kernel.sh

- **功能**：扫描 `agent/shared/src/`，禁止 kernel 出现行为/IO/并发/时钟/状态容器；并把 `agent/shared/Cargo.toml` 依赖限定在白名单内。
- **守护**：[05-dependency-rules.md](../01-system/05-dependency-rules.md) §2 R1 / R3——kernel 只承载稳定共享语言与纯函数，禁止吸收行为、I/O 和业务状态。
- **禁用模式（`forbidden_patterns`）**：

| 模式 | 理由 |
|---|---|
| `\bToolRegistry\b` | 属于 `tools` crate-root façade |
| `\bTaskStore\b` / `\bTaskStoreStats\b` | 属于 Storage crate-root façade |
| `\bstd::fs::` / `\btokio::fs::` / `\bFile::` / `read_to_string` / `write(` / `create_dir` | share 不得做 fs IO |
| `\bstd::process::` / `\btokio::process::` / `Command::new` | share 不得 spawn process |
| `\breqwest::` / `\bhyper::` / `\bureq::` / `\bhttp::` | share 不得做网络/http IO |
| `\bparking_lot::` / `\bRwLock\b` | 状态容器不属于 share |
| `#[\s*async_trait\s*]` | async trait 行为属于 feature |
| `\btrait\s+(Tool|AgentRunner)\b` | 行为 trait 属于 `tools` crate-root façade |
| `Arc<\s*Mutex\b` | 运行时状态不属于 share kernel |
| `\btokio::sync::(?:mpsc\|Semaphore\|oneshot\|{ ... })` | 并发原语属于 feature |
| `\bCancellationToken\b` | 属于 feature |
| `\bSystemTime::now\b` / `\bInstant::now\b` | share kernel 不得读时钟 |
| `\bUuid::now_v7\b` / `\bUuid::new_v4\b` | share kernel 不得生成 id |

- **`per_file_exemptions`**：空。带退出条件的临时豁免（命中模式但放行某文件）当前**没有任何**。
- **`forbidden_modules`**（防回归禁单——已迁出，禁止爬回）：

| 路径 | 理由 |
|---|---|
| `agent/shared/src/task/batch.rs` | task 批处理行为属于 Storage 当前 façade |
| `agent/shared/src/task/display.rs` | task 展示行为属于 Storage 当前 façade |
| `agent/shared/src/task/list.rs` | task 列表行为属于 Storage 当前 façade |
| `agent/shared/src/task/store.rs` | task store 行为属于 Storage 当前 façade |

- **依赖白名单（`allowed_dependencies`）**：`serde`, `serde_json`, `serde_yml`, `thiserror`, `tokio`, `tokio-util`, `uuid`, `log`, `logging`, `unicode-width`, `utils`。

### 4a. check-composition-layout.sh

- **功能**：锁定 `agent/composition/src` 的 capability-first wiring modules 结构；Composition 按被装配职责分片，不机械复制 feature crate 的 Hexagonal 四层。
- **允许的顶层源码**：`lib.rs`, `app.rs`, `provider.rs`, `runtime.rs`, `tools.rs`, `update.rs`；`lib.rs` 必须且只能公开声明 `app/provider/runtime/tools/update` 五个 wiring module。
- **禁止结构**：`domain/application/ports/adapters`、`api/business/contract/core/gateway/capabilities` 文件或目录，以及任意未登记顶层源码或子目录。
- **白名单预算**：路径例外、整文件豁免、行级 allow、`grep -v` / exclude / skip 均为 0；允许文件集合是 Target 结构化 policy，不计 migration debt。
- **范围边界**：本守卫证明 Composition 物理结构、façade 模块声明，以及 `FeatureGateways` 的 Provider/Tool gateway 被 Runtime 主 bootstrap 实际消费；全部 Adapter 构造上移由 #950 承接，正式跨 capability 边界替换由 #1022 承接。
- **#1002 故意违规证据**：临时创建 `agent/composition/src/domain.rs` 时，单 Guard 与总编排均以 exit 2 命中 `forbidden Hexagonal/COLA layer`；删除探针后两者 clean pass。
- **#948 注入规则**：`composition/src/runtime.rs` 必须把 `gateways.provider` / `gateways.tools` 传给 Runtime；Runtime 主 bootstrap 必须声明两个 trait-object 参数，并分别经 `build_llm_client_with_gateway`、`new_registry`、`register_all_tools` 消费。规则使用结构化正向断言，白名单仍为 0。
- **#948 故意违规证据**：临时把 `gateways.provider` 改为默认 `provider::wire_provider()` 后，单 Guard 与总编排均以 exit 2 命中 `missing provider gateway forwarding`；恢复后 clean pass。

## 5. check-cola-layer-purity.sh

- **定位**：这是迁移期固定层级守卫，只描述当前执行中的路径与 `crate::<layer>` 引用约束，**NEVER** 代表 [代码组织规范](../01-system/06-code-organization.md) 的 Target 目录原则。
- **功能**：检查未迁移 feature 的迁移期固定层目录与层间依赖方向；Runtime 限制为 `RUNTIME_HEX_LAYERS = {domain, application, ports, adapters, shared}`；Context 限制为 `CONTEXT_HEX_LAYERS = {domain, application, ports, adapters}`；Policy 在 #916 后暂时只允许 `lib.rs`，#917 以真实 Policy PL/AllowAll 恢复 `domain.rs/adapters.rs`；Storage 限制为 `STORAGE_HEX_LAYERS = {domain, ports, adapters}` 并暂时允许过渡目录；Audit 在 #928 后允许真实 `domain + ports + adapters`，继续禁止空 COLA 占位；Tools 同时锁定 #909 scope/profile 授权边界。
- **Tools scope/profile 机械约束**：生产代码不得恢复 `ToolProfile::excludes` 或按 `ToolName` / `tool_name` match 的授权黑名单；`ToolProfile::allowed_capabilities` 必须是唯一私有字段，只允许 `baseline`、`derive_restricted` 与只读 accessor，不得新增 setter/insert/union、`&mut self` 或字段赋值式扩大 API；`RegistryScopeBuilder` / `RegistryScope` 不得由 crate root façade 导出，`ToolRegistry` 不得进入 domain。扫描不设路径白名单、exception、exclude 或 skip，脚本内含 capability 正例及各类违规反例 sanity。
- **实际检查语义**：普通 feature 的顶层目录受 `FEATURE_LAYERS` 限制；Runtime、Context、Policy、Storage 与 Audit 使用各自目标规则。Policy 由 `POLICY_HEX_LAYERS = ∅`、`POLICY_ALLOWED_TOP_LEVEL_FILES = {lib.rs}` 和 legacy 层禁单锁定 #916 后过渡基线；`domain.rs/adapters.rs` 只有 #917 提供真实实现时才能恢复，禁止空层占位；Audit 由 `AUDIT_HEX_LAYERS = {domain, ports, adapters}`、`AUDIT_ALLOWED_TOP_LEVEL_FILES = {lib.rs, domain.rs, ports.rs, adapters.rs}` 和 legacy 禁单锁定 #928 基线；Storage domain 额外禁止物理 fs API、`PathBuf` 与 `crate::adapters`。
- **迁移治理**：Target 覆盖门槛、实施 leaf issue 状态、责任与退出证据 **MUST** 只在 [Migration Governance §1](03-migration-governance.md) 维护；本节 **MUST** 只登记现行脚本行为、常量与白名单。
- **结构定义**：未迁移 feature 使用 `FEATURE_LAYERS`；Runtime/Context/Storage 使用各自登记目标层；Policy 在 #916 后无内部层，#917 随真实实现恢复 `domain/adapters`；Audit 使用 `domain/ports/adapters` 与精确顶层文件集合，后续 #929/#930 只能随真实实现同步增量扩展；Storage 过渡集合有 #883 退出条件，**NEVER** 扩张。
- **被禁依赖方向（`FORBIDDEN_LAYER_DEPS`）**：

| 当前层 | 禁止依赖 |
|---|---|
| `business` | `core`, `gateway`, `contract` |
| `utils` | `business`, `core`, `gateway`, `contract` |
| `contract` | `business`, `core`, `gateway`, `utils` |
| `gateway` | `business`, `utils` |

- **检查方式**：
  - 扫描 `agent/features/*/src/*`：普通 feature 的目录名必须在 `FEATURE_LAYERS`；Runtime、Context、Provider、Policy、Storage 与 Audit 使用各自目标规则。
  - Policy 顶层在 #916 后只允许 `lib.rs`；重新出现 path helper、`api/business/contract/core/gateway/capabilities` 或空 `domain/adapters` 时直接失败，#917 随真实实现恢复。
  - Audit 的 `domain.rs` / `ports.rs` 顶层文件与同名目录均参与层级依赖扫描；跨 crate wildcard `use audit::*` 被拒绝，消费者必须显式导入登记的 root façade 符号。
  - Audit 顶层只允许 #927 已证明的 `lib.rs/domain.rs/ports.rs` 与 `domain/ports` 层；重新出现 `api` / `business` / `contract` / `core` / `gateway` / `capabilities` 文件或目录时直接失败，其他层必须由对应后续实现 Issue 同步更新 Guard。
  - Provider 顶层重新出现 `api` / `business` / `contract` / `core` / `gateway` 文件或目录时直接失败。
  - Storage 顶层重新出现 `api.rs` / `api/`、`business.rs` / `business/`、`contract.rs` / `contract/`、`gateway.rs` / `gateway/` 时直接失败；新增其他未登记目录同样失败。
  - Storage `domain.rs` / `domain/` 若出现物理 fs API、`PathBuf` 或依赖 `crate::adapters`，直接失败。
  - 依赖方向扫描跳过测试路径，并按 `FORBIDDEN_LAYER_DEPS` 检查未迁移横向层及 Runtime、Context、Provider、Storage Hexagonal 层。
  - 检查 `agent/runtime`, `agent/provider`, `agent/tools` 旧目录**不存在**。
- **#988 故意违规证据**：临时恢复 `agent/features/audit/src/api.rs` 后，单 Guard 以 exit 2 命中 `Audit empty or legacy fixed layer is forbidden`；删除违规文件后单 Guard 与总编排均 clean pass。Audit 无路径白名单、整文件豁免或隐式 exclude，白名单预算保持 0。
- **#991 故意违规证据**：临时恢复 `agent/features/storage/src/api.rs` 后，单 Guard 以 exit 2 命中 `Storage legacy fixed layer is forbidden`；删除违规文件后单 Guard 与总编排均 clean pass。
- **#992 故意违规证据**：临时恢复 `agent/features/provider/src/business.rs` 后，单 Guard 以 exit 2 命中 `Provider legacy fixed layer is forbidden`；删除违规文件后 clean pass。Provider 原 13 个 `business → core` 精确例外已全部删除。
- **白名单（`LAYER_MIGRATION_EXCEPTIONS`）**——已登记的迁移期层级倒置：

| 路径 | 目标层 | 上下文 |
|---|---|---|
| `agent/features/tools/src/business/mcp_manager/connection.rs` | `core` | MCP 连接触达 registry |

- **Runtime 六边形迁移例外（`RUNTIME_LAYER_MIGRATION_EXCEPTIONS`）**：4 个精确 `path + target layer` 例外，均为 #995 只迁目录而不改变接线语义后仍存在的 Current 倒置：`application/client/accessors.rs → adapters`、`application/client/from_args.rs → adapters`、`ports/input_buffer.rs → application`、`ports/legacy.rs → application`。脚本对其做 stale 自检；由 #874–#879 删除，禁止扩张。

- **#916 安全所有权规则（`check-context-architecture.sh` R8）**：Policy/Runtime 生产代码禁止恢复 `PathAccess` / `PathKind` / `path_accesses` / `requires_read_before_write` / Policy path helper；Bash safety 禁止与 `allow_all` 条件耦合。路径解析经 Project `WorkspaceRead`，read-before-write 与 Bash safety 留在 Tool adapter。规则无路径例外。

## 6. check-crate-api-boundary.sh

- **功能**：检查跨 feature 访问经稳定 façade。未迁移 feature 继续使用 `::<feature>::api`；Provider、Runtime、Context、Policy、Storage、Project 与 Audit 使用登记的 crate-root 窄 façade；Workflow 只允许 `workflow::api` 与 composition-only wiring。#916 后 Policy root allowlist 为空；Project `WorkspaceRead` 独占安全路径解析，Tool 直接消费；Context 继续通过既有 `guidance` 模块发布 purpose-specific assessment；Audit 的公开面按真实消费者登记。
- **守护**：[05-dependency-rules.md](../01-system/05-dependency-rules.md) §2 R3——禁止穿透 Current 内部层或 capability 私有模块；禁止 Current `api.rs` 暴露内部层；锁定已迁移 feature 的精确根公开面。
- **常量**：
  - `FEATURE_CRATES = {runtime, project, policy, context, provider, tools, storage, hook, audit, update}`
  - `INTERNAL_SEGMENTS = {contract, gateway, core, business, utils}`
  - `API_FACADE_ALLOWED_SEGMENTS = {contract, gateway}`（仅用于仍有 `api.rs` 的 Current feature）
  - `ROOT_REEXPORT_ALLOW = {project: {ProjectContext}}`
  - `ROOT_ACCESS_ALLOW.policy = ∅`：#916 已删除全部 path façade；#917 只可随真实 Policy PL/AllowAll 消费增量登记。
  - `ROOT_ACCESS_ALLOW.context` 继续只登记 `guidance` 模块；`assess_guidance` / `GuidanceAssessment` 经该目的性 façade发布，Context 的 `adapters::prompt::security` 保持私有。
   - `ROOT_ACCESS_ALLOW.audit`：#927 Usage PL/query contract 加 #928 AppendLog PL、File adapter type/factory；后续公开面必须由真实消费者证明。
   - `ROOT_ACCESS_ALLOW.storage`：既有过渡 façade 加 #928 `SafeStorageRoot` / `SafeStorageDir` / typed entry/open options 路径安全 PL；不包含任何 AppendLog/Usage 类型。
  - `ROOT_ACCESS_ALLOW.project`：Project 发布 `ProjectIdentity` / `WorkspaceId` / `WorktreeKind`、三类 workspace port、opaque restore token、结构化 init/control/restore/git 错误与 composition-only wiring；`WorkspaceService`、Git adapter/port 和内部 state **NEVER** 跨 crate 暴露。
  - `ROOT_ACCESS_ALLOW.provider`：#992 后真实消费者使用的 crate-root façade 符号集合；#903 新增 pull-stream PL 的 `CancellationSignal` 与 `InvocationEvent`，并禁止跨 crate 消费仅供 Provider 内部 decoder 迁移的 `LegacyStreamSink`；#904 将 `OpenAIProviderConfig` 收回 Provider 内部；已退役的 `CallbackHandler` / `StreamHandler` 不再允许；`provider::api` 与 `provider::{domain,ports,adapters}` 跨 crate 访问被拒绝。
  - `ROOT_ACCESS_ALLOW.workflow = ∅`：跨 BC 只经 `workflow::api`；`adaptive_reasoning` composition wiring 由函数调用规则允许，graph/node/config 不再作为 crate-root façade。
  - `ROOT_ACCESS_ALLOW.runtime = {AgentClientImpl, UsageSink, from_args_with_workspace}`：`UsageSink` 是 Composition bridge 实现所需的 Runtime-owned outbound port；bootstrap 由 Composition 注入 Task access/capture views，其他 Runtime 内部 port 不公开。
  - `ROOT_ACCESS_ALLOW.context = {context_port, compact, guidance, skill, session, compose_session_task_capture, LegacyTaskCapture}`
  - `ROOT_ACCESS_ALLOW.storage`：#991 过渡期真实消费者使用的 Task/Memory façade 符号集合；#884 已移除 Tool Result 的 `MAX_TOOL_RESULT_CHARS` / `persist_oversized_results`，Runtime 只经 `storage::api::AtomicBlobPort` 与 composition-only `FileSystemBlobAdapter` 接线，不再允许 Storage 业务 helper。#983 的 AtomicDataset 跨 crate 消费 deferred 至 #896，届时再按真实调用点治理；未新增 path exception 或 Guard allowlist。过渡集合最终随 #883/#896 收敛。
  - `CONTEXT_FORBIDDEN_PATHS = {context/src/api.rs, context/src/gateway.rs, context/src/capabilities}`
  - `POLICY_FORBIDDEN_PATHS` 禁止 Policy 的 `api/business/contract/core/gateway/capabilities` 文件与目录恢复
- **检查方式**：
  - 扫描 `agent/`, `apps/`, `packages/` 下的 `*.rs`（跳过 `target/`）；
  - 未迁移 feature 的跨 crate 入口仍必须是 `api`；Project、Runtime、Context、Policy、Storage 只放行对应 `ROOT_ACCESS_ALLOW` 登记符号；Policy 的 `domain/adapters`、Project internal state/service/Git seam、Context `application/ports/adapters` 与 Storage 私有模块 **NEVER** 直接跨 crate 访问，`context::domain` 仅发布稳定 PL；
  - 对仍存在的 `agent/features/*/src/api.rs`，`pub use crate::<segment>` 仅可指向 `contract` / `gateway`；
  - `CONTEXT_FORBIDDEN_PATHS` 任一路径复活立即失败。
- **例外**：无 path 级白名单。Context、Policy 与 Storage root 集合都是结构化 façade policy，不是 migration exception。

### 6t. check-task-persistence-capability.sh

- **功能**：把 Task persistence/restore authority 限定在 Context 与 Composition，Runtime/Tools 只能获得 `TaskAccess`。
- **守护**：扫描 Runtime/Tools 生产 Rust 源码，拒绝 `TaskPersist`、`PreparedTaskRestore`、`TaskRestoreAdapter`、`TaskSnapshotSource`、`SessionTaskAdapters`、`TaskWiring` 与 `wire_task`；专用测试文件及 `trait_reflection.rs` 测试 fixture 不参与生产权限判断。
- **正向路径**：Composition 创建唯一 `TaskWiring`，把 `TaskAccess` 注入 Runtime/Tools，并把 persistence view 交给 Context factory 封装为 capture-only `LegacyTaskCapture`；Runtime 无 prepare/commit restore 权限。
- **sanity / 故意违规**：脚本内置允许 `TaskAccess`、拒绝 persistence symbols 的 detector sanity。#890 临时向 Runtime 生产文件加入 `use task::TaskPersist` 时以 exit code 2 拒绝；移除后通过。依赖图守卫另以 exit code 2 拒绝临时 `task → runtime` 依赖。
- **范围边界**：跨 Project/Config/Memory/Task 的联合 prepare/commit gate 仍由 #871 承接；本守卫不建立联合 coordinator。

### 6a. check-provider-invocation-scope.sh

- **功能**：锁定 #902 的调用隔离边界，防共享 Provider/client 恢复调用期可变状态。
- **守护**：Provider 生产代码禁止 `AtomicU32/AtomicU8/AtomicBool`、`set_max_tokens`、`set_reasoning_level`、`current_reasoning_level` 与 `reasoning_config.lock()`；Runtime 禁止 `shared_client_lock` 与 setter/restore 路径。
- **正向约束**：`LlmProvider::invocation_stream` 必须显式接收 `&InvocationScope`。
- **故意违规验证**：临时向 Provider source 加入 `set_max_tokens` 标记时脚本退出 2；移除后通过。

### 6b. check-provider-pull-stream.sh

- **功能**：锁定 #903 pull-based `InvocationStream` 生产边界，防止 Provider/Runtime/Context 恢复 callback 驱动。
- **守护**：生产代码禁止 `CallbackHandler`、`StreamHandler`、`RuntimeStreamHandler`、`stream_message_raw` 与 `.stream_message(...)`；Runtime 与 Context 的生产代码和测试替身均禁止引用迁移期 `LegacyStreamSink` / `legacy_stream_message`。
- **正向约束**：`LlmProvider` 必须公开 `invocation_stream`；Runtime 通过 `InvocationEventReducer` 主动 poll 并投影事件。
- **范围边界**：Provider 内部 decoder 与测试替身暂可使用明确命名的 legacy sink，不能成为跨 crate 生产入口。

### 6c. check-provider-http-attempt.sh

- **功能**：锁定 #1033 的单 attempt 机械收敛边界，防止各 driver 重新手写请求发送、错误响应体读取与 HTTP/network 诊断日志拼装。
- **守护**：`agent/features/provider/src/adapters/http_attempt.rs` 的 `HttpAttemptExecutor` 是唯一允许发起请求发送与读取失败/成功响应体的入口（成功路径经其 `read_success_json` helper）；`error_log.rs` 的 `log_network_error` / `log_http_error` / `ErrorLogContext` / `LlmApiErrorRecord` 只能被 `http_attempt.rs` 与 `error_log.rs` 自身引用，其余 driver 只能调用窄的 `error_log::log_stream_protocol_error`。
- **扫描范围**：整个 `agent/features/provider/src`（不仅 `adapters/`，覆盖 `domain/`、`ports.rs`、`published_language.rs`、`lib.rs` 等所有生产文件），精确排除 `adapters/http_attempt.rs` 本身（唯一 executor）与本 crate 测试约定文件（`*_tests.rs` / `tests.rs` / `tests/` 目录）；`error_log.rs` 仍在扫描范围内，仅对"禁止自引用诊断 API"这一项单独豁免。
- **测试尾部剥离**：按本 crate 惯例——每个生产文件底部若有整段 `#[cfg(test)] mod tests { ... }`（列级、后跟 `mod NAME {` 而非 `mod NAME;`），扫描时剥离该标记起到文件末尾的内容；**NEVER** 盲目从文件中出现的第一个 `#[cfg(test)]` 处截断——中段声明式 `#[cfg(test)] mod x;`（引用外部测试子模块文件，如 `mod message_conversion_tests;` 或 `#[path = "..."] mod foo;`）不会触发截断，其后的生产代码继续参与扫描；若同一文件同时存在中段声明式标记与末尾内联测试块，只以最后一个"以 `mod NAME {` 开块"的标记为截断点。
- **禁用模式**：
  - `\.send\(\)` / `\.execute\(` —— driver 必须经 `HttpAttemptExecutor::execute` 发送请求，禁止直接调用 `RequestBuilder::send()` / `Client::execute()`；
  - `\.(text|json|bytes|chunk)(::<T>)?\(\)` 紧跟 `\.await`（**跨行**检测，允许两者分处相邻两行，中间可穿插空行/被剥离的整行注释）—— driver 不得直接 `await` `reqwest::Response` 的 `text()`/`json()`/`bytes()`/`chunk()`（含带 turbofish 的 `.json::<T>()`）；`RequestBuilder::json(&body)` 因参数非空、`BoundedErrorBody::text()` 因无 `.await` 而天然豁免；整行 `//`/`///` 注释在匹配前被剔除，避免"注释里提到 `response.json().await`"误报。
  - `log_network_error` / `log_http_error` / `ErrorLogContext` / `LlmApiErrorRecord` —— 仅 `http_attempt.rs`（消费方）与 `error_log.rs`（原生定义处）可引用，其余 driver 一律改走 `error_log::log_stream_protocol_error`。
- **白名单**：无。
- **刻意的简化**：注释豁免只剔除"整行以 `//` 开头"的注释（含 `///` doc comment），不做完整词法级字符串/注释区分；当前代码库内所有已知的 `.json().await` 提及都落在这类整行注释里，足够覆盖真实场景，换取实现简单。
- **故意违规验证**（#1033 doc audit，验证后已还原）：临时在 driver 文件末尾追加 `client.get(url).send().await`、跨行 `.json::<T>().await` 与 `crate::adapters::error_log::log_network_error(..)` 三类探针，单 Guard 分别以 exit 2 命中三条对应说明；另在 `openai_compatible.rs` 中段声明式 `#[cfg(test)] mod message_conversion_tests;` 之后插入 `.send().await` 探针，验证不会被误剥离，同样以 exit 2 命中；移除探针后单 Guard 与总编排均 clean pass。
- **范围边界**：本守卫只锁定“单次 attempt 怎么发、怎么判失败、怎么记一条日志”的机械收敛；不覆盖、也不代表 Runtime 已接管跨调用 retry/backoff 或 stream→non-stream fallback（P6/P7，由 [#905](https://github.com/rushsinging/aemeath/issues/905) 承接收口），也不覆盖 pull-based `InvocationStream`（P4，由 [#903](https://github.com/rushsinging/aemeath/issues/903) 承接）；详见 [Migration Governance §4](03-migration-governance.md#4-provider-现状缺口s2-代码盘点)。
- **失败模式**：命中任一模式即输出对应 `[architecture]` 说明并以 exit code 2 退出。

### 6d. check-provider-driver-acl.sh

- **功能**：锁定 #904 的 Provider-owned Driver ACL，避免 Runtime、Composition 或 CLI 重新解析 driver 并选择具体协议实现。
- **守护**：外层生产代码禁止引用 `ProviderDriverKind::parse`、`OpenAIProviderConfig`、`ProtocolFamily` 或 `DriverSpec`；Provider crate-root 禁止重新导出 `OpenAIProviderConfig`；`LlmClient::from_config` 必须在 Provider 内调用 `DriverSpec::parse`。
- **范围边界**：外层只传递配置中的 driver/source key/API style 原始值；协议族、OpenAI Chat Completions/Responses 方言及实现配置由 Provider 唯一 factory 决定。未知 driver 与非法组合必须 fail-closed。
- **失败模式**：命中任一模式即输出对应 `[architecture]` 说明并以 exit code 2 退出。

## 7. check-context-architecture.sh

- **功能**：守护 agent context 所有权重构（project 拥有 `WorkspaceState`）的架构不变量。
- **守护**：`docs/superpowers/specs/2026-06-07-agent-context-ownership-redesign.md`——workspace 真相单一所有者在 project，tools 只用读/控能力，持久化 DTO 留 session 边界，git 收敛在 `GitCli`。
- **规则**（CTX-R 前缀为 context ownership 局部规则编号，与 [05-dependency-rules.md](../01-system/05-dependency-rules.md) 的全局铁律 R1–R7 **无对应关系**）：

| 编号 | 规则 | 守护目标 |
|---|---|---|
| CTX-R1 | `ToolExecutionContext` 定义不得含 `workspace_root` / `path_base` / `context_stack` 字段 | 防上下文三元组爬回 tools |
| CTX-R2 | `tools/` 不得引用 `PersistedWorkspaceContext` / `WorkspacePersist` | 持久化是 session 边界，tools 不得直接触达 |
| CTX-R3 | `struct WorkspaceState` 仅可在 `project/` 定义；`agent/features/` 内（project 除外）禁止任何 struct 同时打包 `workspace_root + path_base + (context_stack\|stack)` | 防 `WorktreeWorkingContext` 复活 |
| CTX-R4 | 生产代码调 `.workspace_control()` 仅限 `tools/src/business/bash.rs` 与 `worktree.rs` | 控能力集中收口 |
| CTX-R5 | `project/` 内非测试 `Command::new("git")` 仅限 `business/git_ops.rs` | git 收敛在 `GitCli` 适配器 |
| CTX-R6 | `WorkspacePersist` 仅可出现在 `project/`（def/impl）与 `runtime/` | 与 CTX-R2 重叠的兜底 |

- **白名单**（路径级 allowlist）：

| 规则 | 允许 | 说明 |
|---|---|---|
| CTX-R4 | `agent/features/tools/src/business/bash.rs`, `agent/features/tools/src/business/worktree.rs` | 唯一允许调 `.workspace_control()` 的生产文件 |
| CTX-R5 | `agent/features/project/src/business/git_ops.rs` | 唯一允许在 `project/` 调 `Command::new("git")` 的生产文件 |
| 测试放行 | `*_test.rs`, `*_tests.rs`, `tests/` 目录, `#[cfg(test)]` 区域 | R4 / R5 / R6 对测试代码放行 |

- **范围缩窄**：R3 的 triple-bundle 检测**限定 `agent/features/`**（不含 `agent/shared/`, `packages/sdk/`）——这两处是设计允许的序列化/投影形态（`PersistedWorkspaceContext` / `WorkspaceContextView`），不是运行期可变三元组。

## 8. check-forbidden-imports.sh

- **功能**：检查源码 import 边界，禁止非 composition 代码引用生产 adapter。
- **守护**：[05-dependency-rules.md](../01-system/05-dependency-rules.md) §2 R1 / R6——shared adapter 只能由 Composition 装配；feature 与 CLI 不得直接 import 易变 detail。
- **白名单（`RUNTIME_ADAPTER_MIGRATION_EXCEPTIONS`）**——临时精确豁免：

| 路径 | 说明 |
|---|---|
| `agent/features/runtime/src/adapters/runtime.rs` | Runtime-owned ACL 暂时把 shared adapter newtype 适配到 runtime-local port。保留到对应消费方-owned outbound port 由供应 adapter 直接实现、Composition 完成接线且 #982 故意违规证明生效；具体迁移责任与退出证据见 Migration Governance O2/O8 |

- **检查方式**：扫描 `agent/`, `apps/`, `packages/` 下的 `*.rs`（跳过 `*_test.rs` / `*_tests.rs` / `tests/` / `agent/composition/src/`），匹配 `\bshare::adapter\b | \bshared::adapter\b | agent/shared/src/adapter`。
- **自检**：脚本会校验 exception 表中所有路径仍被命中；未命中即报"stale"并要求清理。

## 9. check-tui-tea-purity.sh

- **功能**：检查 TUI update 子树保持 TEA 纯函数语义——副作用一律走 `Cmd` / `Effect` 派发。
- **守护**：[01-architecture-and-dataflow.md](../02-modules/tui/01-architecture-and-dataflow.md) §TEA 架构——`update()` 不得直接 `await` / `spawn` / IO / 调 hook。
- **检查目标目录**（`TUI_PURE_DIRS`）：
  - `apps/cli/src/tui/app`
  - `apps/cli/src/tui/model`
  - `apps/cli/src/tui/view_assembler`
  - `apps/cli/src/tui/view_model`
- **禁用模式**：

| 模式 | 含义 |
|---|---|
| `tokio::spawn\s*\(` | 异步 spawn |
| `std::thread::spawn\s*\(` | 线程 spawn |
| `Command::new\s*\(` | 进程执行 |
| `HookRunner::run` / `.run_hook\s*\(` | Hook 直接调用 |
| `clipboard::` / `arboard::` / `copypasta::` | 剪贴板依赖 |
| `read_clipboard_image\s*\(` / `process_image_file\s*\(` | 剪贴板图片 |
| `Handle::block_on` / `Runtime::block_on` | 同步阻塞运行时 |
| `block_in_place` | 阻塞占位 |
| `.await\b` | 直接 await（不允许在 update） |

- **白名单（`EXEMPT_FILES`）**——runtime / 命令执行层，预期含副作用：

| 文件 | 豁免理由（#59 S5-gap 裁定） |
|---|---|
| `apps/cli/src/tui/app/mod.rs` | 同步 git 元数据探测（`Command::new`），非 update 副作用 |
| `apps/cli/src/tui/app/run_loop.rs` | runtime 编排层（事件循环 `.await`），TEA 副作用执行器所在 |
| `apps/cli/src/tui/app/runtime.rs` | runtime 编排层 / Effect executor 本身 |
| `apps/cli/src/tui/app/slash.rs` | B 块 wontfix：命令主分发为 request-response + `Option<String>` 控制流，Effect 化需把每命令拆成"发 Effect + UiEvent 回流续接"状态机，引入大量 pending 状态、破坏 `Some(prompt)` 直返、重写 `slash_tests`，收益仅 guard 名单少一项、成本高 → **整文件豁免**，不引入行级豁免 |
| `apps/cli/src/tui/app/slash_tests.rs` | 测试 mock |
| `apps/cli/src/tui/app/slash_effect_tests.rs` | 测试 mock |

- **行级豁免锚点**：单行末尾 `// allow tea_side_effect` 注释可放行。
- **注**：A1-A4 已 Effect 化/转纯的文件（`dialog.rs`, `suggestions.rs`, 已删除的 `save.rs`, `memory.rs`）已移出本名单，受严格纯度检查约束。

## 10. check-tui-toplevel-layout.sh

- **功能**：保证 `apps/cli/src/tui` 顶层目录全部在白名单内；同时拦截 feature #57 之前的旧模块路径。
- **白名单**（顶层目录名正则）：`^(adapter|app|effect|model|render|update|view_assembler|view_model|view_state)$`。
- **被禁旧路径**：`tui::(core|output_area|input|display|completion|session)`（含 `crate::` 前缀），命中即视为 feature #57 之前的遗留。

## 11. check-tui-effect-boundary.sh

- **功能**：TUI `model/` 和 `update/` 子树**严格不执行**任何副作用——比 §9 更严，不接受 EXEMPT 名单。
- **检查目标目录**：
  - `apps/cli/src/tui/model`
  - `apps/cli/src/tui/update`
- **禁用模式**（与 §9 一致，**外加** `mpsc::Sender`）：spawn / Command / HookRunner / clipboard / block_on / `.await` / `mpsc::Sender`。
- **白名单**：无。
- **错误信息**：`TUI model/update must describe side effects as Effect values instead of executing them directly`.

## 12. check-tui-model-view-boundaries.sh

- **功能**：保证 TUI model / render / view_assembler / view_model 之间的依赖方向。
- **检查项**：

| 子树 | 禁用模式 | 错误信息 |
|---|---|---|
| `model/` | `ratatui` / `Crossterm` / `Terminal<` / `AgentClient` / `mpsc::Sender` / `tokio::spawn` / `std::thread::spawn` / `Command::new` / `clipboard::` / `arboard::` / `copypasta::` / `read_clipboard_image` / `process_image_file` / `Handle::block_on` / `Runtime::block_on` / `block_in_place` / `.await` | model 必须保持纯函数 |
| `render/` | `find_last_running_tool` / `last running` / `最后一个 running` | render 不得有"标记最后一个 running tool 为完成"的旧 fallback |
| `view_assembler/` | `ratatui` / `tokio::spawn` / `std::thread::spawn` / `Command::new` / `mpsc::Sender` / `.await` / `HookRunner::run` / `.run_hook` | view_assembler 不得渲染或执行副作用 |
| `view_model/` | `crate::tui::model` / `ratatui` | view_model 不得依赖 model 内部或 ratatui |
| `model/` + `view_model/` + `view_assembler/` + `render/` | `sdk::ChatEvent` / `RuntimeStreamEvent` | SDK/runtime 事件协议必须经 adapter 适配后再进入 TUI model |

- **物理遗留守卫**：

| 路径 | 错误信息 |
|---|---|
| `apps/cli/src/tui/core/state` 存在 | `legacy tui/core/state ... forbidden after feature #55` |
| `apps/cli/src/tui/core/update` 存在 | `legacy tui/core/update ... forbidden after feature #55` |
| `apps/cli/src/tui/model/session` 存在 | `tui/model/session is not a fifth model context; session model belongs under runtime` |
| `apps/cli/src/tui/render/output_area/markdown.rs` 存在 | `output render implementation must live under tui/render/output after feature #55` |
| `apps/cli/src/tui/render/output_area/rendered_lines.rs` 存在 | 同上 |
| `apps/cli/src/tui/render/output_area/render_blocks.rs` 存在 | 同上 |
| `apps/cli/src/tui/render/output_area/render_spans.rs` 存在 | 同上 |
| `apps/cli/src/tui/render/output_area/render_status.rs` 存在 | 同上 |
| `apps/cli/src/tui/render/output_area/diff.rs` 存在 | 同上 |
| `apps/cli/src/tui/render/output_area/tool_display/` 存在 | 同上 |

- **白名单**：无。

## 13. check-tui-output-legacy-guards.sh

- **功能**：TUI M2 之后的输出区旁路守卫。
- **检查项**：
  - 整个 `apps/cli/src/tui` 不得出现 `find_last_running` / `last running` / `最后一个 running`。
  - `apps/cli/src/tui/output_area` + `apps/cli/src/tui/render` 不得在非 `if matches!(line.style, LineStyle::ToolCallRunning)` 上下文中调 `cell.set_char('●')`（防覆盖已完成 tool 的状态图标）。
- **白名单**：cell 写入的 `if matches!(line.style, LineStyle::ToolCallRunning)` 守卫条件本身。

## 14. check-tui-block-nesting.sh

- **功能**：gutter 归属不变量（Task 4.2）——gutter（marker/indent）**只由**渲染器 `document_renderer.rs` 经 `apply_gutter` 注入；block 组件的 `render_self` 绝不自写 gutter/marker/indent。
- **检查目标目录**：`apps/cli/src/tui/render/output/blocks/*.rs`。
- **禁用模式**：`\bapply_gutter\s*\(`。
- **白名单**：无（这是高价值、无歧义检查）。
- **刻意的简化**：marker 前缀检测（"● "/"  > " 等）有意不做——`thinking.rs`(💭)、`queued_submission.rs`(⏳) 合法保留内容字形，`ask_user`/`edit_diff` 含内容内前缀，强行正则易误报。

## 15. check-render-isolation.sh

- **功能**：render 隔离守卫（feature #58 输出区单一真相管线）——保证 `apps/cli/src/tui/render/output` 保持纯函数边界。
- **检查目标目录**：`apps/cli/src/tui/render/output`。
- **禁用规则**：

| 规则 | 模式 |
|---|---|
| 禁引 Model 可变类型 | `use\s+crate::tui::model::`（`view_model::` 允许） |
| 禁 fs IO | `\bstd::fs::` |
| 禁 process | `\bstd::process::` |
| 禁 tokio | `\btokio::` |
| 选区上色唯一路径 | `SELECTION_BG` 只能出现在 `selection_overlay.rs`（断言行 `assert` 豁免） |

- **白名单**：
  - `selection_overlay.rs` 是 `SELECTION_BG` 唯一允许文件；
  - `#[cfg(test)]` 测试代码区豁免 IO / 选区断言。

## 16. check-unsafe-text-ops.sh

- **功能**：扫描整个 `apps/cli/src`（不仅 tui），检测因"字节偏移落在非 char 边界"而 panic 的文本操作。
- **禁用模式**：

| 模式 | 含义 |
|---|---|
| `.chars().nth(` | 字符索引误当字节索引 |
| `&var[..]` | `&str` 字节切片 |
| `var[a..b]` | `String` 字节切片 |
| `.split_at(` | `str::split_at` 非 char 边界 panic |

- **白名单（文件级）**：

| 路径 | 理由 |
|---|---|
| `apps/cli/src/tui/render/display/safe_text.rs` | 安全 helper 集中地 |
| `apps/cli/src/tui/display/safe_text.rs` | 历史路径（safe_text 的同义存放） |
| `apps/cli/src/tui/text.rs` | `split_at_ascii` 等只计数字节值 < 128 的 ASCII 字符 helper |

- **行级豁免锚点**：`// allow unsafe_text_op: Vec slice`——对 `Vec<u8>` 切片（非 `str` 切片）显式豁免。
- **刻意的简化**：
  - 不检测 `get(range)`（返回 `Option` 不 panic，是 safe_text 推荐用法，flag 会误伤）；
  - 不检测 `truncate`（本仓库内均为 `Vec::truncate`，flag 会产生误导性注解）。

## 17. check-log-target-prefix.sh

- **功能**：扫描整个仓库的 `.rs` 生产代码，检查所有 `log::xxx!` 宏中的 `target:` 字符串字面量必须以 `aemeath:` 开头。
- **守护**：日志架构统一——所有 log target 必须遵循 `aemeath:<domain>[:<crate>]` 命名约定，避免日志路由到错误的 target。
- **检查方式**：
  - 扫描全部 `.rs` 文件（排除 `target/`、`tests/`、`*test*.rs`、`packages/global/logging/src/`）；
  - 匹配 `target:\s*"[^"]*"` 模式，筛选出不包含 `aemeath:` 的行；
  - 引用常量（如 `target: LOG_TARGET`）不带引号，不会被匹配，自然放行。
- **白名单**：无文件级白名单。
- **例外**：`packages/global/logging/src/`（该目录的精确白名单校验由 Rust 测试 `domain/routing_guard.rs` 覆盖；#936 将其切换为消费唯一 TargetCatalog）。
- **错误信息**：`log target must start with 'aemeath:' (or use LOG_TARGET constant)`。
- **关联 Rust 守卫**：`packages/global/logging/src/domain/routing_guard.rs` 有同功能的 `cargo test` 守卫，使用精确白名单校验。

### 17a. check-logging-scope-context.sh

- **功能**：扫描整个 `packages/global/logging/src` 的生产 Rust 文件，拒绝未登记的 `static`、`static mut`、`lazy_static!` 与 `thread_local!` 状态；`pub`、多行声明同样参与检查。
- **守护**：新日志执行上下文只能通过不可变 `LogContext` 与 Tokio task-local scope 传播；禁止以改名或移动文件的方式恢复 Main/Sub 共享的进程级可变 current 状态。
- **精确白名单**：`context.rs` 的 `SCOPED_CONTEXT / LEGACY_EXECUTION_CONTEXT / BOOT_TS / APP_VERSION / PID / TEST_LOCK`，以及 `file_sink.rs` 的 `UNKNOWN_TARGET_REPORTS / LOGGER`。其中 `LEGACY_EXECUTION_CONTEXT` 只为 #940 消费迁移及 #942 退役保留；白名单 **NEVER** 扩张，新增进程元数据必须有独立设计证据。
- **故意违规证据**：在 `formatter.rs` 临时新增带 `pub(crate)`、多行声明且不使用 `CURRENT_*` 命名的 `ACTIVE_REQUEST` 后，单 Guard 以 exit 2 阻断；恢复后单 Guard clean pass。
- **退出条件**：#942 删除 legacy setters 与 `LEGACY_EXECUTION_CONTEXT` 后，从精确白名单删除该项。

### 17b. check-logging-settings-injection.sh

- **功能**：扫描 Logging、Runtime 与全仓初始化调用，锁定 ConfigSnapshot → LoggingSettings 的单向注入。
- **守护**：`packages/global/logging/src` 生产代码不得读取 env；Runtime 不得调用 `UnifiedLogger::init`、构造 `LoggingSettings`、恢复 `init_logging` 或读取 `AEMEATH_LOG_STDERR`；`UnifiedLogger::init` 的唯一生产调用点必须是 `agent/composition/src/logging_setup.rs`。
- **白名单**：无路径排除或 migration exception；Config `EnvAdapter` 仍是 `AEMEATH_LOG_LEVEL` 的唯一业务 env reader，Composition 仅映射系统输出模式 `AEMEATH_LOG_STDERR`。
- **故意违规证据**：临时在 Logging formatter 恢复 `std::env::var("AEMEATH_LOG_LEVEL")` 后单 Guard 以 exit 2 阻断；恢复后单 Guard clean pass。

## 18. no_mod_rs.sh

- **功能**：架构 guard——检测项目中新增的 `mod.rs` 文件，强制 Rust 2018+ 文件即模块惯例。
- **运行模式**：
  - 默认（无参数）：扫描全仓库 `*/src/*/mod.rs`；
  - `--diff`：仅检查 git 暂存区 `*.rs` 中 `diff-filter=A` 的 `mod.rs`。
- **跳过路径**：`.worktrees/`, `.claude/`, `target/`；默认模式使用目录级 `find -prune`，不得递归扫描 linked worktree、工具缓存或构建产物。
- **白名单**：无（这就是"无例外"规则）。
- **错误信息**：`Rust 2018+ 推荐使用与目录同名的文件代替 mod.rs：foo/mod.rs → foo.rs`.
  
## 19. check-config-env-guard.sh
  
- **位置**：`.agents/hooks/check-config-env-guard.sh`。
- **功能**：禁止 config 包外读取业务 env（`AEMEATH_*`、`*_API_KEY`、`LLM_*`）。业务 env 只允许在白名单路径读取。
- **扫描路径**：`agent/features/**`、`apps/cli/src/**`。
- **业务 env 列表**：`AEMEATH_CONTEXT_SIZE`、`AEMEATH_PROVIDER`、`AEMEATH_API_KEY`、`AEMEATH_BASE_URL`、`AEMEATH_MODEL`、`AEMEATH_MAX_TOKENS`、`AEMEATH_PERMISSION_MODE`、`AEMEATH_MAX_TOOL_CONCURRENCY`、`AEMEATH_MAX_AGENT_CONCURRENCY`、`AEMEATH_VERBOSE`、`AEMEATH_LOG_LEVEL`、`ANTHROPIC_API_KEY`、`OPENAI_API_KEY`、`CLAUDE_API_KEY`、`LLM_API_KEY`、`LLM_BASE_URL`、`DEEPSEEK_API_KEY`、`MINIMAX_API_KEY`、`MIMO_API_KEY`、`VOLCENGINE_CODING_PLAN_API_KEY`、`AGNES_API_KEY`、`OLLAMA_API_KEY`。
- **白名单路径**：
  - `agent/shared/src/config/adapters/env` — EnvAdapter，唯一业务 env 读取点
  - `agent/shared/src/config/adapters/paths` — `AEMEATH_AGENTS_DIR`，路径根
  - `agent/shared/src/config/domain/driver_env` — driver→env name 映射
  - `agent/features/runtime/src/core/config_app_service.rs` — `resolve_provider_api_keys` 在 config 加载时从 env 注入 per-provider API key
  - `packages/global/logging/` — `AEMEATH_LOG_LEVEL` 在 logging 层处理
  - `build.rs` — 编译期
  
## 20. run_tui_single_source_structure_guard（内联）

- **位置**：`check-architecture-guards.sh` 内的 `run_tui_single_source_structure_guard` 函数，**不**是独立脚本。
- **功能**：feature #70 结构化单一真相规则——app/domain 真相只在 `model/` 或 `view_state/`；render widgets 仅保留 render 投影/缓存；退场 adapter 必须只活在 `#[cfg(test)]`。
- **检查项**：

| 编号 | 检查 | 详情 |
|---|---|---|
| 20.1 | `apps/cli/src/tui/adapter.rs` 中 `pub mod input_widget` / `resize` / `live_status_widget` / `status_widget` / `output_widget` / `output_view_widget` 必须在 `#[cfg(test)]` 区域内 | 退场 widget adapter 不得重新恢复为生产模块 |
  | 20.2 | `apps/cli/src/tui/adapter/{input_widget, resize, live_status_widget, status_widget, output_widget, output_view_widget}.rs` 不得恢复生产 writeback/helper API（如 `set_text`、`set_cursor_byte_index`、`resize_mapping`、`map_resize`、`apply_resize`、`&mut InputArea` 等） | 防 widget 重新变成"拥有状态的可变对象" |
  | 20.3 | `apps/cli/src/tui/render/{input/input_area*, status, output_area*}` 不得物理存储 `textarea` / `history` / `saved_input` / `status_type` / `vm` / `thinking` / `is_selecting` / `selection_*` / `spinner` / `task_status_lines` / `queued_submission_lines` / `last_visible_height` / `last_line_count` / `scroll_offset` / `auto_scroll` 等镜像字段 | 真相必须留 `model/` 或 `view_state/` |
| 20.4 | render widgets 不得恢复 completion / suggestions / spinner 镜像存储与类型（`pub(super) suggestions: Vec`、`pub selected_suggestion`、`pub show_suggestions`、`struct SpinnerState`） | 同上 |
| 20.5 | render widgets 不得暴露 `set_text` / `set_cursor_byte_index` / `set_pending_images` / `set_focused` / `set_thinking` / `start_selection` / `set_suggestions` / `accept_suggestion` 等生产状态变更 API | 状态变更一律经 `model` / `view_state` 与 projection helper |
| 20.6 | 生产路径不得调 `(input_area\|status_bar\|output_area).{set_text, set_cursor_byte_index, set_pending_images, get_text, start_selection, scroll_up, start_spinner, set_task_status, ...}` | 调 widget 镜像方法当真相读/写 |
| 20.7 | 生产路径不得写 `widget.{scroll_offset\|auto_scroll\|is_selecting\|selection_*\|spinner\|task_status_lines\|queued_submission_lines} = ...`（排除 `view_state/` 与合法 selection 模块） | 直接赋值 widget 镜像字段 |
| 20.8 | `OutputArea` 选区/复制坐标 helper 必须保持只读纯函数——`get_line_content` / `screen_to_anchor` / `word_bounds_at` / `selected_text_for_view` / `selected_text_for_range` 不得用 `&mut self` | 防选区 helper 偷偷写状态 |
| 20.9 | TUI output document 投影必须集中化；render widgets 不得持有 renderer 缓存、不得调 `refresh_output_widget_from_model` / `handle_resize(visible_height)` / `set_document(...)` / `replace_document(...)` 等旧 API | 渲染真相归 `document_renderer.rs` |
| 20.10 | `queued_submission_lines` 不得作为业务真相从 `OutputArea` 读取（除 `app/update/notice.rs`） | 改走 `ConversationModel.queued_submissions` / `LiveStatusViewModel` |
| 20.11 | `apps/cli/src/tui/**`（除 `model/input/`）中 `model.input.document.{clear, insert_text, replace_text, move_, set_cursor_col, delete_}` 全部禁止 | input 文档变更一律经 `InputIntent → InputModel::apply` |
| 20.12 | `apps/cli/src/tui/app/state/**` 不得镜像 `total_input_tokens` / `total_output_tokens` / `total_api_calls` / `last_input_tokens` / `usage_snapshot` / `record_usage` / `thinking_enabled` | usage/thinking 真相留 `RuntimeModel`，状态由 `StatusViewAssembler` 派生 |

### 21. AgentClient trait 最小化（#567 事件流收口）

`check-agent-client-trait-minimal.sh`

| # | 规则 | 理由 |
|---|---|---|
| 21.1 | `packages/sdk/src/client.rs` 中 `trait AgentClient` 只允许 `chat()`、同步 `cancel_run(run_id)` 与 Config control-plane 的 `config_view()` / `update_config()` | Chat data plane 仍走事件流；Config 查询/更新只交换 SDK 纯值 DTO，禁止把 Config service/reader/watch 暴露给交付层 |

> 该 allow set 仍是窄 façade；后续 interaction/run-control 扩容按对应 leaf 同步更新并提供故意违规证据。

- **白名单**：各 check 内联有具体保留名单（如 19.3 允许 `pub(super) text:&...`、`pub(super) cursor:&...`，允许 `pub(super) focused` / `pending_images` / `content_width` 等投影字段）。

## 22. check-shared-run-loop.sh

- **功能**：验证 Runtime 内只有一个共享 Loop Engine 实现，禁止在 `agent/shared/` 或其他 feature crate 中出现平行 run-loop 实现。
- **守护**：确保 Loop Engine 的单一真相——所有 Main / Sub Run 共用同一驱动骨架（[03-loop-and-state-machine.md](../02-modules/runtime/03-loop-and-state-machine.md)）。
- **检查方式**：确认 Runtime 的 Main/Sub 入口调用唯一 `loop_engine::run_loop`，禁止旧 FSM；并扫描 `agent/features/runtime/src`、`agent/features/tools/src/adapters/agent_tool.rs` 与 `agent/features/tools/src/domain/types/agent.rs`，禁止恢复 Session token 槽或 `max_turns`。
- **失败模式**：发现平行 loop 实现时以 exit code 2 退出。

## 23. check-run-control-boundary.sh

- **位置**：`.agents/hooks/check-run-control-boundary.sh`。
- **功能**：锁定 SDK run control Published Language 与 `AgentClient` 的迁移期扩容边界，防止 #878 atomic cutover 完成前提前引入并发原语或新 RPC。
- **守护**：
  - `packages/sdk/src/run.rs`（SDK run control Published Language）只能是纯值 DTO，禁止 `CancellationToken` / `Sender<` / `Receiver<` / `Mutex<` / `RwLock<` / `Arc<`；
  - `packages/sdk/src/client.rs` 禁止在 #878 atomic cutover 前出现 `cancel_run_step` / `terminate_run` 新 API。
- **检查方式**：`grep -nE` 分别扫描上述两个文件，命中即输出对应说明并 `exit 1`。
- **白名单**：无。
- **失败模式**：`SDK run control Published Language must contain only pure value DTOs.` / `New run control APIs must not reach production AgentClient before #878 atomic cutover.`

## 24. check-config-reader-injection.sh

- **位置**：`.agents/hooks/check-config-reader-injection.sh`。
- **功能**：禁止 Runtime/TUI/CLI 直接构造 `ConfigAppService`，并禁止 TUI/CLI 持有 ConfigReader/Query/Writer/participant/subscription/watch。
- **守护**：Composition 构造唯一 Config wiring；Runtime 只持注入视图与 Main Run snapshot；交付层只见 SDK DTO。
- **检查方式**：扫描 Runtime/TUI 的 `ConfigAppService::new` 及 TUI Config 契约符号。
- **例外**：仅 `trait_reflection.rs` 的测试 fixture；生产路径零例外。
- **失败模式**：`Config reader injection guard FAILED`，exit 2。

## 25. check-production-reachability.sh

- **位置**：`.agents/hooks/check-production-reachability.sh`，调用 `cargo run --quiet -p xtask -- source-guard`。
- **功能**：扫描 `agent/`、`apps/`、`packages/` 的 Rust 源码，拦截非 `cfg(test)` 的公开 `*_for_test` / `test_only` 入口、未保护的 `testing` / `fixture(s)` / `fake(s)` 模块，以及超过集中 baseline 的生产 `allow(dead_code)`。
- **baseline**：`.agents/dead-code-baseline.json` 当前上限 10，记录 owner、原因和退出条件；历史清理由 #649/#947 承接，新增数量必须显式评审。
- **public surface**：`source-guard <root> <output>` 可输出按路径和声明排序的 deterministic public surface，仅供 diff review，不承诺 crates.io semver。
- **执行策略**：source guard 同时进入通用 Git pre-commit（仅 staged 路径命中时）与本地 Stop 守卫；#1018 实测热耗时约 3.1-6s，不新增在线 workflow。

### Git pre-commit（本地钩子，非架构守卫）

- **位置**：`.cargo/hooks/pre-commit`，通过 `core.hooksPath=.cargo/hooks` 启用。
- **行为**：对 staged Rust 执行 `cargo fmt` 并重新暂存；相关源码/守卫变更执行 source guard；TUI scenario/snapshot 变更只检查 `.snap.new` / `.pending-snap`。
- **边界**：不执行 production reachability、workspace/all-target、Coverage、完整 P0 或任何依赖 GitHub 网络的 Issue 治理检查。
- **绕过**：仅使用 Git 原生 `--no-verify`；PR Test plan 必须披露并补跑。

### #677 文档—代码双向校验（人工关键节点）

- **时机**：sub-issue 创建/调整后、叶子 PR 创建前、叶子 PR 合入后、#677 关闭前。
- **检查**：gate marker、开发前差异、无待对齐、实施结果与 PR/commit 证据、延期承接 Issue，以及原生 parent/sub-issue/blocked-by 状态。
- **方式**：使用 `gh issue view` 与 GitHub 原生关系人工核验；该规则只服务 #677 有限生命周期，不沉淀为长期 xtask 或通用 pre-commit。

## 附：钩子体系（非架构守卫）

以下脚本与架构守卫共用 `.agents/aemeath.json` 注册，但**不是**架构守卫；列出供完整理解编排。

### reject-main-edit.sh（PreToolUse）

- **触发**：`PreToolUse` 钩子，`Edit` / `Write` 工具。
- **行为**：
  1. 仅对 `Edit` / `Write` 生效，其他工具直接放行；
  2. 解析 `git rev-parse --show-toplevel`，项目外文件放行；
  3. 用 git 原生检测（`git rev-parse --absolute-git-dir` vs `--git-common-dir`）判断是否在 worktree 中，worktree 放行；
  4. 否则输出 "Edit/Write rejected: 在 main 工作区直接修改" 错误并以 exit 2 阻断。
- **设计意图**：强制 [AGENTS.md](../../../AGENTS.md) §Git 工作流——所有代码 / 文档 / 配置修改都在独立 git worktree 中执行。

### check-unit-tests.sh（Stop）

- **触发**：`Stop` 钩子（无 matcher）。
- **行为**：
  1. 输出 hook 调试信息（`AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR` / `ROOT` / `PWD`）；
  2. 设置 `CARGO_TARGET_DIR=target/hook-tests`（隔离各 checkout 的 cargo 元数据，避免 stale path-dep 缓存）；
  3. 对 11 个 crate 顺序跑 `cargo test --lib`（`cli` 用 `cargo test -p cli --bin aemeath`）；
  4. 每个 crate 默认最多运行 180 秒，可用 `AEMEATH_UNIT_TEST_TIMEOUT_SECS` 调整；超时会终止并回收该 cargo 进程组、输出 crate 名与上限并返回 124；
  5. 任一 crate 超时或测试失败后立即退出，**NEVER** 继续执行后续 crate。
- **被测 crates**：`share, runtime, project, policy, context, provider, tools, storage, hook, audit, cli`。

## 维护说明

- **新增守卫**：在 `.agents/hooks/` 添加 `check-<name>.sh`，在 `check-architecture-guards.sh` 串行调用表中追加一行，并在本文档新增一节。
- **调整白名单**：直接修改脚本中常量；**MUST** 在同一 PR 中同步本文档对应小节。
- **清理 stale exception**：脚本自检会提示"exception list is stale"——按提示删除未命中的精确路径。
- **冲突解决**：本文档与脚本不一致时，**以脚本为准**——脚本是运行时真相源；本文档跟随脚本迁移。

## 相关文档

- 系统级代码组织规范：[../01-system/06-code-organization.md](../01-system/06-code-organization.md)
- 依赖规则与铁律：[../01-system/05-dependency-rules.md](../01-system/05-dependency-rules.md)
- Current → Target 迁移跟踪：[migration-governance.md](03-migration-governance.md)
- 仓库级工作约束：[../../../AGENTS.md](../../../AGENTS.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-17 | 登记 #983 的 AtomicDataset crate-root public façade；因跨 crate Memory 消费 deferred 至 #896，不提前修改 `ROOT_ACCESS_ALLOW.storage`，且 #983 无 Guard exception / allowlist 净增 | [#983](https://github.com/rushsinging/aemeath/issues/983) |
| 2026-07-17 | #903 收紧 `check-provider-pull-stream.sh`：Runtime/Context 的生产代码与测试替身统一禁止跨 crate 使用 legacy sink；同时为 Stop 单 crate 测试增加 180 秒默认超时、进程组回收与失败快速退出，避免单 crate 卡住整个 Hook | [#903](https://github.com/rushsinging/aemeath/issues/903) |
| 2026-07-16 | 新增 `check-provider-http-attempt.sh`（§6c）：锁定 #1033 单 attempt 机械收敛（send/cancel/status 只能经 crate-private `HttpAttemptExecutor`、HTTP/network 诊断日志 API 仅限 `http_attempt.rs` + `error_log.rs`）；串行守卫总数由 25 增至 26（此前 §6a `check-provider-invocation-scope.sh` 已计入，故基数为 25 而非 24） | [#1033](https://github.com/rushsinging/aemeath/issues/1033) |
| 2026-07-16 | 文档审查修正：补登记此前文档从未登记、但脚本编排一直包含的 `check-run-control-boundary.sh`（新增 §23，原 §23/§24 顺延为 §24/§25）；同时收紧 `check-provider-http-attempt.sh` 扫描范围至整个 `agent/features/provider/src`（非仅 `adapters/`）、修复 `strip_test_tail` 首个 `#[cfg(test)]` 盲截尾问题、新增 `.text()/.json()/.bytes()/.chunk()` 跨行 body 读取绕过检测；串行守卫总数由 26 更正为 27，与 `check-architecture-guards.sh` 实际调用数一致 | [#1033](https://github.com/rushsinging/aemeath/issues/1033) |
| 2026-07-14 | 将固定层级检查重分类为迁移期守卫，精确记录按测试路径跳过文件及普通文件内 `#[cfg(test)]` block 仍受扫描的运行时语义，并将覆盖门槛、实施状态、责任与退出证据收口到 Migration Governance | [#972](https://github.com/rushsinging/aemeath/issues/972) |
