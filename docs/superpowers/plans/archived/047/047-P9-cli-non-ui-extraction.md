# Feature 47 P9: CLI 非 UI 模块抽取到 runtime

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 CLI 中 reflection.rs、mcp_loader.rs、logging_setup.rs、prompt.rs、image.rs 等非 UI 模块迁入 runtime crate，使 CLI 只保留纯 UI/入口层。

**Architecture:** 迁移方向为 `apps/cli/src/*.rs` → `crates/runtime/src/` 对应模块。每个模块迁入后，CLI 通过 `::runtime::api::` 引用。runtime 内部把 `::runtime::api::` 外部引用改为 `crate::api::`。迁移保持行为不变，编译通过即可。

**Tech Stack:** Rust workspace，无新依赖引入（`env_logger` 和 `base64` 在 CLI Cargo.toml 中已存在，迁入 runtime 时需同步）。

**分支：** `feature/47-thin-cli-phase9`，worktree `.worktrees/feature-47-thin-cli-phase9`

---

## 迁移清单

| Task | 源文件 (CLI) | 目标位置 (runtime) | 需新增依赖 | 反向引用数 |
|------|-------------|-------------------|-----------|-----------|
| 1 | `mcp_loader.rs` | `bootstrap/mcp_loader.rs` | 无 | 1 (`setup/tooling.rs`) |
| 2 | `prompt.rs` + `prompt/git_context.rs` + `prompt_tests.rs` | `prompt/prompt_build.rs` + `prompt/git_context.rs` + `prompt/prompt_build_tests.rs` | 无 | 4 (`setup/prompt_bundle.rs`×3, `prompt.rs`×1) |
| 3 | `reflection.rs` | `chat/reflection.rs` | 无 | 1 (`repl/turns.rs`) |
| 4 | `image.rs` + `image/clipboard.rs` | `image/mod.rs` + `image/clipboard.rs` | `base64 = "0.22"` | ~15 (TUI + REPL) |
| 5 | `logging_setup.rs` | `bootstrap/logging_setup.rs` | `env_logger = "0.11"` | 4 (main + setup + sessions + processing) |
| 6 | 编译验证 + 文档 + 提交 | — | — | — |

**执行顺序：** Task 1 → 2 → 3 可并行（互不依赖）；Task 4、5 也可并行；Task 6 在全部完成后执行。

---

## Task 1: 迁移 mcp_loader.rs → runtime

**Files:**
- Move: `apps/cli/src/mcp_loader.rs` → `crates/runtime/src/bootstrap/mcp_loader.rs`
- Modify: `crates/runtime/src/bootstrap/mod.rs`
- Modify: `apps/cli/src/main.rs`
- Modify: `apps/cli/src/run_orchestration/setup/tooling.rs`

- [x] **Step 1: 复制 mcp_loader.rs 到 runtime/bootstrap/**

```bash
cp apps/cli/src/mcp_loader.rs crates/runtime/src/bootstrap/mcp_loader.rs
```

- [x] **Step 2: 修改 runtime 侧 mcp_loader.rs 的 import 路径**

`::runtime::api::core::` → `crate::api::core::`，共 4 处：

```rust
// crates/runtime/src/bootstrap/mcp_loader.rs
use crate::api::core::config::paths;
use crate::api::core::mcp::McpServerConfig;
use crate::api::core::mcp_manager::McpConnectionManager;
use crate::api::core::tool::ToolRegistry;
```

测试中同理：

```rust
use crate::api::core::mcp::McpServerConfig;
```

- [x] **Step 3: 在 runtime bootstrap/mod.rs 注册模块并 re-export**

在 `crates/runtime/src/bootstrap/mod.rs` 顶部添加 `pub mod mcp_loader;`，在 re-export 区添加：

```rust
pub use mcp_loader::{load_mcp_manager, parse_mcp_servers_config, spawn_mcp_connect};
```

- [x] **Step 4: 删除 CLI 侧 mcp_loader.rs 并从 main.rs 移除 mod 声明**

```bash
rm apps/cli/src/mcp_loader.rs
```

从 `apps/cli/src/main.rs` 移除 `mod mcp_loader;`。

- [x] **Step 5: 更新 CLI 引用**

`apps/cli/src/run_orchestration/setup/tooling.rs`:

```rust
// 旧：use crate::mcp_loader::spawn_mcp_connect;
// 新：
use ::runtime::api::bootstrap::spawn_mcp_connect;
```

- [x] **Step 6: 验证编译**

```bash
cargo build -p runtime && cargo build -p cli
```

Expected: 编译成功。

---

## Task 2: 迁移 prompt.rs + prompt/ → runtime

**Files:**
- Move: `apps/cli/src/prompt.rs` → `crates/runtime/src/prompt/prompt_build.rs`
- Move: `apps/cli/src/prompt/git_context.rs` → `crates/runtime/src/prompt/git_context.rs`
- Move: `apps/cli/src/prompt_tests.rs` → `crates/runtime/src/prompt/prompt_build_tests.rs`
- Create: `crates/runtime/src/prompt/mod.rs`
- Modify: `crates/runtime/src/lib.rs`
- Modify: `apps/cli/src/main.rs`
- Modify: `apps/cli/src/run_orchestration/setup/prompt_bundle.rs`
- Modify: `apps/cli/src/run_orchestration/prompt.rs`

注意：runtime 已有 `crates/prompt/` crate（Guidance），但 `crates/runtime/src/prompt/` 目前只是 re-export。本 Task 在 runtime 内部新建 `prompt/` 子目录放 prompt 构建逻辑，不与 `crates/prompt/` crate 冲突。

- [x] **Step 1: 创建 runtime prompt 目录并复制文件**

```bash
mkdir -p crates/runtime/src/prompt
cp apps/cli/src/prompt.rs crates/runtime/src/prompt/prompt_build.rs
cp apps/cli/src/prompt/git_context.rs crates/runtime/src/prompt/git_context.rs
cp apps/cli/src/prompt_tests.rs crates/runtime/src/prompt/prompt_build_tests.rs
```

- [x] **Step 2: 修改 prompt_build.rs 的 import 路径**

```rust
// crates/runtime/src/prompt/prompt_build.rs
use crate::api::core::config::{paths, MemoryConfig};
use crate::api::core::hook::HookRunner;
use crate::api::core::memory::{
    memory_base_dir, project_hash_from_path, MemoryEntry, MemoryStore,
};

mod git_context;
use git_context::{collect_git_context, is_git_repo};
```

函数体内所有 `::runtime::api::` 改为 `crate::api::`：

```rust
// load_agents_md 中的 security 引用
let warnings = crate::api::core::security::scan_content("AGENTS.md", &agents_md);
if let Some(prefix) = crate::api::core::security::format_warnings(&warnings) {
```

测试模块的路径引用修改：

```rust
#[cfg(test)]
#[path = "prompt_build_tests.rs"]
mod prompt_build_tests;
```

- [x] **Step 3: 修改 prompt_build_tests.rs 的 import 路径**

所有 `::runtime::api::` → `crate::api::`。

- [x] **Step 4: 创建 prompt/mod.rs**

```rust
// crates/runtime/src/prompt/mod.rs
mod git_context;
mod prompt_build;

pub use prompt_build::{
    build_system_prompt_parts, collect_memory_context, current_date, load_agents_md,
    PromptContext, SystemPromptParts,
};
```

注意：`prompt_build.rs` 内部已有 `mod git_context;` 声明，需要改为从 mod.rs 统一管理。调整方式：

- `prompt_build.rs` 中移除 `mod git_context;`，改为 `use crate::prompt::git_context::{...};` 或 `use super::git_context::{...};`
- `mod.rs` 中声明 `pub(crate) mod git_context;` 和 `mod prompt_build;`

最终 `prompt_build.rs` 开头改为：

```rust
use crate::api::core::config::{paths, MemoryConfig};
use crate::api::core::hook::HookRunner;
use crate::api::core::memory::{
    memory_base_dir, project_hash_from_path, MemoryEntry, MemoryStore,
};
use super::git_context::{collect_git_context, is_git_repo};
```

- [x] **Step 5: 在 runtime/lib.rs 注册 prompt 模块**

`crates/runtime/src/lib.rs` 添加 `pub mod prompt;`。

注意：这会与已有的 `pub use prompt;`（re-export `crates/prompt` crate）冲突。需要改为不同的模块名。建议用 `prompt_build` 作为 runtime 内部模块名：

最终方案：在 `crates/runtime/src/` 下创建 `prompt_build/` 目录而非 `prompt/`，避免与 `pub use prompt;` 冲突。

```text
crates/runtime/src/prompt_build/
├── mod.rs
├── prompt_build.rs      # 原 cli/prompt.rs
├── git_context.rs       # 原 cli/prompt/git_context.rs
└── prompt_build_tests.rs # 原 cli/prompt_tests.rs
```

`crates/runtime/src/lib.rs` 添加 `pub mod prompt_build;`。

`prompt_build/mod.rs`:

```rust
pub(crate) mod git_context;
mod prompt_build;

pub use prompt_build::{
    build_system_prompt_parts, collect_memory_context, current_date, load_agents_md,
    PromptContext, SystemPromptParts,
};
```

`prompt_build/prompt_build.rs`:

```rust
use super::git_context::{collect_git_context, is_git_repo};
```

移除内部的 `mod git_context;`。

- [x] **Step 6: 在 runtime/api.rs 添加 re-export**

```rust
// crates/runtime/src/api.rs 末尾追加
pub use crate::prompt_build;
```

- [x] **Step 7: 删除 CLI 侧文件并更新 main.rs**

```bash
rm apps/cli/src/prompt.rs apps/cli/src/prompt/git_context.rs apps/cli/src/prompt_tests.rs
rmdir apps/cli/src/prompt
```

从 `apps/cli/src/main.rs` 移除 `mod prompt;`。

- [x] **Step 8: 更新 CLI 引用**

`apps/cli/src/run_orchestration/setup/prompt_bundle.rs`:

```rust
// 旧：use crate::prompt::{build_system_prompt_parts, PromptContext};
// 新：
use ::runtime::api::prompt_build::{build_system_prompt_parts, PromptContext};

// 旧：prompt_parts: crate::prompt::SystemPromptParts,
// 新：
prompt_parts: ::runtime::api::prompt_build::SystemPromptParts,
```

测试中：

```rust
// 旧：use crate::prompt::SystemPromptParts;
// 新：
use ::runtime::api::prompt_build::SystemPromptParts;
```

`apps/cli/src/run_orchestration/prompt.rs`:

```rust
// 旧：prompt_parts: crate::prompt::SystemPromptParts,
// 新：
prompt_parts: ::runtime::api::prompt_build::SystemPromptParts,
```

- [x] **Step 9: 验证编译**

```bash
cargo build -p runtime && cargo build -p cli
```

---

## Task 3: 迁移 reflection.rs → runtime

**Files:**
- Move: `apps/cli/src/reflection.rs` → `crates/runtime/src/chat/reflection.rs`
- Modify: `crates/runtime/src/chat/mod.rs`
- Modify: `apps/cli/src/main.rs`
- Modify: `apps/cli/src/repl/turns.rs`

- [x] **Step 1: 复制文件**

```bash
cp apps/cli/src/reflection.rs crates/runtime/src/chat/reflection.rs
```

- [x] **Step 2: 修改 runtime 侧 import 路径**

所有 `::runtime::api::` → `crate::api::`：

```rust
// crates/runtime/src/chat/reflection.rs
use crate::api::core::memory::MemoryStore;
use crate::api::core::reflection::ReflectionEngine;
```

函数签名和函数体中：

```rust
use crate::api::core::config::MemoryConfig;
use crate::api::core::message::Message;
use crate::api::provider::client::LlmClient;
use crate::api::core::memory::memory_base_dir;
use crate::api::core::memory::project_hash_from_path;
use crate::api::core::memory::MemoryLayer;
use crate::api::provider::types::SystemBlock;
use crate::api::provider::StreamHandler;
use crate::api::core::message::Role;
use crate::api::core::reflection::ReflectionOutput;
```

测试中同理改为 `crate::api::`。

- [x] **Step 3: 在 chat/mod.rs 注册模块**

读取 `crates/runtime/src/chat/mod.rs` 并添加 `pub mod reflection;`。

- [x] **Step 4: 删除 CLI 侧并更新 main.rs**

```bash
rm apps/cli/src/reflection.rs
```

从 `apps/cli/src/main.rs` 移除 `mod reflection;`。

- [x] **Step 5: 更新 CLI 引用**

`apps/cli/src/repl/turns.rs`:

```rust
// 旧：crate::reflection::run_reflection(...)
// 新：
::runtime::api::chat::reflection::run_reflection(...)
```

- [x] **Step 6: 验证编译**

```bash
cargo build -p runtime && cargo build -p cli
```

---

## Task 4: 迁移 image.rs + image/ → runtime

**Files:**
- Move: `apps/cli/src/image.rs` → `crates/runtime/src/image/mod.rs`
- Move: `apps/cli/src/image/clipboard.rs` → `crates/runtime/src/image/clipboard.rs`
- Modify: `crates/runtime/Cargo.toml`（添加 `base64`）
- Modify: `crates/runtime/src/lib.rs`
- Modify: `apps/cli/src/main.rs`
- Modify: `apps/cli/src/tui/app/run_loop.rs`
- Modify: `apps/cli/src/tui/app/slash.rs`
- Modify: `apps/cli/src/tui/app/cmd_exec.rs`
- Modify: `apps/cli/src/tui/app/update.rs`
- Modify: `apps/cli/src/tui/app/event.rs`
- Modify: `apps/cli/src/tui/app/paste_handler.rs`
- Modify: `apps/cli/src/tui/app/mod.rs`
- Modify: `apps/cli/src/tui/app/input_handler.rs`
- Modify: `apps/cli/src/repl/image_input.rs`
- Modify: `apps/cli/src/repl/commands.rs`
- Modify: `apps/cli/src/repl/mod.rs`
- Modify: `apps/cli/src/repl/input.rs`

- [x] **Step 1: 添加 base64 依赖到 runtime**

在 `crates/runtime/Cargo.toml` 的 `[dependencies]` 中添加：

```toml
base64 = "0.22"
```

- [x] **Step 2: 创建 runtime/image 目录并复制文件**

```bash
mkdir -p crates/runtime/src/image
cp apps/cli/src/image.rs crates/runtime/src/image/mod.rs
cp apps/cli/src/image/clipboard.rs crates/runtime/src/image/clipboard.rs
```

- [x] **Step 3: 修复 Linux IMAGE_MAX_HEIGHT 未定义问题**

在 `crates/runtime/src/image/mod.rs` 常量区添加：

```rust
pub const IMAGE_MAX_HEIGHT: u32 = 2000;
```

- [x] **Step 4: 在 runtime/lib.rs 注册 image 模块**

```rust
pub mod image;
```

- [x] **Step 5: 在 runtime/api.rs 添加 re-export**

```rust
pub use crate::image;
```

- [x] **Step 6: 删除 CLI 侧文件并更新 main.rs**

```bash
rm apps/cli/src/image.rs apps/cli/src/image/clipboard.rs
rmdir apps/cli/src/image
```

从 `apps/cli/src/main.rs` 移除 `mod image;`。

- [x] **Step 7: 批量更新 CLI 引用**

所有 `crate::image::` → `::runtime::api::image::`，涉及文件列表：

- `tui/app/run_loop.rs`：2 处
- `tui/app/slash.rs`：1 处
- `tui/app/cmd_exec.rs`：2 处
- `tui/app/update.rs`：1 处
- `tui/app/event.rs`：1 处（类型引用）
- `tui/app/paste_handler.rs`：3 处
- `tui/app/mod.rs`：1 处（类型引用）
- `tui/app/input_handler.rs`：1 处
- `repl/image_input.rs`：1 处 use + 内部引用
- `repl/commands.rs`：1 处 use + 1 处调用
- `repl/mod.rs`：1 处类型引用
- `repl/input.rs`：1 处 use

- [x] **Step 8: 验证编译**

```bash
cargo build -p runtime && cargo build -p cli
```

---

## Task 5: 迁移 logging_setup.rs → runtime

**Files:**
- Move: `apps/cli/src/logging_setup.rs` → `crates/runtime/src/bootstrap/logging_setup.rs`
- Modify: `crates/runtime/Cargo.toml`（添加 `env_logger`）
- Modify: `crates/runtime/src/bootstrap/mod.rs`
- Modify: `apps/cli/src/main.rs`
- Modify: `apps/cli/src/sessions_command.rs`
- Modify: `apps/cli/src/run_orchestration/setup.rs`
- Modify: `apps/cli/src/tui/app/processing.rs`

- [x] **Step 1: 添加 env_logger 依赖到 runtime**

在 `crates/runtime/Cargo.toml` 的 `[dependencies]` 中添加：

```toml
env_logger = "0.11"
```

- [x] **Step 2: 复制文件到 runtime/bootstrap/**

```bash
cp apps/cli/src/logging_setup.rs crates/runtime/src/bootstrap/logging_setup.rs
```

- [x] **Step 3: 修改 runtime 侧 import 路径**

```rust
// crates/runtime/src/bootstrap/logging_setup.rs
use crate::api::core::logging::{self, LogFile};
```

函数签名中：

```rust
pub fn init_logging(logging_config: &crate::api::core::config::LoggingConfig) {
```

- [x] **Step 4: 将 pub(crate) 改为 pub**

原 CLI 中函数是 `pub(crate)`，迁入 runtime 后需要改为 `pub` 以便 CLI 调用：

```rust
pub fn set_session_id(id: String) { ... }
pub fn set_current_turn(turn: usize) { ... }
pub fn init_logging(...) { ... }
pub fn init_panic_hook() { ... }
```

- [x] **Step 5: 在 bootstrap/mod.rs 注册模块并 re-export**

```rust
pub mod logging_setup;
pub use logging_setup::{init_logging, init_panic_hook, set_current_turn, set_session_id};
```

- [x] **Step 6: 删除 CLI 侧并更新 main.rs**

```bash
rm apps/cli/src/logging_setup.rs
```

从 `apps/cli/src/main.rs` 移除 `mod logging_setup;`。

- [x] **Step 7: 更新 CLI 引用**

`apps/cli/src/main.rs`：

```rust
// 旧：logging_setup::init_panic_hook();
// 新：
::runtime::api::bootstrap::init_panic_hook();
```

`apps/cli/src/sessions_command.rs`：

```rust
// 旧：use crate::logging_setup::set_session_id;
// 新：
use ::runtime::api::bootstrap::set_session_id;
```

`apps/cli/src/run_orchestration/setup.rs`：

```rust
// 旧：use crate::logging_setup::init_logging;
// 新：
use ::runtime::api::bootstrap::init_logging;

// 旧：crate::logging_setup::set_session_id(session_id.clone());
// 新：
::runtime::api::bootstrap::set_session_id(session_id.clone());
```

`apps/cli/src/tui/app/processing.rs`：

```rust
// 旧：crate::logging_setup::set_current_turn(turn);
// 新：
::runtime::api::bootstrap::set_current_turn(turn);
```

- [x] **Step 8: 验证编译**

```bash
cargo build -p runtime && cargo build -p cli
```

---

## Task 6: 完整编译验证 + 更新文档 + 提交

- [x] **Step 1: 完整 workspace 编译**

```bash
cargo build
cargo clippy --workspace -- -D warnings
```

- [x] **Step 2: 运行 runtime 和 cli 测试**

```bash
cargo test -p runtime
cargo test -p cli
```

- [x] **Step 3: 更新 docs/feature/active.md #47 条目**

在 #47 "当前推进" 段落末尾追加：

> CLI 侧非 UI 模块已完成拆分：`reflection`、`mcp_loader`、`prompt`（prompt_build）、`image`、`logging_setup` 已迁入 `crates/runtime`，CLI 只保留纯 UI/入口层（TUI、REPL、render、CLI 参数解析、run_orchestration 薄壳）。后续从 `core` 继续拆分 support domain。

- [x] **Step 4: 更新 docs/feature/specs/047-ddd-redesign.md**

在 checkpoint 进展部分追加 P9 拆分记录。

- [x] **Step 5: 提交**

```bash
git add -A
git commit -m "refactor: 将 CLI 非 UI 模块迁入 runtime (refs #47)

- mcp_loader → runtime::bootstrap::mcp_loader
- prompt/git_context → runtime::prompt_build
- reflection → runtime::chat::reflection
- image/clipboard → runtime::image
- logging_setup → runtime::bootstrap::logging_setup

CLI 侧只保留纯 UI/入口层：TUI、REPL、render、CLI 参数解析、run_orchestration 薄壳。

Co-Authored-By: Aemeath (Zhipu/glm-5.1) <github:rushsinging/aemeath>"
```

- [x] **Step 6: 合并回 main 并验证**

```bash
git checkout main
git merge --no-ff feature/47-thin-cli-phase9
cargo build
cargo test -p runtime
cargo test -p cli
```
