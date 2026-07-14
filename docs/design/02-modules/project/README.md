# Project / Workspace（支撑域）

> 层级：02-modules / project（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#791（S2）
> 本模块拥有 worktree 工作区上下文的单一可变状态源、栈式帧管理与隔离范式。通过 `WorkspacePort` 向 Runtime 和 Context Management 供给工作区位置与 git 上下文。

## 1. 模块定位

Project / Workspace BC 管理 Agent 执行时的"在哪里工作"——当前工作目录、worktree 进出、git 上下文供给。它是文件系统路径与 git 状态的唯一真相源，所有 Tool 的路径解析都经此模块。

| 概念 | 回答 |
|---|---|
| **Workspace** | 当前工作区上下文是什么 |
| **WorkspaceFrame** | 上下文栈的一帧（进入 / 退出 worktree） |
| **fork** | 子 agent 如何继承工作区上下文 |
| **WorkspacePort** | 外部如何读取和变更工作区 |
| **git 上下文供给** | Context Management 如何获取 git 信息注入 |

## 2. 核心决策

1. **Workspace 是聚合根 + 单一可变状态源**：全进程只有一个 `WorkspaceService` 实例持有 `Mutex<WorkspaceState>`，所有路径解析和 worktree 操作经此唯一入口。**NEVER** 存在第二个可变 workspace 状态源。
2. **Frame 栈模型**：worktree 进入压栈、退出弹栈，栈帧记录上一层的 `path_base` 与 `workspace_root`。嵌套 worktree **NEVER** 允许——进入前栈必须为空（残栈自愈除外）。
3. **纯函数转换规则**：状态转换逻辑（enter / exit / switch_to / set_path_base / set_workspace_root）是纯函数，接收 `&mut WorkspaceState` + git 端口，无隐藏副作用。便于测试和推理。
4. **fork 隔离范式**：子 agent 从父 agent 当前快照派生独立实例——继承 `workspace_root` + `path_base` + `initial_cwd`，空栈，独立锁，共享 git 端口。子 agent workspace 能力 ≤ 父 agent。
5. **WorkspacePort 统一出站端口**：外部经 `WorkspaceRead` + `WorkspaceControl` + `WorkspacePersist` 三 trait 访问工作区。三 trait 可合并为单一 `WorkspacePort` super-trait，也可保持分离供不同消费方按需依赖。
6. **git 上下文经端口供给**：Context Management 经 `WorkspaceRead` 读取当前分支、路径等信息，注入 Context Window。Workspace BC 不自行注入——它只提供数据源。

## 3. 模块内部结构

```text
project/
├── workspace/              # Workspace 聚合根
│   ├── state.rs            # WorkspaceState + 纯函数转换规则
│   ├── frame.rs            # WorkspaceFrame 栈帧
│   ├── service.rs          # WorkspaceService（单一可变状态源 + fork）
│   └── error.rs            # WorkspaceError
├── git/                    # git 出站端口与适配器
│   ├── port.rs             # GitWorktreeOps trait
│   └── cli.rs              # GitCli 生产适配器（spawn git CLI）
├── port/                   # WorkspaceRead / Control / Persist trait
└── api/                    # BC 对外 facade
```

目录表达业务能力而非 `contract / business / gateway / utils` 等横向技术层。Composition Root 是唯一生产装配入口。

## 4. 对外端口

| 端口 | 消费方 | 职责 |
|---|---|---|
| `WorkspaceRead` | 所有 Tool / Context Management | 读取当前 workspace_root、path_base、resolve 路径、in_worktree、initial_cwd |
| `WorkspaceControl` | Agent Runtime（EnterWorktree / ExitWorktree 工具） | 变更 path_base、workspace_root、enter / exit worktree |
| `WorkspacePersist` | Context Management | snapshot / restore（Session 落盘与恢复） |
| `GitWorktreeOps` | Workspace 内部 | git 命令执行（worktree add、show-toplevel 等）——这是 Workspace 的出站端口，非对外 |

> `WorkspaceRead`、`WorkspaceControl`、`WorkspacePersist` 可由 `WorkspaceService` 同时实现。消费方按需依赖对应 trait，降低耦合。

## 5. 与其他 BC 的关系

### Agent Runtime

Runtime 经 `WorkspaceControl` 执行 worktree enter / exit（由 EnterWorktree / ExitWorktree 工具触发）。Runtime 不直接管理路径状态——经端口委托给 Workspace BC。

### Context Management

Context Management 经 `WorkspacePersist` 在 Session 落盘时收集 `PersistedWorkspaceContext`，恢复时分发。经 `WorkspaceRead` 读取当前工作区信息注入 Context Window。

### Tool

所有文件系统 Tool 经 `WorkspaceRead::resolve()` 将相对路径解析为绝对路径。Tool **NEVER** 自行拼接 cwd。

### Config

Config 通过只读 ConfigSnapshot 提供 Project 相关配置。Workspace BC 不绕过快照读取裸配置。

## 6. 设计边界

- **NEVER** 存在第二个可变 workspace 状态源。
- **NEVER** 允许嵌套 worktree（栈非空时禁止 enter）。
- **NEVER** 让 Tool 自行拼接工作目录路径。
- **NEVER** 将 Workspace 内部 `WorkspaceState` 直接暴露给外部。
- **MUST** 所有状态转换经纯函数规则。
- **MUST** worktree enter 前校验同源（同一 git 仓库）。
- **MUST** 子 agent workspace 能力 ≤ 父 agent（fork 只继承位置，不继承栈）。

## 7. 文档导航

| 文档 | 内容 |
|---|---|
| [01-domain-model.md](01-domain-model.md) | Workspace 聚合根、Frame 栈、状态转换规则、fork、错误模型 |
| [02-ports-and-adapters.md](02-ports-and-adapters.md) | WorkspaceRead / Control / Persist、GitWorktreeOps、持久化 DTO、git 上下文供给 |

## 8. 相关文档

- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md) §7 Project / Workspace
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md) §4
- Runtime 领域模型：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)
- Context Management Session：[../context-management/01-session.md](../context-management/01-session.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Workspace 聚合根、Frame 栈、fork、三端口、git 上下文供给 | #791 |
