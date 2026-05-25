# Feature 47 P11: crates→services 重命名 + share 包

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:**
1. 将 `crates/` 重命名为 `services/`，语义更精确
2. 新增 `services/share/` 包，作为 services 之间的公共抽象层
3. 解决 tools→project 依赖问题：tools 通过 share 间接调用 project 暴露的接口
4. 更新门禁规则

**Architecture:**

```
services/
├── share/        # 跨 service 公共抽象（trait 定义、re-export）
├── core/         # 核心领域
├── project/      # 项目域（worktree）
├── tools/        # 工具实现
├── storage/      # 持久化
├── policy/       # 策略/安全
├── prompt/       # Guidance
├── provider/     # LLM provider
├── runtime/      # 运行时编排
├── hook/         # Hook（skeleton）
├── audit/        # 审计（skeleton）
```

**依赖规则：**
- `tools` 可依赖 `{core, share}`
- `share` 可依赖所有 services
- 其他 services 仍只依赖 `{core}`
- `runtime` 可依赖所有 services
- `cli` 只依赖 `runtime`

**分支：** `feature/47-p11-services-share`

---

## Task 1: crates/ → services/ 重命名

**影响范围：**
- `Cargo.toml` workspace members 路径
- 所有 `Cargo.toml` 中 `path = "../xxx"` 相对路径（不需要改，因为目录结构不变）
- `.agents/hooks/check-cargo-dependency-graph.sh` 中 `business_allow` 不需要改（用 package name 不是路径）
- 所有 `#[path = "..."]` 引用（内部，不需要改）
- CI/CD 脚本、文档中的路径引用

**Steps:**

- [ ] **Step 1: git mv crates services**

```bash
git mv crates services
```

- [ ] **Step 2: 更新根 Cargo.toml workspace members**

```toml
members = [
    "apps/cli",
    "services/core",
    "services/runtime",
    "services/project",
    "services/policy",
    "services/prompt",
    "services/provider",
    "services/tools",
    "services/storage",
    "services/hook",
    "services/audit",
]
```

注意：各子 crate 的 `Cargo.toml` 中 `path = "../xxx"` 引用不需要改，因为目录结构不变，只是父目录名变了。

- [ ] **Step 3: 搜索并更新所有引用 `crates/` 的文件**

涉及文件：
- `.agents/hooks/check-architecture-guards.sh`（如果有 crates 路径引用）
- `docs/` 中文档引用
- `.agents/` 中配置引用
- `build_cli.sh`（如果存在）

```bash
grep -rn 'crates/' --include='*.sh' --include='*.md' --include='*.toml' --include='*.json' .
```

- [ ] **Step 4: 验证编译**

```bash
cargo build
```

---

## Task 2: 新增 services/share 包

**Steps:**

- [ ] **Step 1: 创建 services/share/Cargo.toml**

```toml
[package]
name = "share"
version = "0.1.0"
edition = "2021"

[dependencies]
aemeath_core = { package = "core", path = "../core" }
project = { path = "../project" }
async-trait = { workspace = true }
```

- [ ] **Step 2: 创建 services/share/src/lib.rs**

```rust
//! 跨 service 公共抽象层
//!
//! share 定义 services 之间的公共接口（trait），具体实现由各 service 提供。
//! tools 等消费者只依赖 share，不直接依赖具体 service。

pub mod worktree_ops;

// Re-export 常用类型供消费者直接使用
pub use worktree_ops::WorktreeOps;
```

- [ ] **Step 3: 创建 services/share/src/worktree_ops.rs**

定义 worktree 操作 trait：

```rust
use aemeath_core::tool::ToolContext;
use async_trait::async_trait;
use std::path::PathBuf;

/// worktree 操作抽象
///
/// project crate 提供具体实现。tools crate 通过此 trait 调用，
/// 避免直接依赖 project。
#[async_trait]
pub trait WorktreeOps: Send + Sync {
    /// 进入指定路径的 worktree
    fn enter_worktree(&self, ctx: &mut ToolContext, path: PathBuf) -> Result<(), String>;

    /// 退出当前 worktree
    fn exit_worktree(&self, ctx: &mut ToolContext) -> Result<(), String>;
}
```

但等等——实际 project::worktree 的函数签名是：

```rust
pub fn enter_worktree(ctx: &mut ToolContext, path: PathBuf) -> Result<(), String>
pub fn exit_worktree(ctx: &mut ToolContext) -> Result<(), String>
```

是同步函数，不需要 async_trait。简化为：

```rust
/// worktree 操作抽象
///
/// tools 通过此模块调用 project 的 worktree 函数，
/// 避免直接依赖 project crate。
pub use project::worktree::{enter_worktree, exit_worktree};
```

这样 share 只是 re-export project 的函数。tools 依赖 share 就能调用。

更合理的做法：share 不引入 trait，直接 re-export。因为当前场景不需要多态/替换实现。

最终 `services/share/src/worktree_ops.rs`：

```rust
/// worktree 操作的公共接口
///
/// tools 通过此模块调用 project 的 worktree 函数，
/// 避免直接依赖 project crate（门禁不允许 tools→project）。
pub use project::worktree::{
    enter_worktree, exit_worktree,
    workspace_context_from_tool_context,
};
```

- [ ] **Step 4: 更新根 Cargo.toml 添加 share**

```toml
members = [
    ...
    "services/share",
]
```

- [ ] **Step 5: 验证编译**

```bash
cargo build -p share
```

---

## Task 3: worktree 工具通过 share 调用 project

**Steps:**

- [ ] **Step 1: 更新 tools/Cargo.toml**

移除 `project` 依赖，添加 `share` 依赖：

```toml
[dependencies]
aemeath_core = { package = "core", path = "../core" }
share = { path = "../share" }
```

- [ ] **Step 2: 更新 tools/src/worktree.rs**

将 `project::worktree::` 改为 `share::worktree_ops::`：

```rust
use share::worktree_ops::{enter_worktree, exit_worktree};
```

所有调用点：
- `project::worktree::enter_worktree(ctx, path)` → `enter_worktree(ctx, path)`
- `project::worktree::exit_worktree(ctx)` → `exit_worktree(ctx)`

- [ ] **Step 3: 验证编译**

```bash
cargo build -p tools && cargo build -p runtime && cargo build -p cli
```

---

## Task 4: 更新门禁规则

**Steps:**

- [ ] **Step 1: 更新 check-cargo-dependency-graph.sh**

```python
business_allow = {
    "cli": {"runtime"},
    "runtime": {"core", "project", "policy", "prompt", "provider", "tools", "storage", "hook", "audit", "share"},
    "share": {"core", "project"},
    "project": {"core"},
    "policy": {"core"},
    "prompt": {"core"},
    "provider": {"core"},
    "tools": {"core", "share"},
    "storage": {"core"},
    "hook": {"core"},
    "audit": {"core"},
    "core": set(),
}
```

关键变化：
- `tools`: `{core}` → `{core, share}`
- 新增 `share`: 可依赖 `{core, project}`
- `runtime`: 新增 `share`

- [ ] **Step 2: 运行门禁验证**

```bash
bash .agents/hooks/check-cargo-dependency-graph.sh
bash .agents/hooks/check-architecture-guards.sh
```

---

## Task 5: 完整验证 + 文档 + 提交

- [ ] **Step 1: 完整编译**

```bash
cargo build
cargo clippy --workspace -- -D warnings
cargo test -p share
cargo test -p tools
cargo test -p project
cargo test -p runtime
```

- [ ] **Step 2: 更新 docs/feature/active.md**

- [ ] **Step 3: 更新 P10 plan 进展**

- [ ] **Step 4: 提交**

```bash
git add -A
git commit -m "refactor: crates→services + share 包 (refs #47)

- crates/ 重命名为 services/
- 新增 services/share：跨 service 公共抽象层
- worktree 工具通过 share 间接调用 project
- 更新门禁：tools 可依赖 share，share 可依赖 project

Co-Authored-By: Aemeath (Zhipu/glm-5.1) <github:rushsinging/aemeath>"
```

- [ ] **Step 5: 合并回 main 并验证**

---

## 执行顺序

Task 1 (重命名) → Task 2 (share 包) → Task 3 (worktree 迁移) → Task 4 (门禁) → Task 5 (验证提交)

Task 1 必须先完成，Task 2-4 依赖 Task 1。
