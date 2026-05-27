# Feature 47 Strict DDD Workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 #47 严格方案 B 落地为 `apps/` + `crates/` workspace，使 `apps/cli` 只直接依赖 `runtime`，并用 architecture guards 防止依赖边界回退。

**Architecture:** 本次采用可编译 checkpoint 迁移：先创建 `crates/runtime` facade，由 `runtime::api` 过渡暴露 CLI 当前需要的核心类型；再将 `shared/kernel`、`contexts/provider`、`contexts/tool` 迁移到 `crates/core`、`crates/provider`、`crates/tools`，并创建 supporting domain skeleton crates。`runtime` 是唯一编排者，supporting domains 默认只依赖 `core`；现阶段不重写 agent loop，只通过 Cargo 依赖和 import 边界先实现严格入口约束。

**Tech Stack:** Rust workspace、Cargo path dependencies、Bash architecture hooks、cargo metadata、Python3 hook helpers。

---

### Task 1: Create target crates and runtime facade

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/runtime/Cargo.toml`
- Create: `crates/runtime/src/lib.rs`
- Create: `crates/runtime/src/api.rs`
- Create: `crates/project/Cargo.toml`, `crates/project/src/lib.rs`, `crates/project/src/api.rs`
- Create: `crates/policy/Cargo.toml`, `crates/policy/src/lib.rs`, `crates/policy/src/api.rs`
- Create: `crates/prompt/Cargo.toml`, `crates/prompt/src/lib.rs`, `crates/prompt/src/api.rs`
- Create: `crates/storage/Cargo.toml`, `crates/storage/src/lib.rs`, `crates/storage/src/api.rs`
- Create: `crates/hook/Cargo.toml`, `crates/hook/src/lib.rs`, `crates/hook/src/api.rs`
- Create: `crates/audit/Cargo.toml`, `crates/audit/src/lib.rs`, `crates/audit/src/api.rs`

- [x] **Step 1: Create skeleton directories**

Run: `mkdir -p crates/runtime/src crates/project/src crates/policy/src crates/prompt/src crates/storage/src crates/hook/src crates/audit/src`
Expected: directories exist.

- [x] **Step 2: Add runtime package**

Create `crates/runtime/Cargo.toml`:

```toml
[package]
name = "runtime"
version = "0.1.0"
edition = "2021"

[dependencies]
core = { path = "../core" }
provider = { path = "../provider" }
tools = { path = "../tools" }
project = { path = "../project" }
policy = { path = "../policy" }
prompt = { path = "../prompt" }
storage = { path = "../storage" }
hook = { path = "../hook" }
audit = { path = "../audit" }
```

- [x] **Step 3: Add runtime facade source**

Create `crates/runtime/src/lib.rs`:

```rust
pub mod api;
```

Create `crates/runtime/src/api.rs`:

```rust
pub use audit;
pub use core;
pub use hook;
pub use policy;
pub use project;
pub use prompt;
pub use provider;
pub use storage;
pub use tools;
```

- [x] **Step 4: Add support skeleton packages**

For each support crate (`project`, `policy`, `prompt`, `storage`, `hook`, `audit`), create `Cargo.toml` with only `core` path dependency, `src/lib.rs` with `pub mod api;`, and `src/api.rs` with a small marker type such as `ProjectApiMarker`.

- [x] **Step 5: Update workspace members temporarily**

Add target crates to root `Cargo.toml` members while still keeping existing `shared/kernel`, `contexts/provider`, and `contexts/tool` until the next task moves them.

- [x] **Step 6: Verify target skeletons build**

Run: `cargo check -p runtime`
Expected: pass.

### Task 2: Move existing crates to `crates/`

**Files:**
- Move: `shared/kernel/` → `crates/core/`
- Move: `contexts/provider/` → `crates/provider/`
- Move: `contexts/tool/` → `crates/tools/`
- Modify: root `Cargo.toml`
- Modify: `crates/core/Cargo.toml`
- Modify: `crates/provider/Cargo.toml`
- Modify: `crates/tools/Cargo.toml`

- [x] **Step 1: Move directories**

Run: `mv shared/kernel crates/core && mv contexts/provider crates/provider && mv contexts/tool crates/tools && rmdir shared contexts`
Expected: old transition directories removed.

- [x] **Step 2: Rename packages**

Set package names:
- `crates/core/Cargo.toml`: `name = "core"`
- `crates/provider/Cargo.toml`: `name = "provider"`
- `crates/tools/Cargo.toml`: `name = "tools"`

- [x] **Step 3: Update support dependencies**

Set dependencies:
- `crates/provider/Cargo.toml`: `core = { path = "../core" }`
- `crates/tools/Cargo.toml`: `core = { path = "../core" }`

- [x] **Step 4: Update workspace members**

Root `Cargo.toml` members should be exactly:

```toml
members = [
    "apps/cli",
    "crates/core",
    "crates/runtime",
    "crates/project",
    "crates/policy",
    "crates/prompt",
    "crates/provider",
    "crates/tools",
    "crates/storage",
    "crates/hook",
    "crates/audit",
]
```

- [x] **Step 5: Replace old crate names in provider/tools source**

Replace `kernel::` with `core::` in `crates/provider/src` and `crates/tools/src`.

- [x] **Step 6: Verify moved crates**

Run: `cargo check -p core && cargo check -p provider && cargo check -p tools && cargo check -p runtime`
Expected: all pass.

### Task 3: Make CLI depend only on runtime

**Files:**
- Modify: `apps/cli/Cargo.toml`
- Modify: all Rust files under `apps/cli/src`

- [x] **Step 1: Change CLI path dependencies**

Replace `kernel`, `provider`, `tool` dependencies with:

```toml
runtime = { path = "../../crates/runtime" }
```

Keep only technical dependencies such as tokio, clap, ratatui, crossterm, serde_json, logging, rendering, and terminal libraries.

- [x] **Step 2: Replace direct business crate references**

In `apps/cli/src/**/*.rs`, replace:
- `kernel::` → `runtime::api::core::`
- `provider::` → `runtime::api::provider::`
- `tool::` → `runtime::api::tools::`
- `use kernel` → `use runtime::api::core`
- `use provider` → `use runtime::api::provider`
- `use tool` → `use runtime::api::tools`

- [x] **Step 3: Verify CLI no longer has direct business dependencies**

Run a cargo metadata script to assert `cli` direct workspace dependencies are only `runtime`.
Expected: script prints `cli -> runtime` only.

- [x] **Step 4: Verify compile**

Run: `cargo check -p cli`
Expected: pass.

### Task 4: Implement architecture guards

**Files:**
- Create: `.agents/hooks/check-cargo-dependency-graph.sh`
- Create: `.agents/hooks/check-forbidden-imports.sh`
- Create: `.agents/hooks/check-cli-thin-entry.sh`
- Create: `.agents/hooks/check-core-no-upstream-deps.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`
- Modify: `.agents/hooks/check-rust-file-lines.sh`
- Modify: `.agents/hooks/check-unit-tests.sh`

- [x] **Step 1: Add dependency graph guard**

Create a shell script that invokes Python and `cargo metadata --no-deps --format-version 1`, then enforces this allowlist:

```python
BUSINESS_ALLOW = {
    "cli": {"runtime"},
    "runtime": {"core", "project", "policy", "prompt", "provider", "tools", "storage", "hook", "audit"},
    "project": {"core"},
    "policy": {"core"},
    "prompt": {"core"},
    "provider": {"core"},
    "tools": {"core"},
    "storage": {"core"},
    "hook": {"core"},
    "audit": {"core"},
    "core": set(),
}
```

- [x] **Step 2: Add forbidden import guard**

Guard `apps/cli/src/**/*.rs` against `use core::`, `use project::`, `use policy::`, `use prompt::`, `use provider::`, `use tools::`, `use storage::`, `use hook::`, `use audit::`, and any `<crate>::` direct path outside `runtime::api` for business crates.

- [x] **Step 3: Add thin entry guard**

Guard `apps/cli/Cargo.toml` against path dependencies to business crates other than `runtime`.

- [x] **Step 4: Add core upstream guard**

Guard `crates/core/Cargo.toml` against path dependencies to any workspace business crate.

- [x] **Step 5: Wire guard aggregator**

Update `.agents/hooks/check-architecture-guards.sh` to execute all four new guard scripts plus existing guards.

- [x] **Step 6: Update file-line and unit-test hooks**

Update line scanning roots to `apps/` and `crates/`. Update unit test packages to `core`, `runtime`, `project`, `policy`, `prompt`, `provider`, `tools`, `storage`, `hook`, `audit`, and `cli`.

- [x] **Step 7: Verify guards fail on a temporary violation**

Temporarily add a forbidden dependency/import, run corresponding guard, confirm failure, then revert the temporary violation.

- [x] **Step 8: Verify guards pass cleanly**

Run: `.agents/hooks/check-architecture-guards.sh`
Expected: pass.

### Task 5: Update docs, verify, commit, merge

**Files:**
- Modify: `docs/feature/active.md`
- Modify: `docs/feature/specs/047-ddd-redesign.md`
- Modify: `build_cli.sh`

- [x] **Step 1: Update build script package**

Ensure `build_cli.sh` still uses `cargo build --release --package cli`.

- [x] **Step 2: Update feature docs**

Record that strict scheme B implementation has migrated to `apps/` + `crates/`, added runtime facade, and added architecture guards.

- [x] **Step 3: Run full verification**

Run:
- `cargo fmt --all -- --check`
- `cargo check`
- `cargo test -p cli run_orchestration`
- `cargo test`
- `./build_cli.sh`
- `.agents/hooks/check-architecture-guards.sh`
- `.agents/hooks/check-unit-tests.sh`

Expected: all pass.

- [x] **Step 4: Commit**

Commit message:

```text
refactor: 落地严格 DDD workspace 结构 (refs #47)
```

- [x] **Step 5: Merge to main and verify**

Exit worktree, merge branch into `main`, rerun full verification on `main`, then remove the worktree.
