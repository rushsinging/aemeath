# Feature 47 P17: share/core 瘦身为最小共享内核

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** `agent/share`（当前映射为 `core`）从 67 个文件瘦身为真正的最小共享内核——只保留 value object、error types、基础 trait、协议无关 DTO。不属于核心共享的类型下沉到各自的 supporting domain 或 runtime domain 层。

## 当前状态

`agent/share/src/` 有 67 个文件，包含：
- ✅ 应留在 core 的：`error.rs`、`lib.rs`、`session_types.rs`（基础类型）
- ❌ 应在 supporting domain 的：
  - `tool.rs`（392 行）→ `agent/tools/`
  - `worktree_ops.rs`（297 行）→ `agent/project/`
  - `provider.rs`（111 行）→ `agent/provider/`
  - `skill_ops.rs` + `skill_ops_loader.rs`（255+205 行）→ `agent/prompt/`
  - `config/` 整个目录（~2000 行）→ 应在独立 config 模块或各 domain 内
  - `memory/` 整个目录（~900 行）→ 应在独立 memory domain 或 runtime domain 层
- ❌ 应在 runtime domain 层的：
  - `task/` 整个目录（~1500 行）→ runtime `domain/task/`
  - `message/` 整个目录（~800 行）→ runtime `domain/chat/` 或独立 message 模块
  - `token_estimation.rs`（371 行）→ runtime `domain/compact/`
  - `string_idx/` 整个目录（~400 行）→ 可能留在 core（纯字符串工具）

## 原则

- `core`（share）= **所有 crate 都需要的基础类型**
- 只有被 3+ 个 crate 引用的类型才允许留在 core
- 被单一 crate 使用的类型应下沉到该 crate
- config 解析逻辑不应在 core（core 只含 config DTO/struct，不含加载/合并逻辑）

## 步骤

- [ ] **1. 审计 share 中每个模块的实际引用者**
  - `grep -rn 'share::tool\|core::tool' agent/*/src/` 统计哪些 crate 引用
  - 对每个子模块做同样统计
  - 输出引用者数量清单

- [ ] **2. 迁移 `tool.rs` → `agent/tools/`**
  - `share::tool::Tool` / `ToolCall` / `ToolResult` / `ToolRegistry` → `tools` crate
  - 如果被 3+ crate 引用，在 core 只留 type alias re-export

- [ ] **3. 迁移 `worktree_ops.rs` → `agent/project/`**
  - Worktree 操作只被 runtime 和 tools 使用，不属于 core

- [ ] **4. 迁移 `provider.rs` → `agent/provider/`**
  - Provider trait 和基础类型应属于 provider domain

- [ ] **5. 迁移 `skill_ops.rs` + `skill_ops_loader.rs` → `agent/prompt/`**
  - Skill 操作是 prompt domain 的职责

- [ ] **6. 迁移 `config/` 中加载/合并逻辑**
  - config DTO struct（`AppConfig`、`ModelsConfig` 等）可留在 core
  - 加载/合并/持久化逻辑（`manager/merge.rs`、`manager/persistence.rs`、`models/resolve.rs`）移到 runtime `infrastructure/bootstrap/` 或独立 config crate

- [ ] **7. 迁移 `memory/` → 独立模块或 runtime domain**
  - Memory 操作是 runtime 编排的职责，不是所有 crate 都需要

- [ ] **8. 迁移 `task/` → runtime `domain/task/`**
  - Task 状态机只在 runtime 内使用

- [ ] **9. 迁移 `message/` 类型**
  - `Message` / `Role` / `ContentBlock` 被广泛使用，可能需留在 core
  - 但 `integrity.rs`（修复/清理逻辑）→ runtime `domain/chat/`
  - `tests.rs` → 随主体移动

- [ ] **10. 迁移 `token_estimation.rs` → runtime `domain/compact/`**

- [ ] **11. 验证 core 瘦身后只含最小共享类型**
  - 预期保留：`error.rs`、`lib.rs`、`session_types.rs`、`message/types.rs`、`string_idx/`、config DTO struct
  - `cargo build` + `cargo test` 全部通过
  - 每个 crate 依赖 core 的理由都合理
