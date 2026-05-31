# Feature 47 DDD 目标态对齐实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将当前 `agent/*` 扁平 crate 结构渐进对齐到 `docs/feature/specs/047-ddd-redesign.md` 定义的目标态：`agent/features/*` + `agent/shared` + `agent/composition`，并用守卫强制 feature-boundary / Published Language / OHS / adapter 隔离。

**Architecture:** 本计划只做架构边界对齐，不改变 CLI/TUI 行为，也不恢复 #36 server/agents/proto/infra。迁移采用兼容壳 + 小步移动：先补 composition 与守卫，再迁 supporting features，最后迁 runtime/tools/provider；每个 checkpoint 都保持 `cargo check` 与架构守卫通过。

**Tech Stack:** Rust workspace、Cargo crate 重命名/路径迁移、feature-boundary、COLA、shell architecture guards、`packages/sdk::AgentClient`。

---

## 0. 当前差距基线

对照 `docs/feature/specs/047-ddd-redesign.md`：

| 目标态要求 | 当前状态 | 对齐策略 |
|---|---|---|
| `agent/features/<feature>/` 纵向 feature boundary | 当前为 `agent/runtime`、`agent/tools`、`agent/provider` 等扁平 crate | 用兼容 package name 保持依赖名不变，先移动目录到 `agent/features/*` |
| `agent/shared` 命名 | 当前为 `agent/share` crate，package name `share` | 先目录迁移为 `agent/shared`，package name 暂保留 `share`；后续如需重命名单独拆 plan |
| `agent/composition` 唯一生产装配入口 | 当前装配仍在 `runtime::AgentClientImpl::from_args()` 与 CLI composition root 中 | 新增 `agent/composition`，CLI 只调用 composition 构造 `Arc<dyn AgentClient>` |
| feature 内部 `contract/gateway/core/business/utils/api.rs` | 当前多数 feature 只有 `api.rs` + `business/core/utils`，无 `contract/gateway` | 逐 feature 新增 `contract` / `gateway`，`api.rs` 只 re-export 这两层 |
| runtime `api.rs` 只暴露 contract/gateway | 当前仍 `pub use crate::business::*` / `core::*` / `utils::*` | 分阶段将 CLI/runtime 外部消费迁到 `sdk` / `gateway`，再收窄 `api.rs` |
| `apps/cli` 只直接依赖 `packages/sdk`、`agent/composition` 和纯技术库 | 当前 `apps/cli` 直接依赖 `runtime` + `sdk` | 新增 composition 后把 `runtime` 依赖替换为 `composition` |
| `shared/adapter/**` 仅 composition 可 import | 当前 adapter 类实现仍散落于 `runtime/utils/adapter`、provider/tools 内部 | 先建 adapter 目录和守卫，再迁真实 adapter |
| guard 覆盖目标态 §6.4.8 全部维度 | 当前已有多项守卫，但路径仍基于扁平 `agent/*` | 每次目录迁移同步更新守卫，防止源码与 hook 脱节 |

## 1. 文件结构目标

最终结构：

```text
agent/
  features/
    runtime/
    tools/
    provider/
    prompt/
    project/
    storage/
    policy/
    hook/
    audit/
  shared/
  composition/
packages/
  sdk/
  global/logging/
apps/
  cli/
```

每个 feature 内部按需使用：

```text
agent/features/<feature>/src/
  contract/     # DTO / Event / Command / Query
  gateway/      # Open Host Service trait + wire_<feature>()
  core/         # 内部编排 / use case / port
  business/     # 领域规则 / 状态机 / 不变量
  utils/        # feature 私有工具
  api.rs        # 只 re-export contract + gateway
  lib.rs
```

## 2. 实施总原则

1. **不改行为**：所有任务只改变边界、路径、依赖和公开 API；CLI/TUI 交互语义不变。
2. **不大爆炸移动**：每个任务最多迁一组同类 crate 或一个边界。
3. **先守卫后收口**：新增目录或边界后同任务更新 `.agents/hooks/check-architecture-guards.sh` 聚合脚本及对应子守卫。
4. **兼容 package name**：目录可先迁移，Cargo package name 暂保持 `runtime` / `tools` / `share` 等，降低 import churn。
5. **不创建空层**：没有职责的 `contract/gateway/core/business/utils` 不建空目录；但每个跨 feature 被消费的能力必须经 `api.rs`。
6. **提交粒度**：每个 Task 独立 commit，commit message 引用 `refs #47`。

---

### Task 1: 新增 `agent/composition` 骨架并接入 workspace

**Files:**
- Modify: `Cargo.toml`
- Create: `agent/composition/Cargo.toml`
- Create: `agent/composition/src/lib.rs`
- Create: `agent/composition/src/app.rs`
- Create: `agent/composition/src/runtime.rs`
- Modify: `.agents/hooks/check-cargo-dependency-graph.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`

- [ ] **Step 1: 写最小 composition crate**

`agent/composition/Cargo.toml`：

```toml
[package]
name = "composition"
version = "0.1.0"
edition = "2021"

[dependencies]
runtime = { path = "../runtime" }
sdk = { path = "../../packages/sdk" }
async-trait = { workspace = true }
```

`agent/composition/src/lib.rs`：

```rust
#![deny(clippy::print_stdout, clippy::print_stderr)]

pub mod app;
pub mod runtime;
```

`agent/composition/src/app.rs`：

```rust
use std::sync::Arc;

use runtime::api::client::AgentClientImpl;
use sdk::AgentClient;

pub type AgentClientHandle = Arc<dyn AgentClient>;

pub fn agent_client_from_runtime(client: AgentClientImpl) -> AgentClientHandle {
    Arc::new(client)
}
```

`agent/composition/src/runtime.rs`：

```rust
pub use runtime::api::client::AgentClientImpl;
```

- [ ] **Step 2: 将 workspace 纳入 composition**

在根 `Cargo.toml` workspace members 中添加：

```toml
    "agent/composition",
```

- [ ] **Step 3: 更新依赖图守卫允许 composition**

`.agents/hooks/check-cargo-dependency-graph.sh` 必须表达：

```text
apps/cli -> composition -> runtime
apps/cli -> sdk
composition -> runtime / sdk / features / shared / shared adapters
features/shared 不得依赖 composition
```

- [ ] **Step 4: 验证**

Run: `cargo check -p composition`

Expected: PASS。

Run: `.agents/hooks/check-architecture-guards.sh`

Expected: PASS。

- [ ] **Step 5: Commit**

Run:

```bash
git add Cargo.toml agent/composition .agents/hooks/check-cargo-dependency-graph.sh .agents/hooks/check-architecture-guards.sh
git commit -m "refactor(ddd): add composition root skeleton (refs #47)"
```

---

### Task 2: CLI 依赖从 `runtime` 切到 `composition`

**Files:**
- Modify: `apps/cli/Cargo.toml`
- Modify: `apps/cli/src/runtime_adapter.rs` or current CLI composition-root file that constructs `AgentClientImpl`
- Modify: `.agents/hooks/check-cli-thin-entry.sh`
- Modify: `.agents/hooks/check-forbidden-imports.sh`

- [ ] **Step 1: 调整 CLI Cargo 依赖**

`apps/cli/Cargo.toml`：

```toml
[dependencies]
composition = { path = "../../agent/composition" }
sdk = { path = "../../packages/sdk" }
```

删除：

```toml
runtime = { path = "../../agent/runtime" }
```

- [ ] **Step 2: 修改 CLI composition root import**

将 CLI 中构造 runtime client 的 import 从：

```rust
use runtime::api::client::AgentClientImpl;
```

改为：

```rust
use composition::runtime::AgentClientImpl;
```

如果 CLI 需要 `Arc<dyn AgentClient>`，统一改为：

```rust
let client = composition::app::agent_client_from_runtime(runtime_client);
```

- [ ] **Step 3: 收紧 CLI 守卫**

`.agents/hooks/check-cli-thin-entry.sh` 与 `.agents/hooks/check-forbidden-imports.sh` 应禁止 `apps/cli/src/**/*.rs` 出现：

```text
use runtime::
runtime::api
```

允许例外只应是迁移期明确白名单；本任务结束后白名单应为空。

- [ ] **Step 4: 验证**

Run: `cargo check -p cli`

Expected: PASS。

Run: `.agents/hooks/check-cli-thin-entry.sh`

Expected: PASS，且输出不包含 runtime 白名单残留。

Run: `.agents/hooks/check-architecture-guards.sh`

Expected: PASS。

- [ ] **Step 5: Commit**

Run:

```bash
git add apps/cli/Cargo.toml apps/cli/src .agents/hooks/check-cli-thin-entry.sh .agents/hooks/check-forbidden-imports.sh
git commit -m "refactor(ddd): route cli through composition root (refs #47)"
```

---

### Task 3: 将 `agent/share` 目录迁移为 `agent/shared`

**Files:**
- Move: `agent/share/` → `agent/shared/`
- Modify: `Cargo.toml`
- Modify: all `path = "../share"` / `path = "../../agent/share"` references
- Modify: `.agents/hooks/check-share-minimal-kernel.sh`
- Modify: `.agents/hooks/check-share-no-upstream-deps.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`

- [ ] **Step 1: 目录迁移，package name 暂不改**

保持 `agent/shared/Cargo.toml` 中：

```toml
[package]
name = "share"
```

仅修改 workspace path：

```toml
    "agent/shared",
```

- [ ] **Step 2: 更新所有 Cargo path**

将各 crate 中：

```toml
share = { path = "../share" }
```

改为：

```toml
share = { path = "../shared" }
```

如果从 `apps/` 或 `packages/` 引用，则按相对路径改为：

```toml
share = { path = "../../agent/shared" }
```

- [ ] **Step 3: 更新 share 守卫路径**

所有 guard 中的 `agent/share` 改为 `agent/shared`；守卫语义不变：shared kernel 只允许数据契约，不允许 store / IO / 并发 / 时间 / 行为流程。

- [ ] **Step 4: 验证**

Run: `cargo check -p share`

Expected: PASS。

Run: `.agents/hooks/check-share-minimal-kernel.sh`

Expected: PASS。

Run: `.agents/hooks/check-share-no-upstream-deps.sh`

Expected: PASS。

Run: `.agents/hooks/check-architecture-guards.sh`

Expected: PASS。

- [ ] **Step 5: Commit**

Run:

```bash
git add Cargo.toml agent/shared .agents/hooks
git add -u agent/share
git commit -m "refactor(ddd): rename share directory to shared (refs #47)"
```

---

### Task 4: 建立 `agent/features/*` 目录并迁移 supporting features

**Files:**
- Move: `agent/audit` → `agent/features/audit`
- Move: `agent/policy` → `agent/features/policy`
- Move: `agent/project` → `agent/features/project`
- Move: `agent/storage` → `agent/features/storage`
- Move: `agent/prompt` → `agent/features/prompt`
- Move: `agent/hook` → `agent/features/hook`
- Modify: `Cargo.toml`
- Modify: affected `Cargo.toml` path dependencies
- Modify: `.agents/hooks/check-cargo-dependency-graph.sh`
- Modify: `.agents/hooks/check-crate-api-boundary.sh`

- [ ] **Step 1: 迁移低依赖 supporting features**

保持 package names 不变，例如 `agent/features/project/Cargo.toml` 仍为：

```toml
[package]
name = "project"
```

根 workspace members 改为：

```toml
    "agent/features/audit",
    "agent/features/hook",
    "agent/features/policy",
    "agent/features/project",
    "agent/features/prompt",
    "agent/features/storage",
```

- [ ] **Step 2: 更新 path dependency**

示例：

```toml
project = { path = "../project" }
```

从 feature 内部引用时改为：

```toml
project = { path = "../project" }
```

从 `agent/features/runtime` 后续引用时改为：

```toml
project = { path = "../project" }
```

从 `agent/composition` 引用时改为：

```toml
project = { path = "../features/project" }
```

- [ ] **Step 3: 更新 api-boundary 守卫**

`.agents/hooks/check-crate-api-boundary.sh` 的路径规则必须覆盖：

```text
agent/features/*/src/{core,business,utils}
agent/features/*/src/api.rs
agent/features/*/src/contract
agent/features/*/src/gateway
```

禁止跨 feature import：

```text
::<other_feature>::business
::<other_feature>::core
::<other_feature>::utils
::<other_feature>::contract
::<other_feature>::gateway
```

允许跨 feature import：

```text
::<other_feature>::api::
```

- [ ] **Step 4: 验证**

Run: `cargo check -p audit -p policy -p project -p storage -p prompt -p hook`

Expected: PASS。

Run: `.agents/hooks/check-crate-api-boundary.sh`

Expected: PASS。

Run: `.agents/hooks/check-architecture-guards.sh`

Expected: PASS。

- [ ] **Step 5: Commit**

Run:

```bash
git add Cargo.toml agent/features .agents/hooks
git add -u agent/audit agent/hook agent/policy agent/project agent/prompt agent/storage
git commit -m "refactor(ddd): move supporting domains under features (refs #47)"
```

---

### Task 5: 迁移 capability features：`tools` 与 `provider`

**Files:**
- Move: `agent/tools` → `agent/features/tools`
- Move: `agent/provider` → `agent/features/provider`
- Modify: `Cargo.toml`
- Modify: affected `Cargo.toml` path dependencies
- Modify: `.agents/hooks/check-cargo-dependency-graph.sh`
- Modify: `.agents/hooks/check-crate-api-boundary.sh`
- Modify: `.agents/hooks/check-cola-layer-purity.sh`

- [ ] **Step 1: 迁移目录，保持 package name**

Workspace members：

```toml
    "agent/features/provider",
    "agent/features/tools",
```

- [ ] **Step 2: 保持批准横向依赖**

`tools` 允许依赖：

```toml
project = { path = "../project" }
storage = { path = "../storage" }
policy = { path = "../policy" }
audit = { path = "../audit" }
share = { path = "../../shared" }
```

`provider` 只允许依赖 `share` 和纯技术库。

- [ ] **Step 3: 更新守卫中的批准清单**

`.agents/hooks/check-cargo-dependency-graph.sh` 中保留：

```text
tools -> project/policy/storage/audit/share
provider -> share
```

禁止：

```text
provider -> runtime
tools -> runtime
tools -> prompt
provider -> tools
```

- [ ] **Step 4: 验证**

Run: `cargo check -p tools -p provider`

Expected: PASS。

Run: `.agents/hooks/check-cargo-dependency-graph.sh`

Expected: PASS。

Run: `.agents/hooks/check-cola-layer-purity.sh`

Expected: PASS。

- [ ] **Step 5: Commit**

Run:

```bash
git add Cargo.toml agent/features .agents/hooks
git add -u agent/tools agent/provider
git commit -m "refactor(ddd): move tool and provider domains under features (refs #47)"
```

---

### Task 6: 迁移核心域 `runtime` 到 `agent/features/runtime`

**Files:**
- Move: `agent/runtime` → `agent/features/runtime`
- Modify: `Cargo.toml`
- Modify: `agent/composition/Cargo.toml`
- Modify: affected `Cargo.toml` path dependencies
- Modify: `.agents/hooks/check-cargo-dependency-graph.sh`
- Modify: `.agents/hooks/check-cola-layer-purity.sh`
- Modify: `.agents/hooks/check-crate-api-boundary.sh`

- [ ] **Step 1: 迁移 runtime 目录，保持 package name**

Root workspace member：

```toml
    "agent/features/runtime",
```

`agent/composition/Cargo.toml`：

```toml
runtime = { path = "../features/runtime" }
```

- [ ] **Step 2: runtime 依赖路径改为 sibling feature**

`agent/features/runtime/Cargo.toml` 中 supporting feature path 统一为：

```toml
audit = { path = "../audit" }
hook = { path = "../hook" }
policy = { path = "../policy" }
project = { path = "../project" }
prompt = { path = "../prompt" }
provider = { path = "../provider" }
storage = { path = "../storage" }
tools = { path = "../tools" }
share = { path = "../../shared" }
sdk = { path = "../../../packages/sdk" }
```

- [ ] **Step 3: 更新 runtime COLA 守卫路径**

`.agents/hooks/check-cola-layer-purity.sh` 中 runtime 扫描路径从：

```text
agent/runtime/src
```

改为：

```text
agent/features/runtime/src
```

规则保持：`business` 不依赖外部 SDK/adapter，`core` 通过 port/gateway 编排，`utils` 不承载领域规则。

- [ ] **Step 4: 验证**

Run: `cargo check -p runtime`

Expected: PASS。

Run: `cargo check --workspace`

Expected: PASS。

Run: `.agents/hooks/check-architecture-guards.sh`

Expected: PASS。

- [ ] **Step 5: Commit**

Run:

```bash
git add Cargo.toml agent/features agent/composition .agents/hooks
git add -u agent/runtime
git commit -m "refactor(ddd): move runtime under features (refs #47)"
```

---

### Task 7: 为 supporting features 引入 `contract` / `gateway` / 收窄 `api.rs`

**Files:**
- Modify: `agent/features/{audit,policy,project,storage,prompt,hook}/src/lib.rs`
- Create/Modify: `agent/features/{audit,policy,project,storage,prompt,hook}/src/contract.rs` or `contract/*.rs`
- Create/Modify: `agent/features/{audit,policy,project,storage,prompt,hook}/src/gateway.rs` or `gateway/*.rs`
- Modify: `agent/features/{audit,policy,project,storage,prompt,hook}/src/api.rs`
- Modify: `.agents/hooks/check-crate-api-boundary.sh`

- [ ] **Step 1: 每个 feature 的 `api.rs` 只 re-export contract/gateway**

目标形态：

```rust
pub use crate::contract::*;
pub use crate::gateway::*;
```

禁止形态：

```rust
pub use crate::business::*;
pub use crate::core::*;
pub use crate::utils::*;
```

- [ ] **Step 2: 迁移公开 DTO 到 `contract`**

示例：`project` 的 `WorkingPaths`、`WorktreeRequest`、`WorktreeResult` 进入：

```text
agent/features/project/src/contract/worktree.rs
```

`project/src/contract.rs`：

```rust
pub mod worktree;
pub use worktree::*;
```

- [ ] **Step 3: 迁移公开服务入口到 `gateway`**

示例：`project` 对外服务入口进入：

```text
agent/features/project/src/gateway/worktree_gateway.rs
```

`project/src/gateway.rs`：

```rust
pub mod worktree_gateway;
pub use worktree_gateway::*;
```

- [ ] **Step 4: 保留旧函数名的兼容 gateway**

如果 runtime/tools 仍调用函数式 API，先在 gateway 中保留同名薄 wrapper：

```rust
pub fn current_path() -> std::path::PathBuf {
    crate::business::worktree::current_path()
}
```

后续再把调用方迁到 trait gateway；不要在本任务扩大行为改动。

- [ ] **Step 5: 更新 api-boundary guard**

新增检查：任意 `agent/features/*/src/api.rs` 不允许出现：

```text
crate::business
crate::core
crate::utils
```

只允许：

```text
crate::contract
crate::gateway
```

如果某 feature 暂无公开 contract/gateway，`api.rs` 可为空或只暴露 marker，但必须登记迁移豁免。

- [ ] **Step 6: 验证**

Run: `cargo check -p audit -p policy -p project -p storage -p prompt -p hook`

Expected: PASS。

Run: `.agents/hooks/check-crate-api-boundary.sh`

Expected: PASS。

- [ ] **Step 7: Commit**

Run:

```bash
git add agent/features/{audit,policy,project,storage,prompt,hook}/src .agents/hooks/check-crate-api-boundary.sh
git commit -m "refactor(ddd): publish supporting domain contracts and gateways (refs #47)"
```

---

### Task 8: 为 `tools` / `provider` 引入 Published Language 与 Gateway

**Files:**
- Modify: `agent/features/tools/src/lib.rs`
- Modify: `agent/features/tools/src/api.rs`
- Create/Modify: `agent/features/tools/src/contract.rs` or `contract/*.rs`
- Create/Modify: `agent/features/tools/src/gateway.rs` or `gateway/*.rs`
- Modify: `agent/features/provider/src/lib.rs`
- Modify: `agent/features/provider/src/api.rs`
- Create/Modify: `agent/features/provider/src/contract.rs` or `contract/*.rs`
- Create/Modify: `agent/features/provider/src/gateway.rs` or `gateway/*.rs`

- [ ] **Step 1: tools contract 只放 Tool published language**

`tools` contract 应包含对外 DTO / Query / Command，例如：

```rust
pub use share::tool::{Tool, ToolCall, ToolContext, ToolResult};
pub use crate::business::mcp::McpServerConfig;
```

如果某类型仍在 `business`，本任务只移动类型定义，不移动执行逻辑。

- [ ] **Step 2: tools gateway 暴露 ToolCatalog / registration OHS**

目标 API 形态：

```rust
pub trait ToolGateway: Send + Sync {
    fn register_builtin_tools(&self, registry: &mut ToolRegistry);
    fn register_subagent_tools(&self, registry: &mut ToolRegistry);
}
```

迁移期可继续保留：

```rust
pub use crate::core::tool_registry::ToolRegistry;
pub use crate::core::registry::{register_all_tools, register_subagent_tools};
```

但必须放在 `gateway` 再由 `api.rs` re-export。

- [ ] **Step 3: provider contract 只放 provider published language**

`provider` contract 应包含：

```rust
pub use crate::business::types::{ModelInfo, ModelRequest, ModelResponse, ModelStreamEvent, Usage};
pub use crate::business::stream::{StreamChunk, StreamEvent};
```

不要从 `api.rs` 直接 re-export provider 内部模块。

- [ ] **Step 4: provider gateway 暴露 LLM Gateway / model query**

目标 API 形态：

```rust
#[async_trait::async_trait]
pub trait ProviderGateway: Send + Sync {
    async fn stream_message(&self, request: ModelRequest) -> Result<ModelResponse, LlmError>;
    fn list_models(&self) -> Vec<ModelInfo>;
}
```

迁移期 wrapper 可委托当前 provider client/pool。

- [ ] **Step 5: 验证**

Run: `cargo check -p tools -p provider -p runtime`

Expected: PASS。

Run: `.agents/hooks/check-crate-api-boundary.sh`

Expected: PASS。

Run: `.agents/hooks/check-architecture-guards.sh`

Expected: PASS。

- [ ] **Step 6: Commit**

Run:

```bash
git add agent/features/tools/src agent/features/provider/src .agents/hooks
git commit -m "refactor(ddd): publish tool and provider gateways (refs #47)"
```

---

### Task 9: 收窄 `runtime::api` 为 runtime Published Language / OHS

**Files:**
- Modify: `agent/features/runtime/src/lib.rs`
- Modify: `agent/features/runtime/src/api.rs`
- Create: `agent/features/runtime/src/contract.rs`
- Create: `agent/features/runtime/src/gateway.rs`
- Modify: `agent/features/runtime/src/core/client/*.rs`
- Modify: `agent/composition/src/runtime.rs`
- Modify: `.agents/hooks/check-crate-api-boundary.sh`

- [ ] **Step 1: 建立 runtime contract**

`agent/features/runtime/src/contract.rs`：

```rust
pub use sdk::{
    AgentClient, ChatEvent, ChatRequest, ChatStream, ChangeSet, CostInfo, ProjectContext,
    SessionSnapshot, TaskSummary,
};
```

- [ ] **Step 2: 建立 runtime gateway**

`agent/features/runtime/src/gateway.rs`：

```rust
pub use crate::core::client::AgentClientImpl;
```

后续如引入 `RuntimeGateway` trait，应放在此处；本任务不改变 `AgentClient` 既有契约。

- [ ] **Step 3: 收窄 runtime api**

`agent/features/runtime/src/api.rs` 目标：

```rust
pub use crate::contract::*;
pub use crate::gateway::*;
```

删除所有直接 re-export：

```rust
pub use crate::business::*;
pub use crate::core::*;
pub use crate::utils::*;
pub mod hook { ... }
pub mod policy { ... }
pub mod project { ... }
pub mod prompt { ... }
pub mod provider { ... }
pub mod storage { ... }
pub mod tools { ... }
pub mod core { ... }
```

- [ ] **Step 4: 修正 composition import**

`agent/composition/src/runtime.rs`：

```rust
pub use runtime::api::AgentClientImpl;
```

- [ ] **Step 5: 修正仍依赖 `runtime::api::<supporting>` 的内部代码**

runtime 内部不得通过 `crate::api::<supporting>` 访问下游 feature；应直接使用对应 crate 的 `api`：

```rust
use project::api::current_path;
use tools::api::ToolRegistry;
use provider::api::ApiDriverKind;
```

- [ ] **Step 6: 验证**

Run: `cargo check -p runtime -p composition -p cli`

Expected: PASS。

Run: `.agents/hooks/check-crate-api-boundary.sh`

Expected: PASS，且 runtime api 不再暴露内部层。

Run: `.agents/hooks/check-architecture-guards.sh`

Expected: PASS。

- [ ] **Step 7: Commit**

Run:

```bash
git add agent/features/runtime/src agent/composition/src .agents/hooks/check-crate-api-boundary.sh
git commit -m "refactor(ddd): narrow runtime api to published gateway (refs #47)"
```

---

### Task 10: 建立 `agent/shared/adapter` 并迁移生产 adapter

**Files:**
- Create: `agent/shared/src/adapter.rs`
- Create: `agent/shared/src/adapter/{provider,filesystem,process,git,storage,hook,telemetry}.rs` as needed
- Modify: `agent/shared/src/lib.rs`
- Move: `agent/features/runtime/src/utils/adapter/*` → suitable `agent/shared/src/adapter/*`
- Modify: `agent/composition/src/*.rs`
- Modify: `agent/features/runtime/src/core/port.rs`
- Modify: `.agents/hooks/check-crate-api-boundary.sh`
- Modify: `.agents/hooks/check-forbidden-imports.sh`

- [ ] **Step 1: 新增 shared adapter namespace**

`agent/shared/src/lib.rs` 添加：

```rust
pub mod adapter;
```

`agent/shared/src/adapter.rs`：

```rust
pub mod hook;
pub mod provider;
```

- [ ] **Step 2: 迁移 runtime utils adapter**

将：

```text
agent/features/runtime/src/utils/adapter/hook_adapter.rs
agent/features/runtime/src/utils/adapter/provider_adapter.rs
```

迁到：

```text
agent/shared/src/adapter/hook.rs
agent/shared/src/adapter/provider.rs
```

这些 adapter 实现 runtime/core port 或 supporting gateway 时，只能由 composition 装配；feature 内不得直接 import。

- [ ] **Step 3: composition 装配 adapter**

`agent/composition/src/runtime.rs` 负责构造 adapter 并注入 runtime gateway，目标语义：

```rust
use share::adapter::hook::HookNotificationAdapter;
use share::adapter::provider::ProviderInfoAdapter;
```

如果当前 `AgentClientImpl::from_args()` 尚未接收显式 adapter，本任务先只迁移类型位置，不改构造签名；下一任务再注入。

- [ ] **Step 4: 守卫 adapter 隔离**

`.agents/hooks/check-forbidden-imports.sh` 增加：生产代码中除 `agent/composition/**` 外禁止：

```text
share::adapter
agent/shared/src/adapter
```

测试文件可豁免。

- [ ] **Step 5: 验证**

Run: `cargo check -p share -p runtime -p composition`

Expected: PASS。

Run: `.agents/hooks/check-forbidden-imports.sh`

Expected: PASS。

Run: `.agents/hooks/check-architecture-guards.sh`

Expected: PASS。

- [ ] **Step 6: Commit**

Run:

```bash
git add agent/shared/src agent/features/runtime/src agent/composition/src .agents/hooks
git commit -m "refactor(ddd): isolate production adapters under shared (refs #47)"
```

---

### Task 11: 将 runtime 初始化编排移入 composition

**Files:**
- Modify: `agent/composition/src/app.rs`
- Modify: `agent/composition/src/runtime.rs`
- Modify: `agent/features/runtime/src/core/client/from_args.rs`
- Modify: `agent/features/runtime/src/utils/bootstrap/*.rs`
- Modify: `apps/cli/src/runtime_adapter.rs` or CLI launch file

- [ ] **Step 1: composition 提供统一构造入口**

新增 API：

```rust
use std::sync::Arc;

use sdk::{AgentClient, BootstrapArgs, Result};

pub async fn build_agent_client(args: BootstrapArgs) -> Result<Arc<dyn AgentClient>> {
    let client = crate::runtime::build_runtime_client(args).await?;
    Ok(Arc::new(client))
}
```

如果 `BootstrapArgs` 现名不同，使用 `packages/sdk/src/bootstrap.rs` 的现有 DTO，不新增重复类型。

- [ ] **Step 2: runtime 只暴露接受已解析依赖的构造**

目标方向：

```rust
pub async fn build_runtime_client(args: BootstrapArgs) -> sdk::Result<AgentClientImpl> {
    AgentClientImpl::from_args(args).await
}
```

迁移期允许 `from_args()` 保留在 runtime，但 CLI 不得直接调用。

- [ ] **Step 3: CLI 只调用 composition**

CLI launch file 目标：

```rust
let client = composition::app::build_agent_client(bootstrap_args).await?;
run_tui(client).await?;
```

CLI 不再 import `AgentClientImpl`。

- [ ] **Step 4: 验证**

Run: `cargo check -p cli -p composition -p runtime`

Expected: PASS。

Run: `.agents/hooks/check-cli-thin-entry.sh`

Expected: PASS。

- [ ] **Step 5: Commit**

Run:

```bash
git add agent/composition/src agent/features/runtime/src apps/cli/src
git commit -m "refactor(ddd): move runtime construction behind composition (refs #47)"
```

---

### Task 12: 更新最终架构守卫为目标态路径与语义

**Files:**
- Modify: `.agents/hooks/check-architecture-guards.sh`
- Modify: `.agents/hooks/check-cargo-dependency-graph.sh`
- Modify: `.agents/hooks/check-cli-thin-entry.sh`
- Modify: `.agents/hooks/check-cola-layer-purity.sh`
- Modify: `.agents/hooks/check-crate-api-boundary.sh`
- Modify: `.agents/hooks/check-forbidden-imports.sh`
- Modify: `.agents/hooks/check-share-minimal-kernel.sh`
- Modify: `.agents/hooks/check-share-no-upstream-deps.sh`
- Modify: `.agents/hooks/check-rust-file-lines.sh` if present

- [ ] **Step 1: 守卫覆盖 §6.4.8 全部维度**

总守卫必须执行以下检查：

```text
feature 跨界 import 只能经 <feature>::api
禁止绕过 api 直连 contract/gateway/core/business/utils
禁止 feature 依赖环
feature 禁止直接 import shared::adapter
shared 非 adapter 禁止依赖 features
shared kernel 禁止 IO/store/行为/并发/时间
feature 内 COLA 层间纯度
CLI 薄入口只依赖 composition + sdk
Rust 文件行数 <= 400
```

- [ ] **Step 2: 加 sanity fixtures 或内置样例**

每个关键守卫至少包含允许/禁止样例字符串，例如：

```text
ALLOW: runtime -> tools::api::ToolGateway
DENY:  runtime -> tools::business::BuiltinTool
DENY:  cli -> runtime::api::AgentClientImpl
ALLOW: cli -> composition::app::build_agent_client
```

- [ ] **Step 3: 验证**

Run: `.agents/hooks/check-architecture-guards.sh`

Expected: PASS。

Run: `cargo check --workspace`

Expected: PASS。

Run: `cargo test --workspace`

Expected: PASS。

Run: `cargo clippy --workspace -- -D warnings`

Expected: PASS。

- [ ] **Step 4: Commit**

Run:

```bash
git add .agents/hooks
git commit -m "refactor(ddd): enforce target feature boundary guards (refs #47)"
```

---

### Task 13: 文档同步与完成确认

**Files:**
- Modify: `docs/feature/active.md`
- Modify: `docs/feature/specs/047-ddd-redesign.md` only if target constraints changed; otherwise do not modify
- Modify: this plan file checkbox status as needed

- [ ] **Step 1: 更新 active.md 的 #47 当前状态**

在 `docs/feature/active.md` 的 #47 详情中追加对齐结果：

```text
目标态对齐完成：代码结构已与 spec §6.4 的 agent/features + agent/shared + agent/composition 对齐；CLI 经 packages/sdk + composition 接入 runtime；feature api 只发布 contract/gateway；shared adapter 仅 composition 可用；架构守卫覆盖 §6.4.8 约束。
```

- [ ] **Step 2: 不修改 spec，除非目标态发生变化**

如果只是实现对齐，不要把实现进展写入 `docs/feature/specs/047-ddd-redesign.md`。该 spec 已声明只描述目标态，进度进入 `active.md` / git history。

- [ ] **Step 3: 最终验证**

Run: `cargo check --workspace`

Expected: PASS。

Run: `cargo test --workspace`

Expected: PASS。

Run: `cargo clippy --workspace -- -D warnings`

Expected: PASS。

Run: `.agents/hooks/check-architecture-guards.sh`

Expected: PASS。

- [ ] **Step 4: Commit**

Run:

```bash
git add docs/feature/active.md docs/superpowers/plans/2026-05-31-047-ddd-target-alignment.md
git commit -m "docs(feature): record 047 ddd target alignment completion (refs #47)"
```

---

## 3. 验收标准

1. `agent/` 物理结构符合 `docs/feature/specs/047-ddd-redesign.md` §6.4：`features/`、`shared/`、`composition/` 三分。
2. `apps/cli/Cargo.toml` 不直接依赖 `runtime`、supporting feature 或 `share/shared`；只通过 `composition` + `sdk` 接入业务。
3. 每个 feature 的跨边界 API 只经 `api.rs` 暴露，且 `api.rs` 只 re-export `contract` + `gateway`。
4. Runtime 不再通过 `runtime::api` 伪装转发 supporting domains。
5. `shared` kernel 仍保持最小数据契约，不回流 ToolRegistry / TaskStore / IO / store / 并发原语 / 时间逻辑。
6. 生产代码只有 `agent/composition` 能 import `shared::adapter::*`。
7. `.agents/hooks/check-architecture-guards.sh` 覆盖 spec §6.4.8 的全部守卫维度。
8. `cargo check --workspace`、`cargo test --workspace`、`cargo clippy --workspace -- -D warnings`、架构守卫全部通过。

## 4. 明确不做

1. 不恢复 #36 已移除的 `apps/server`、`apps/agents`、`packages/proto`、`infra`。
2. 不引入运行时 DI 容器，不做目录自动发现。
3. 不在本计划中重命名 package name（例如 `share` → `shared`）以外的公共 crate 名；若需要，另开兼容计划。
4. 不修改 Provider、Tool、Policy、Hook 的业务语义。
5. 不把实现进度写回 `docs/feature/specs/047-ddd-redesign.md`，除非目标态约束本身变化。

## 5. 自检清单

- Spec coverage：覆盖 §6.4 目标目录、§6.4.2 feature 内模板、§6.4.5 shared 语义、§6.4.6 composition root、§6.4.7 依赖规则、§6.4.8 架构守卫、§6.6 AgentClient SDK、§6.7 CLI 薄边界、§12 渐进迁移原则。
- Placeholder scan：本计划不包含 TBD / TODO / implement later；每个任务包含明确文件、改法、验证命令和 commit。
- Type consistency：`composition`、`runtime`、`sdk::AgentClient`、`contract`、`gateway`、`api.rs` 命名在所有任务中保持一致。
