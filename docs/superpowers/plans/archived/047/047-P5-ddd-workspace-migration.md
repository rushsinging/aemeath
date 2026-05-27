# Feature 47 DDD Workspace Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将当前 `cli`、`packages/core`、`packages/llm`、`packages/tools` 迁移到已确认的 DDD workspace 目录结构，并同步更新构建脚本与 `.agents` hooks。

**Architecture:** 本轮执行目录与 crate 名迁移，不做大规模领域逻辑拆分。当前实际 crate 映射为：`cli` → `apps/cli`，`aemeath-core` → `shared/kernel`，`aemeath-llm` → `contexts/provider`，`aemeath-tools` → `contexts/tool`；其余 bounded context 在 spec 中保留为后续逻辑拆分目标。

**Tech Stack:** Rust workspace、Cargo path dependencies、Bash hooks、Aemeath CLI。

---

### Task 1: Move crate directories

**Files:**
- Move: `cli/` → `apps/cli/`
- Move: `packages/core/` → `shared/kernel/`
- Move: `packages/llm/` → `contexts/provider/`
- Move: `packages/tools/` → `contexts/tool/`

- [x] **Step 1: Create target directories**

Run: `mkdir -p apps contexts shared`
Expected: directories exist.

- [x] **Step 2: Move current crates**

Run: `mv cli apps/cli && mv packages/core shared/kernel && mv packages/llm contexts/provider && mv packages/tools contexts/tool && rmdir packages`
Expected: old `packages/` is removed and target crate directories exist.

- [x] **Step 3: Verify moved Cargo manifests**

Run: `test -f apps/cli/Cargo.toml && test -f shared/kernel/Cargo.toml && test -f contexts/provider/Cargo.toml && test -f contexts/tool/Cargo.toml`
Expected: command exits 0.

### Task 2: Rename packages and update Cargo paths

**Files:**
- Modify: `Cargo.toml`
- Modify: `apps/cli/Cargo.toml`
- Modify: `shared/kernel/Cargo.toml`
- Modify: `contexts/provider/Cargo.toml`
- Modify: `contexts/tool/Cargo.toml`

- [x] **Step 1: Update workspace members**

Set root members to `apps/cli`, `shared/kernel`, `contexts/provider`, `contexts/tool`.

- [x] **Step 2: Rename package names**

Set package names:
- `apps/cli/Cargo.toml`: `cli`
- `shared/kernel/Cargo.toml`: `kernel`
- `contexts/provider/Cargo.toml`: `provider`
- `contexts/tool/Cargo.toml`: `tool`

- [x] **Step 3: Update path dependencies**

Update dependencies:
- `apps/cli`: `kernel = { path = "../../shared/kernel" }`, `provider = { path = "../../contexts/provider" }`, `tool = { path = "../../contexts/tool" }`
- `contexts/provider`: `kernel = { path = "../../shared/kernel" }`
- `contexts/tool`: `kernel = { path = "../../shared/kernel" }`

- [x] **Step 4: Verify Cargo metadata**

Run: `cargo metadata --no-deps --format-version 1`
Expected: package names include `cli`, `kernel`, `provider`, `tool`.

### Task 3: Update Rust crate imports

**Files:**
- Modify: all Rust files under `apps/cli/src`, `contexts/provider/src`, `contexts/tool/src`

- [x] **Step 1: Replace old crate paths**

Replace:
- `aemeath_core` → `kernel`
- `aemeath_llm` → `provider`
- `aemeath_tools` → `tool`

- [x] **Step 2: Run formatter**

Run: `cargo fmt --all -- --check`
Expected: pass or report formatting changes needed.

- [x] **Step 3: Run compile check**

Run: `cargo check`
Expected: pass.

### Task 4: Update scripts and hooks for target paths/packages

**Files:**
- Modify: `build_cli.sh`
- Modify: `.agents/aemeath.json`
- Modify: `.agents/hooks/check-unit-tests.sh`
- Modify: `.agents/hooks/check-rust-file-lines.sh`
- Modify: `.agents/hooks/check-tui-tea-purity.sh`
- Modify: `.agents/hooks/check-unsafe-text-ops.sh`

- [x] **Step 1: Update build script package**

Change release build package from `aemeath-cli` to `cli`.

- [x] **Step 2: Update unit test hook package names**

Use commands:
- `cargo test -p kernel --lib`
- `cargo test -p provider --lib`
- `cargo test -p tool --lib`
- `cargo test -p cli --bin aemeath`

- [x] **Step 3: Update architecture guard paths**

Change TUI paths from `cli/src/tui` to `apps/cli/src/tui`; restrict line-limit scan to `apps/`, `contexts/`, and `shared/`.

- [x] **Step 4: Keep Stop hook order stable**

Leave `.agents/aemeath.json` Stop hook command order unchanged unless a path must change.

### Task 5: Update docs and verify

**Files:**
- Modify: `docs/feature/active.md`
- Modify: `docs/feature/specs/047-ddd-redesign.md`

- [x] **Step 1: Record implementation progress**

Update #47 active entry to note the current checkpoint moved actual crates to `apps/`, `contexts/`, and `shared/kernel`.

- [x] **Step 2: Run full verification**

Run:
- `cargo fmt --all -- --check`
- `cargo check`
- `cargo test -p cli run_orchestration`
- `cargo test`
- `./build_cli.sh`
- `.agents/hooks/check-architecture-guards.sh`
- `.agents/hooks/check-unit-tests.sh`

Expected: all pass.

- [x] **Step 3: Commit**

Commit message: `refactor: 调整 DDD workspace 目录结构 (refs #47)`.
