# Feature #36 Sprint 0.5 Monorepo Layout Migration Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Sprint 1 业务 API 开始前，把仓库迁移到 `apps/*` + `packages/*` 的 monorepo 结构，并移除 `share/`。

**Architecture:** 应用入口放入 `apps/`，公共库、协议和 SDK 放入 `packages/`。Rust package 名保持兼容，目录名改为短名，避免同时修改大量 `use aemeath_core` 代码。

**Tech Stack:** Rust workspace、Cargo path dependencies、tonic-build/prost proto 生成、Docker Compose。

---

## 目标目录结构

```text
apps/
  cli/        # 原 aemeath-cli
  server/     # 原 server
  agents/     # 原 agents
packages/
  core/       # 原 aemeath-core，package name 仍为 aemeath-core
  llm/        # 原 aemeath-llm，package name 仍为 aemeath-llm
  tools/      # 原 aemeath-tools，package name 仍为 aemeath-tools
  proto/      # 原 share/proto
  sdk/        # 原 share/openapi/sdk
infra/
  mongodb/
  deploy/
docs/
```

## Task 1: 移动目录

**Files:**
- Move: `aemeath-cli/` → `apps/cli/`
- Move: `server/` → `apps/server/`
- Move: `agents/` → `apps/agents/`
- Move: `aemeath-core/` → `packages/core/`
- Move: `aemeath-llm/` → `packages/llm/`
- Move: `aemeath-tools/` → `packages/tools/`
- Move: `share/proto/` → `packages/proto/`
- Move: `share/openapi/sdk/` → `packages/sdk/`

- [ ] **Step 1: 确认工作区干净**

Run:
```bash
git status --short --untracked-files=all
```

Expected: 只包含本计划相关文档修改，不能有未解释的代码改动。

- [ ] **Step 2: 创建目标目录**

Run:
```bash
mkdir -p apps packages
```

Expected: `apps/` 和 `packages/` 存在。

- [ ] **Step 3: 移动 Rust 应用与公共库**

Run:
```bash
git mv aemeath-cli apps/cli
git mv server apps/server
git mv agents apps/agents
git mv aemeath-core packages/core
git mv aemeath-llm packages/llm
git mv aemeath-tools packages/tools
```

Expected: 顶层不再有 `aemeath-cli/`、`server/`、`agents/`、`aemeath-core/`、`aemeath-llm/`、`aemeath-tools/`。

- [ ] **Step 4: 移动 proto 与 SDK 并移除 share**

Run:
```bash
git mv share/proto packages/proto
git mv share/openapi/sdk packages/sdk
rmdir share/openapi
rmdir share
```

Expected: `packages/proto/common.proto` 与 `packages/sdk/ts/package.json` 存在，`share/` 不存在。

## Task 2: 更新 Cargo workspace 与 path dependencies

**Files:**
- Modify: `Cargo.toml`
- Modify: `apps/cli/Cargo.toml`
- Modify: `apps/server/Cargo.toml`
- Modify: `apps/agents/Cargo.toml`
- Modify: `packages/llm/Cargo.toml`
- Modify: `packages/tools/Cargo.toml`

- [ ] **Step 1: 更新根 workspace members**

Modify `Cargo.toml`:
```toml
[workspace]
members = [
    "packages/core",
    "packages/llm",
    "packages/tools",
    "apps/cli",
    "apps/server",
    "apps/agents",
]
resolver = "2"
```

Expected: workspace 不再引用旧顶层 crate 目录。

- [ ] **Step 2: 更新所有 path dependencies**

Rules:
- 从 `apps/cli` 到公共库使用 `../../packages/<name>`。
- 从 `apps/server` 到公共库使用 `../../packages/<name>`。
- 从 `apps/agents` 到公共库使用 `../../packages/<name>`。
- 从 `packages/llm` 或 `packages/tools` 到 `packages/core` 使用 `../core`。

Expected examples:
```toml
aemeath-core = { path = "../../packages/core" }
aemeath-llm = { path = "../../packages/llm" }
aemeath-tools = { path = "../../packages/tools" }
```

Inside `packages/llm/Cargo.toml` and `packages/tools/Cargo.toml`:
```toml
aemeath-core = { path = "../core" }
```

- [ ] **Step 3: 检查 Cargo metadata**

Run:
```bash
cargo metadata --no-deps >/tmp/aemeath-cargo-metadata.json
```

Expected: 命令成功，metadata 中 package 路径指向 `apps/*` 与 `packages/*`。

## Task 3: 更新 proto build 与 infra 路径

**Files:**
- Modify: `apps/server/build.rs`
- Modify: `infra/deploy/Dockerfile.server`
- Modify: `infra/deploy/docker-compose.dev.yaml`

- [ ] **Step 1: 更新 server build.rs proto 路径**

Modify `apps/server/build.rs` so proto root points to:
```rust
let proto_dir = PathBuf::from("../../packages/proto");
```

Expected: `cargo test -p server` 能从 `packages/proto/*.proto` 生成代码。

- [ ] **Step 2: 更新 Dockerfile server 路径**

Modify `infra/deploy/Dockerfile.server` so it copies/builds the new workspace layout:
```dockerfile
COPY Cargo.toml Cargo.lock ./
COPY apps ./apps
COPY packages ./packages
COPY infra ./infra
RUN cargo build -p server
CMD ["target/debug/server"]
```

Expected: Docker build context 不再引用旧顶层 `server/` 或 `share/`。

- [ ] **Step 3: 更新 compose build path 如有旧路径引用**

Run:
```bash
docker compose -f infra/deploy/docker-compose.dev.yaml config >/tmp/aemeath-compose-config.out
```

Expected: compose 配置合法。

## Task 4: 更新文档路径

**Files:**
- Modify: `CLAUDE.md`
- Modify: `docs/feature/specs/036-*.md`
- Modify: `docs/feature/plans/036-*.md`

- [ ] **Step 1: 更新 CLAUDE.md 项目结构**

Replace old top-level crate structure with:
```text
aemeath/
├── apps/
│   ├── cli/       # CLI 二进制入口 + TUI + 旧版 REPL
│   ├── server/    # #36 API Server：REST/WS + gRPC
│   └── agents/    # #36 Agent runtime 与角色配置
├── packages/
│   ├── core/      # 核心库：消息、工具、配置、会话、成本追踪、压缩
│   ├── llm/       # LLM 客户端：provider API 调用、流式响应、模型池
│   ├── tools/     # 工具注册：文件读写、搜索、Bash、Agent、Web 等
│   ├── proto/     # 共享 proto 定义
│   └── sdk/       # 外部 SDK
├── infra/
├── docs/
└── TODO.md
```

- [ ] **Step 2: 批量检查旧路径残留**

Run:
```bash
rg "share/|aemeath-core/|aemeath-cli/|aemeath-llm/|aemeath-tools/|server/|agents/" docs/feature/specs docs/feature/plans CLAUDE.md
```

Expected: 只允许历史说明或 Sprint 0 已完成描述中出现旧路径；Sprint 0.5 之后的路径必须使用 `apps/*` 或 `packages/*`。

## Task 5: 验证并提交

**Files:**
- All moved/modified files

- [ ] **Step 1: 运行 Rust 验证**

Run:
```bash
cargo test -p server
cargo test -p agents
cargo check
```

Expected: 全部通过。

- [ ] **Step 2: 运行 compose 验证**

Run:
```bash
docker compose -f infra/deploy/docker-compose.dev.yaml config >/tmp/aemeath-compose-config.out
```

Expected: 命令成功。

- [ ] **Step 3: 确认 share 已移除**

Run:
```bash
test ! -e share
```

Expected: 命令成功。

- [ ] **Step 4: 提交**

Run:
```bash
git add -A
git commit -m "refactor(#36): migrate workspace to apps packages layout"
```

Expected: commit 成功。
