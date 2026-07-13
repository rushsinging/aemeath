# Project / Workspace（支撑域）

> 层级：02-modules / project（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#791（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本模块为每个 Main / Sub workspace context 提供各自的单一可变状态源、栈式帧管理与隔离范式。通过 `WorkspaceRead`、`WorkspaceControl`、`WorkspacePersist` 三个可分别消费的窄 trait 向 Tool 和 Context Management 提供工作区能力；Runtime 只编排相关 Tool 流程。

## 1. 模块定位

Project / Workspace BC 管理 Agent 执行时的"在哪里工作"——当前工作目录、worktree 进出、git 上下文供给。它是文件系统路径与 git 状态的唯一真相源，所有 Tool 的路径解析都经此模块。

| 概念 | 回答 |
|---|---|
| **Workspace** | 当前工作区上下文是什么 |
| **WorkspaceFrame** | 上下文栈的一帧（进入 / 退出 worktree） |
| **fork** | 子 agent 如何继承工作区上下文 |
| **WorkspaceRead / WorkspaceControl / WorkspacePersist** | 外部如何按最小能力读取、变更和持久化工作区 |
| **git 上下文供给** | Context Management 如何获取 git 信息注入 |

## 2. 核心决策

1. **Workspace 是聚合根 + context 内单一可变状态源**：每个 Main / Sub workspace context **MUST** 各有且仅有一个 `WorkspaceService` 持有 `Mutex<WorkspaceState>`，该 context 的路径解析和 worktree 操作经此唯一入口。同一 context 内 **NEVER** 复制第二份可变状态或缓存；fork 创建的是新的隔离 context。
2. **Frame 栈模型**：worktree 进入压栈、退出弹栈，栈帧记录上一层的 `path_base` 与 `workspace_root`。嵌套 worktree **NEVER** 允许——进入前栈必须为空（残栈自愈除外）。
3. **纯函数转换规则**：状态转换逻辑（enter / exit / switch_to / set_path_base / set_workspace_root）是纯函数，接收 `&mut WorkspaceState` + git 端口，无隐藏副作用。便于测试和推理。
4. **fork 隔离范式**：子 agent 从父 agent 当前快照派生独立实例——继承 `workspace_root` + `path_base` + `initial_cwd`，空栈，独立锁，共享 git 端口。子 agent workspace 能力 ≤ 父 agent。
5. **三个窄公开契约**：`WorkspaceRead`、`WorkspaceControl`、`WorkspacePersist` **MUST** 保持可分别消费，消费方 **MUST** 只依赖所需能力；`WorkspaceService` **MAY** 同时实现三者，三个契约 **NEVER** 被单一宽泛 super-trait 取代。
6. **git 上下文经端口供给**：Context Management 经 `WorkspaceRead` 读取当前分支（`current_branch`）、路径等信息，注入 Context Window。Workspace BC 不自行注入——它只提供数据源。
7. **Project-owned 生产 wiring**：Composition Root **MUST** 调用 `project::api::wire_production_workspace(cwd)` 选择生产 wiring，**NEVER** 直接命名或构造 Project 私有的 `GitCli` / `GitWorktreeOps`。factory 返回的 opaque handle 保留 Project-owned 隔离派生能力，并只向业务消费者分发三个窄 trait view。

## 3. 目标布局与契约真相

Project 的目标物理布局、三个公开 trait 的精确方法与类型、消费方映射及内部 `GitWorktreeOps` seam **MUST** 只以 [Workspace 端口与适配器](02-ports-and-adapters.md) 为真相源。本文 **NEVER** 复制物理树或 trait 签名。

## 4. 稳定公开能力概览

| 窄 trait | 能力概览 |
|---|---|
| `WorkspaceRead` | 读取当前 workspace_root、path_base、resolve 路径、in_worktree、current_branch、initial_cwd |
| `WorkspaceControl` | 变更 path_base、workspace_root、enter / exit worktree |
| `WorkspacePersist` | snapshot / restore（Session 落盘与恢复） |

`WorkspaceService` **MAY** 同时实现这三个 trait；production factory、opaque wiring handle、精确契约与内部 git 出站 seam 见 [02-ports-and-adapters.md](02-ports-and-adapters.md)。

## 5. 与其他 BC 的关系

### Agent Runtime

Runtime 编排 worktree 工具调用，Bash / EnterWorktree / ExitWorktree Tool 按最小能力消费 `WorkspaceControl`。Runtime 不直接管理路径状态。

### Context Management

Context Management 经 `WorkspacePersist` 在 Session 落盘时收集 `PersistedWorkspaceContext`，恢复时分发。经 `WorkspaceRead` 读取当前工作区信息注入 Context Window。

### Tool

文件系统 Tool 经 `WorkspaceRead::resolve()` 将相对路径解析为绝对路径；Bash / EnterWorktree / ExitWorktree Tool 按需消费 `WorkspaceControl`。只读文件 Tool **NEVER** 依赖 `WorkspaceControl`，所有 Tool **NEVER** 自行拼接 cwd。

### Config

Config 通过只读 ConfigSnapshot 提供 Project 相关配置。Workspace BC 不绕过快照读取裸配置。

## 6. 设计边界

- 同一 workspace context 内 **NEVER** 存在第二份可变 workspace 状态或缓存。
- **NEVER** 允许嵌套 worktree（栈非空时禁止 enter）。
- **NEVER** 让 Tool 自行拼接工作目录路径。
- **NEVER** 将 Workspace 内部 `WorkspaceState` 直接暴露给外部。
- **NEVER** 向业务消费者暴露 production wiring handle、`GitCli` 或 `GitWorktreeOps`；消费者只接收所需的窄 trait view。
- **MUST** 将 `wire_production_workspace` 的跨 crate 消费限制在 Composition Root。
- **MUST** 所有状态转换经纯函数规则。
- **MUST** worktree enter 前校验同源（同一 git 仓库）。
- **MUST** 子 agent workspace 能力 ≤ 父 agent（fork 只继承位置，不继承栈）。

## 7. 文档导航

| 文档 | 内容 |
|---|---|
| [01-domain-model.md](01-domain-model.md) | Workspace 聚合根、Frame 栈、状态转换规则、fork、错误模型 |
| [02-ports-and-adapters.md](02-ports-and-adapters.md) | 唯一物理布局与端口契约真相：三个窄 trait、production wiring、GitWorktreeOps、持久化 DTO、git 上下文供给 |

## 8. 相关文档

- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md) §7 Project / Workspace
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md) §4
- Runtime 领域模型：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)
- Context Management Session：[../context-management/01-session.md](../context-management/01-session.md)
- 系统架构：[../../01-system/04-system-architecture.md](../../01-system/04-system-architecture.md)
- 代码组织规范：[../../01-system/06-code-organization.md](../../01-system/06-code-organization.md)
- 迁移治理：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Workspace 聚合根、Frame 栈、fork、三端口、git 上下文供给 | #791 |
| 2026-07-14 | 将三个窄 trait 固定为可分别消费的稳定公开契约，删除重复物理树与宽泛 super-trait，以 Project-owned production factory 隔离私有 git adapter，并对齐 WorkspaceControl 的直接 Tool 消费者 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
