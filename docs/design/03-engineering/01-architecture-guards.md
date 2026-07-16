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
│   └─ check-architecture-guards.sh    串行执行 24 个守卫       │
│   └─ check-unit-tests.sh            cargo test --lib         │
└─────────────────────────────────────────────────────────────┘
```

`check-architecture-guards.sh` 本身**不是**守卫，它只做编排（依次调用下表 24 个守卫）。下表才是真正的守卫集合，按调用顺序排列。

## 守卫索引

| # | 守卫脚本 | 类别 | 守护不变量 |
|---|---|---|---|
| 1 | `check-cargo-dependency-graph.sh` | DDD 边界 | Cargo workspace 依赖方向白名单 |
| 2 | `check-cli-thin-entry.sh` | DDD 边界 | CLI 仅 `composition + sdk`，禁止穿入 runtime |
| 3 | `check-share-no-upstream-deps.sh` | DDD 边界 | share 不依赖任何业务 feature |
| 4 | `check-share-minimal-kernel.sh` | DDD 边界 | share kernel 禁行为/IO/并发/时钟 + 依赖白名单 |
| 5 | `check-cola-layer-purity.sh` | 迁移期固定层级 | 未迁移 Feature 继续受 COLA 依赖方向约束；Runtime、Context、Provider 与 Storage 锁定各自 Hexagonal 目录，Storage 暂留登记过渡模块 |
| 6 | `check-crate-api-boundary.sh` | Feature 边界 | 未迁移 feature 经 `::<crate>::api`；Runtime、Context、Storage 仅开放登记的 crate-root 窄 façade |
| 7 | `check-context-architecture.sh` | 业务约束 | agent context 所有权 CTX-R1–CTX-R6 |
| 8 | `check-forbidden-imports.sh` | 业务约束 | `share::adapter` 仅 composition 可引用 |
| 9 | `check-tui-tea-purity.sh` | TUI 架构 | update 纯函数、副作用走 Effect |
| 10 | `check-tui-toplevel-layout.sh` | TUI 架构 | 顶层模块白名单 + feature #57 旧路径守卫 |
| 11 | `check-tui-effect-boundary.sh` | TUI 架构 | model/update 不直接执行 Effect |
| 12 | `check-tui-model-view-boundaries.sh` | TUI 架构 | model/render/view 边界 + 物理遗留 |
| 13 | `check-tui-output-legacy-guards.sh` | TUI 遗留 | TUI M2 后选区/工具状态旁路守卫 |
| 14 | `check-tui-block-nesting.sh` | TUI 组件 | gutter 仅由 document_renderer 注入 |
| 15 | `check-render-isolation.sh` | TUI 渲染 | render/output 纯函数边界 |
| 16 | `check-unsafe-text-ops.sh` | 安全/IO | 禁非 char 边界 str 切片 |
| 17 | `check-log-target-prefix.sh` | 日志架构 | log target 字符串字面量必须以 `aemeath:` 开头 |
| 18 | `no_mod_rs.sh` | 文件约定 | 禁止 `mod.rs` |
| 19 | `check-config-env-guard.sh` | 配置架构 | 禁止 config 包外读业务 env（`AEMEATH_*`、`*_API_KEY`、`LLM_*`） |
| 20 | `run_tui_single_source_structure_guard`（内联） | TUI 结构 | feature #70 结构化单一真相规则 |
| 21 | `check-agent-client-trait-minimal.sh` | SDK 边界 | `AgentClient` trait 仅 `chat()` + 同步 `cancel_run(run_id)`；禁止恢复 `ChatInputEvent::Cancel` |
| 22 | `check-shared-run-loop.sh` | Runtime 架构 | Main/Sub 只调用唯一共享 Loop Engine；禁止旧 FSM、Session token 槽与 `max_turns` |
| 23 | `check-config-reader-injection.sh` | 配置架构 | runtime 消费方不得直接 `ConfigAppService::new`（例外：from_args / trait_model / composition） |
| 24 | `check-production-reachability.sh` | 测试治理 | Rust xtask 拦截生产 test-only API、未保护 testing/fixture/fake 模块与新增 `allow(dead_code)`；可输出 deterministic public surface |

另有 `check-architecture-guards.sh` 内联 `run_tui_single_source_structure_guard` 守卫（#70 TUI 单一真相 + InputModel 写入约束），见 §19。

## 1. check-cargo-dependency-graph.sh

- **功能**：基于 `cargo metadata` 校验各 crate 的业务依赖是否落在显式白名单内。
- **守护**：[05-dependency-rules.md](../01-system/05-dependency-rules.md) §2 R3 / R4 / R6——固化当前 feature 依赖白名单、薄外部驱动与唯一生产装配入口。默认拒绝未声明的业务依赖，防双向/横向乱依赖。
- **白名单（`business_allow`）**：

| Crate | 允许依赖（workspace crate） |
|---|---|
| `cli` | `composition`, `sdk` |
| `composition` | 全部 FEATURE_CRATES + `share` + `sdk` + `logging` |
| `runtime` | `project`, `policy`, `context`, `provider`, `tools`, `storage`, `hook`, `audit`, `share`, `sdk`, `logging` |
| `share` | `logging`, `utils` |
| `project` | `share` |
| `policy` | `share` |
| `context` | `share`, `provider`, `storage`, `sdk` |
| `provider` | `share` |
| `tools` | `share`, `project`, `storage` |
| `storage` | `share` |
| `hook` | `share` |
| `audit` | `share` |
| `update` | `share`, `sdk`, `logging` |
| `sdk` | `share`, `utils` |
| `logging` | ∅ |
| `utils` | ∅ |

> **Memory BC 当前物理落点**：Memory 领域逻辑当前位于 `share` crate（`share::memory::*`），不是独立 crate。Runtime 经 `share` 间接消费 Memory 能力。独立 Memory crate 的升塑见 [migration-governance.md](03-migration-governance.md) M5/M8。
>
> **Workflow BC 当前物理落点**：Workflow（Reasoning Graph）领域逻辑当前位于 `runtime` crate 内部（`runtime::business::reasoning_graph::*`），不是独立 crate。独立 Workflow crate 的升塑取决于 #972 目录调整后是否拆出。

- **例外**：
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
| `\bToolRegistry\b` | 属于 `tools::api` |
| `\bTaskStore\b` / `\bTaskStoreStats\b` | 属于 Storage crate-root façade |
| `\bstd::fs::` / `\btokio::fs::` / `\bFile::` / `read_to_string` / `write(` / `create_dir` | share 不得做 fs IO |
| `\bstd::process::` / `\btokio::process::` / `Command::new` | share 不得 spawn process |
| `\breqwest::` / `\bhyper::` / `\bureq::` / `\bhttp::` | share 不得做网络/http IO |
| `\bparking_lot::` / `\bRwLock\b` | 状态容器不属于 share |
| `#[\s*async_trait\s*]` | async trait 行为属于 feature |
| `\btrait\s+(Tool|AgentRunner)\b` | 行为 trait 属于 `tools::api` |
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

## 5. check-cola-layer-purity.sh

- **定位**：这是迁移期固定层级守卫，只描述当前执行中的路径与 `crate::<layer>` 引用约束，**NEVER** 代表 [代码组织规范](../01-system/06-code-organization.md) 的 Target 目录原则。
- **功能**：检查未迁移 feature 的迁移期固定层目录与层间依赖方向；Runtime 限制为 `RUNTIME_HEX_LAYERS = {domain, application, ports, adapters, shared}`；Context 限制为 `CONTEXT_HEX_LAYERS = {domain, application, ports, adapters}`；Storage 限制为 `STORAGE_HEX_LAYERS = {domain, ports, adapters}`，并暂时允许 #991 过渡目录 `memory_store/task_store`。
- **实际检查语义**：普通 feature 的顶层目录受 `FEATURE_LAYERS` 限制；Runtime、Context 与 Storage 使用各自 Hexagonal 目标规则。Storage domain 额外禁止 `std::fs`/`tokio::fs`、`PathBuf` 与 `crate::adapters`，其过渡模块由 #883/#884 退出。其余规则按路径识别当前层，并对例外做 stale 自检。
- **迁移治理**：Target 覆盖门槛、实施 leaf issue 状态、责任与退出证据 **MUST** 只在 [Migration Governance §1](03-migration-governance.md) 维护；本节 **MUST** 只登记现行脚本行为、常量与白名单。
- **结构定义**：未迁移 feature 使用 `FEATURE_LAYERS = {contract, gateway, core, business, utils}`；Runtime 使用 `RUNTIME_HEX_LAYERS = {domain, application, ports, adapters, shared}`；Context 使用 `CONTEXT_HEX_LAYERS = {domain, application, ports, adapters}`；Storage 使用 `STORAGE_HEX_LAYERS = {domain, ports, adapters}`、`STORAGE_TRANSITIONAL_MODULES = {memory_store, task_store}`，并以 `STORAGE_LEGACY_LAYERS = {api, business, contract, gateway}` 防旧层恢复。Storage 过渡集合有 #883 退出条件，**NEVER** 扩张。
- **被禁依赖方向（`FORBIDDEN_LAYER_DEPS`）**：

| 当前层 | 禁止依赖 |
|---|---|
| `business` | `core`, `gateway`, `contract` |
| `utils` | `business`, `core`, `gateway`, `contract` |
| `contract` | `business`, `core`, `gateway`, `utils` |
| `gateway` | `business`, `utils` |

- **检查方式**：
  - 扫描 `agent/features/*/src/*`：普通 feature 的目录名必须在 `FEATURE_LAYERS`；Runtime、Context、Provider 与 Storage 使用各自 Hexagonal 目标规则。
  - Provider 顶层重新出现 `api` / `business` / `contract` / `core` / `gateway` 文件或目录时直接失败。
  - Storage 顶层重新出现 `api.rs` / `api/`、`business.rs` / `business/`、`contract.rs` / `contract/`、`gateway.rs` / `gateway/` 时直接失败；新增其他未登记目录同样失败。
  - Storage `domain.rs` / `domain/` 若出现物理 fs API、`PathBuf` 或依赖 `crate::adapters`，直接失败。
  - 依赖方向扫描跳过测试路径，并按 `FORBIDDEN_LAYER_DEPS` 检查未迁移横向层及 Runtime、Context、Provider、Storage Hexagonal 层。
  - 检查 `agent/runtime`, `agent/provider`, `agent/tools` 旧目录**不存在**。
- **#991 故意违规证据**：临时恢复 `agent/features/storage/src/api.rs` 后，单 Guard 以 exit 2 命中 `Storage legacy fixed layer is forbidden`；删除违规文件后单 Guard 与总编排均 clean pass。
- **#992 故意违规证据**：临时恢复 `agent/features/provider/src/business.rs` 后，单 Guard 以 exit 2 命中 `Provider legacy fixed layer is forbidden`；删除违规文件后 clean pass。Provider 原 13 个 `business → core` 精确例外已全部删除。
- **白名单（`LAYER_MIGRATION_EXCEPTIONS`）**——已登记的迁移期层级倒置：

| 路径 | 目标层 | 上下文 |
|---|---|---|
| `agent/features/tools/src/business/mcp_manager/connection.rs` | `core` | MCP 连接触达 registry |

- **Runtime 六边形迁移例外（`RUNTIME_LAYER_MIGRATION_EXCEPTIONS`）**：4 个精确 `path + target layer` 例外，均为 #995 只迁目录而不改变接线语义后仍存在的 Current 倒置：`application/client/accessors.rs → adapters`、`application/client/from_args.rs → adapters`、`ports/input_buffer.rs → application`、`ports/legacy.rs → application`。脚本对其做 stale 自检；由 #874–#879 删除，禁止扩张。

## 6. check-crate-api-boundary.sh

- **功能**：检查跨 feature 访问经稳定 façade。未迁移 feature 继续使用 `::<feature>::api`；Provider、Runtime、Context 与 Storage 使用登记的 crate-root 窄 façade。脚本同时禁止 `provider::api` 与 Provider 内部路径穿透、Context 的纯转发层恢复及 Storage 内部固定层穿透。
- **守护**：[05-dependency-rules.md](../01-system/05-dependency-rules.md) §2 R3——禁止穿透 Current 内部层或 capability 私有模块；禁止 Current `api.rs` 暴露内部层；锁定已迁移 feature 的精确根公开面。
- **常量**：
  - `FEATURE_CRATES = {runtime, project, policy, context, provider, tools, storage, hook, audit, update}`
  - `INTERNAL_SEGMENTS = {contract, gateway, core, business, utils}`
  - `API_FACADE_ALLOWED_SEGMENTS = {contract, gateway}`（仅用于仍有 `api.rs` 的 Current feature）
  - `ROOT_REEXPORT_ALLOW = {project: {ProjectContext}}`
  - `ROOT_ACCESS_ALLOW.provider`：#992 后真实消费者使用的 crate-root façade 符号集合；`provider::api` 与 `provider::{domain,ports,adapters}` 跨 crate 访问被拒绝。
  - `ROOT_ACCESS_ALLOW.runtime = {AgentClientImpl, from_args}`
  - `ROOT_ACCESS_ALLOW.context = {context_port, compact, guidance, skill, session}`
  - `ROOT_ACCESS_ALLOW.storage`：#991 过渡期真实消费者使用的 Task/Memory/Tool Result façade 符号集合；最终随 #880/#983/#883/#884 收敛。
  - `CONTEXT_FORBIDDEN_PATHS = {context/src/api.rs, context/src/gateway.rs, context/src/capabilities}`
- **检查方式**：
  - 扫描 `agent/`, `apps/`, `packages/` 下的 `*.rs`（跳过 `target/`）；
  - 未迁移 feature 的跨 crate 入口仍必须是 `api`；Runtime、Context、Storage 只放行对应 `ROOT_ACCESS_ALLOW` 登记符号；Context `application/ports/adapters` 与 Storage 私有模块 **NEVER** 直接跨 crate 访问，`context::domain` 仅发布稳定 PL；
  - 对仍存在的 `agent/features/*/src/api.rs`，`pub use crate::<segment>` 仅可指向 `contract` / `gateway`；
  - `CONTEXT_FORBIDDEN_PATHS` 任一路径复活立即失败。
- **例外**：无 path 级白名单。Context 与 Storage root 集合都是结构化 façade policy，不是 migration exception。

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
- **例外**：`packages/global/logging/src/`（该目录的守卫由 Rust 测试 `target_guard.rs` 覆盖）。
- **错误信息**：`log target must start with 'aemeath:' (or use LOG_TARGET constant)`。
- **关联 Rust 守卫**：`packages/global/logging/src/target_guard.rs` 有同功能的 `cargo test` 守卫，使用精确白名单校验。

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
  - `agent/shared/src/config/adapter/env` — EnvAdapter，唯一业务 env 读取点
  - `agent/shared/src/config/paths` — `AEMEATH_AGENTS_DIR`，路径根
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
| 21.1 | `packages/sdk/src/client.rs` 中 `trait AgentClient` 只能有 `chat()` 与同步 `cancel_run(run_id)` | #567 后内容输入与结果回传走 `ChatInputEvent` + `ChatEvent`；#700 为即时打断增加唯一 out-of-band 例外，必须按 `RunId` 定位并同步触发 per-Run cancellation scope，禁止新增其它 RPC 或无标识会话级取消 |

> 这是迁移期 **Current** 守卫事实，不是 Target API 上限。[#874](https://github.com/rushsinging/aemeath/issues/874) / [#878](https://github.com/rushsinging/aemeath/issues/878) 落地 Runtime-owned interaction identity 后，`AgentClient` **MUST** 增加 typed `reply_interaction` / `cancel_interaction` 命令；[#982](https://github.com/rushsinging/aemeath/issues/982) **MUST** 同步替换本守卫并用故意违规证明新的窄命令集合，旧守卫在替代证据齐备前 **MUST** 保持运行。

- **白名单**：各 check 内联有具体保留名单（如 19.3 允许 `pub(super) text:&...`、`pub(super) cursor:&...`，允许 `pub(super) focused` / `pending_images` / `content_width` 等投影字段）。

## 22. check-shared-run-loop.sh

- **功能**：验证 Runtime 内只有一个共享 Loop Engine 实现，禁止在 `agent/shared/` 或其他 feature crate 中出现平行 run-loop 实现。
- **守护**：确保 Loop Engine 的单一真相——所有 Main / Sub Run 共用同一驱动骨架（[03-loop-and-state-machine.md](../02-modules/runtime/03-loop-and-state-machine.md)）。
- **检查方式**：扫描 `agent/shared/src/` 中是否存在 `run_loop` / `drive_loop` 等平行 loop 实现。
- **失败模式**：发现平行 loop 实现时以 exit code 2 退出。

## 23. check-config-reader-injection.sh

- **位置**：`.agents/hooks/check-config-reader-injection.sh`。
- **功能**：runtime 消费方（`agent/features/runtime/src/`）不得直接调用 `ConfigAppService::new()`。配置应通过注入的 `Arc<dyn ConfigReader>` port 获取 `ConfigSnapshot`。
- **守护**：DDD 依赖注入原则——`ConfigAppService` 是 Application Service，其构造（`new`）只应在 composition 装配根完成。runtime 消费方直接 new 会绕过依赖注入，导致配置来源不可追踪、测试不可 mock。
- **检查方式**：`grep -rn "ConfigAppService::new" agent/features/runtime/src/ --include="*.rs"`，排除例外文件。
- **例外**：

| 路径 | 理由 |
|---|---|
| `agent/features/runtime/src/core/client/from_args.rs` | CLI 启动路径，暂未改造为注入模式 |
| `agent/features/runtime/src/core/client/trait_model.rs` | CLI 启动路径（`AgentClientImpl` wiring），暂未改造为注入模式 |
| `*_test*` / `tests/` | 测试代码豁免 |

- **注**：`agent/composition/src/`（装配根）不在扫描范围内（守卫只扫 `runtime/src/`），天然放行。
- **失败模式**：`❌ Config reader injection guard FAILED: runtime consumer directly new-ing ConfigAppService`

## 24. check-production-reachability.sh

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
  3. 对 11 个 crate 顺序跑 `cargo test --lib`（`cli` 用 `cargo test -p cli --bin aemeath`）。
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
| 2026-07-14 | 将固定层级检查重分类为迁移期守卫，精确记录按测试路径跳过文件及普通文件内 `#[cfg(test)]` block 仍受扫描的运行时语义，并将覆盖门槛、实施状态、责任与退出证据收口到 Migration Governance | [#972](https://github.com/rushsinging/aemeath/issues/972) |
