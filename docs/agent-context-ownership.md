# Agent Context 所有权重构终态

> 完整设计：[`docs/superpowers/specs/2026-06-07-agent-context-ownership-redesign.md`](superpowers/specs/2026-06-07-agent-context-ownership-redesign.md)

## 问题

workspace 状态（`working_root`/`path_base`/`context_stack`）用 5 套类型表达，所有权弥散于 tools/project/runtime/session，导致：

1. **所有权不清**：多个 feature 都能重建同一组 workspace 字段，字段漂移、规则重复。
2. **撕裂读**：`working_root` 与 `path_base` 是两把独立 `Arc<Mutex>`，读者可能观察到中间态。
3. **子 agent 共享 bug**：子 agent `EnterWorktree` 会改到父 agent 的工作目录。
4. **六边形违规**：domain 层直接内联 `Command::new("git")`。

## 终态设计

**project 拥有 workspace 的类型与规则，runtime 仅持有实例生命周期。**

### 依赖图（铁律：无任何 feature 能依赖 runtime）

```
share   → ∅
project → share
tools   → share, project, storage
runtime → 全部 feature + share + sdk + logging
composition → runtime, tools, provider, project, sdk
```

### 类型归属

| 类型 | 所在 crate | 职责 |
|---|---|---|
| `WorkspaceService`（状态容器 + 转换规则） | `agent/features/project` | enter/exit worktree、git 校验，单一可变状态源 |
| `WorkspaceFrame`（运行期栈帧） | `agent/features/project`（内部） | 替代原 `WorkingContext`，不跨 crate |
| `PersistedWorkspaceContext` / `PersistedWorkspaceFrame` | `agent/shared`（`share`） | 纯 serde DTO，仅用于会话持久化，不进执行路径 |
| `WorkspaceContext`（session DTO） | `agent/shared`（`share`） | String 形式，保持旧 session 兼容 |

### 核心规则

- **类型 + 规则归 project**：enter/exit/git 校验规则在 `project/src/business/worktree.rs`，project 只依赖 share（最内层），符合 Clean 依赖规则。
- **实例生命周期归 runtime**：runtime 持有 `Arc<WorkspaceService>` 句柄，不定义 context。
- **tools 直接消费 project**：tools 已被允许依赖 project，无需绕道 runtime。
- **持久化 DTO 留 share**：跨持久化边界被 project/runtime/storage 引用。
- **git 进程调用不进 share**：`check-share-minimal-kernel.sh` 禁止 share 出现 `Command::new`，git adapter 全部落在 project。

### 子 agent 隔离

子 agent 不再经 `Arc` 克隆父 agent 的 workspace 字段；每个子 agent 拥有独立的 `WorkspaceService` 实例，`EnterWorktree` 不影响父 agent。

## 收益

- 消除 5 套类型重复，workspace 事实单一来源（`WorkspaceService` in project）。
- 消除撕裂读（单一 `Mutex` 保护整个 workspace 状态）。
- 子 agent 工作目录隔离。
- 六边形合规（domain 不直接调 git，经 port/adapter）。
