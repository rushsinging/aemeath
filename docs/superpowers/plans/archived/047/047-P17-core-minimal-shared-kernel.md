# Feature 47 P17: share/core 瘦身为最小共享内核

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** `agent/share`（当前映射为 `core`）从 63 个文件（~10404 行）瘦身为真正的最小共享内核——只保留 value object、error types、基础 trait、协议无关 DTO。不属于核心共享的类型下沉到各自的 supporting domain 或 runtime domain 层。

## 当前状态

`agent/share/src/` 共 63 个文件，~10404 行（2026-05-27 审计）。目标 domain 的 scaffold 已存在，部分迁移已启动（facade/复制阶段）：

| 模块 | 行数（估） | 应属于 | 迁移状态 |
|------|----------|--------|---------|
| `tool.rs` | ~330 | `agent/tools/` | ❌ 未开始。`agent/tools` 已有工具实现，但 `Tool`/`ToolContext`/`ToolResult`/`ToolRegistry` 仍以 share 为 canonical source |
| `worktree_ops.rs` | ~200 | `agent/project/` | 🔶 部分完成。`agent/project/src/worktree.rs` 已有相同内容，但 share 原文件仍存在，`agent/tools/src/worktree.rs` 仍引用 `share::worktree_ops` |
| `provider.rs` | ~30 | `agent/provider/` | 🔶 部分完成。`agent/provider` domain 已完整存在，但 `ApiDriverKind` 枚举仍定义在 share，provider 通过 `pub use share::provider::ApiDriverKind` re-export |
| `skill_ops.rs` + `skill_ops_loader.rs` | ~460 | `agent/prompt/` | 🔶 部分完成。`agent/prompt/src/skill/loader.rs` 已有 loader，但 `skill/mod.rs` 明确说核心实现在 share，仅做 facade re-export |
| `config/manager/` | ~780 | runtime bootstrap 或独立 config crate | ❌ 未开始。加载/合并/持久化逻辑仍在 share |
| `config/models/` | ~1030 | share（DTO） | ✅ 保留。config DTO struct 属于共享类型 |
| `config/*.rs`（顶层） | ~880 | share（DTO）/ 各 domain | ❌ 未评估。部分 config 类型可能属于各自 domain |
| `memory/` | ~1240 | runtime domain 或独立 memory crate | ❌ 未开始 |
| `task/` + `task_ops.rs` | ~1900 | runtime `business/task/` | ❌ 未开始。需注意 task 被 `agent/tools` 和 `agent/project` 跨 crate 引用 |
| `message/integrity.rs` | ~100 | runtime `business/chat/` | ❌ 未开始 |
| `message/types.rs` | ~200 | share（核心 DTO） | ✅ 保留。`Message`/`Role`/`ContentBlock` 被广泛使用 |
| `token_estimation.rs` | ~370 | runtime `business/compact/` | ❌ 未开始 |
| `error.rs` | ~100 | share | ✅ 保留 |
| `session_types.rs` | ~80 | share | ✅ 保留 |
| `string_idx/` | ~640 | share（纯工具） | ✅ 保留 |
| `lib.rs` | ~20 | share | ✅ 保留 |

## 原则

- `core`（share）= **所有 crate 都需要的基础类型**
- 只有被 3+ 个 crate 引用的类型才允许留在 core
- 被单一 crate 使用的类型应下沉到该 crate
- config 解析逻辑不应在 core（core 只含 config DTO/struct，不含加载/合并逻辑）

## 步骤

- [ ] **1. 审计 share 中每个模块的实际引用者**
  - `grep -rn 'share::tool\|core::tool' agent/*/src/` 统计哪些 crate 引用
  - 对每个子模块做同样统计
  - 输出引用者数量清单，更新上表

- [ ] **2. 迁移 `tool.rs` → `agent/tools/`** ❌
  - `share::tool::Tool` / `ToolCall` / `ToolResult` / `ToolRegistry` / `ToolContext` / `ImageData` / `WorkingContext` → `tools` crate
  - 如果被 3+ crate 引用，在 core 只留 type alias re-export
  - 更新 `agent/tools/src/lib.rs` 不再依赖 `share::tool`
  - 更新所有 `use share::tool::*` 引用为 `use tools::*`

- [ ] **3. 完成 `worktree_ops.rs` → `agent/project/` 迁移** 🔶
  - `agent/project/src/worktree.rs` 已有内容，本步骤需：
    - 删除 `share/src/worktree_ops.rs`
    - 更新 `agent/tools/src/worktree.rs` 从 `share::worktree_ops` 改为引用 `project::worktree`
    - 更新所有其他引用者

- [ ] **4. 完成 `provider.rs` → `agent/provider/` 迁移** 🔶
  - `ApiDriverKind` 枚举从 `share/src/provider.rs` 移入 `agent/provider/src/api.rs`（作为 canonical definition）
  - 删除 `share/src/provider.rs`
  - 更新所有引用者从 `share::provider::ApiDriverKind` → `provider::ApiDriverKind`

- [ ] **5. 完成 `skill_ops.rs` + `skill_ops_loader.rs` → `agent/prompt/` 迁移** 🔶
  - `Skill` 结构体、`parse_skill`、`load_all_skills`、`load_all_skills_cached` 等从 share 移入 `agent/prompt/src/skill/`
  - 删除 `share/src/skill_ops.rs` 和 `share/src/skill_ops_loader.rs`
  - 更新 `agent/prompt/src/skill/mod.rs` 从 facade re-export 改为 canonical definition

- [ ] **6. 迁移 `config/manager/` 加载/合并/持久化逻辑** ❌
  - config DTO struct（`AppConfig`、`ModelsConfig` 等）留在 `share::config::models`
  - `manager/merge.rs`、`manager/persistence.rs`、`manager/mod.rs` → runtime `utils/bootstrap/` 或独立 config crate
  - 验证 share 中不残留加载/合并逻辑

- [ ] **7. 迁移 `memory/` → runtime `business/` 或独立 memory 模块** ❌
  - `MemoryStore`、`MemoryEntry`、`MemoryLayer`、相关操作 → `agent/runtime/src/business/memory/`
  - 注意：memory 被 reflection（runtime）和 memory tool（tools）使用，跨 crate 场景可能需要保留 MemoryEntry/MemoryStore DTO 在 share

- [ ] **8. 迁移 `task/` + `task_ops.rs` → runtime `business/task/`** ❌
  - `TaskStore`、`TaskStatus`、`TaskSnapshot`、task lifecycle → `agent/runtime/src/business/task/`
  - 注意：task 被 `agent/tools` 和 `agent/project` 跨 crate 引用，`TaskStatus` / `TaskSnapshot` 等 DTO 可能需要保留在 share，仅迁移业务逻辑和 `TaskStore` 操作

- [ ] **9. 迁移 `message/integrity.rs` → runtime `business/chat/`** ❌
  - Message 修复/清理逻辑不属于共享类型，应下沉到 runtime
  - `Message` / `Role` / `ContentBlock` / `constructors` / `query` 保留在 share（被广泛使用）

- [ ] **10. 迁移 `token_estimation.rs` → runtime `business/compact/`** ❌
  - `estimate_tokens`、`needs_compaction` 等 → `agent/runtime/src/business/compact/`
  - 更新 `business/compact/mod.rs` 中现存的 `pub use share::token_estimation::*`

- [ ] **11. 评估 `config/*.rs` 顶层文件归属** ❌
  - `hooks.rs`、`legacy.rs`、`logging.rs`、`memory.rs`、`paths.rs`、`permissions.rs`、`skills.rs`、`storage.rs`、`tools.rs`、`ui.rs`
  - 判断哪些是纯 DTO（留 share）、哪些含业务逻辑（移出）

- [ ] **12. 验证 core 瘦身后只含最小共享类型**
  - 预期保留：`error.rs`、`lib.rs`、`session_types.rs`、`message/types.rs`、`message/constructors.rs`、`message/query.rs`、`string_idx/`、config DTO struct（`models/`）
  - 预期迁出：上面标记的 ❌ 和 🔶 项
  - `cargo build` + `cargo test` 全部通过
  - 每个 crate 依赖 core 的理由都合理
  - share 文件数从 63 → 预期 ~25-30

## 修改记录

- **2026-05-27**：基于 P15 后实际代码状态更新。审计 share 63 文件、~10404 行；部分步骤已启动（worktree_ops/project、provider domain、skill_ops loader）标注为 🔶 部分完成；tool / memory / task / config-manager / integrity / token_estimation 标注为 ❌ 未开始；新增步骤 11（config 顶层文件评估）和步骤 12（合并原验证步骤并细化预期保留清单）；各步骤补充跨 crate 引用注意事项。
