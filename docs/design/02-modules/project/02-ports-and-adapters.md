# Workspace 端口与适配器

> 层级：02-modules / project（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#791（S2）

## 1. 端口体系

Workspace BC 暴露三个对外 trait + 一个内部出站端口：

| 端口 | 方向 | 消费方 | 职责 |
|---|---|---|---|
| `WorkspaceRead` | 对外（入站方向） | 所有 Tool / Context Management | 只读访问当前工作区位置 |
| `WorkspaceControl` | 对外（入站方向） | Agent Runtime | 变更工作区（cd / enter / exit worktree） |
| `WorkspacePersist` | 对外（入站方向） | Context Management | 快照收集 / 恢复 |
| `GitWorktreeOps` | 内部出站 | WorkspaceService | git 命令执行 |

> 三个对外 trait 均由 `WorkspaceService` 实现。消费方按需依赖对应 trait，降低耦合。`WorkspaceRead` 被所有 Tool 依赖，但 Tool **NEVER** 依赖 `WorkspaceControl`。

## 2. WorkspaceRead

只读端口，提供当前工作区位置信息：

```rust
pub trait WorkspaceRead: Send + Sync {
    /// 当前工作根目录
    fn current_workspace_root(&self) -> PathBuf;
    /// 当前路径基准
    fn current_path_base(&self) -> PathBuf;
    /// 将相对路径解析为绝对路径
    fn resolve(&self, rel: &Path) -> PathBuf;
    /// 当前是否位于 linked git worktree
    fn in_worktree(&self) -> bool;
    /// 项目启动时的 cwd（worktree 切换时不变）
    fn initial_cwd(&self) -> PathBuf;
}
```

### 2.1 消费方

| 消费方 | 用途 |
|---|---|
| **文件 Tool**（Read / Write / Edit / Glob / Grep） | `resolve()` 将相对路径解析为绝对路径 |
| **Bash Tool** | `current_path_base()` 作为命令执行的 cwd |
| **Context Management** | `current_workspace_root()` / `initial_cwd()` 获取项目根路径 |
| **Memory BC** | `initial_cwd()` 确定项目级 memory 路径 |

### 2.2 路径解析语义

| 输入 | resolve 输出 |
|---|---|
| `/abs/path` | `/abs/path`（原样） |
| `relative/path` | `path_base.join("relative/path")` |

> **Decision**：`resolve` 不做存在性校验——它只做路径拼接。存在性由调用方（Tool）在操作时检查。这避免了 `resolve` 变成 I/O 操作。

## 3. WorkspaceControl

变更端口，提供工作区位置变更能力：

```rust
pub trait WorkspaceControl: Send + Sync {
    /// 更新 path_base（bash cd 用）
    fn set_path_base(&self, path: PathBuf) -> Result<(), WorkspaceError>;
    /// 更新 workspace_root + path_base（worktree enter/exit 用）
    fn set_workspace_root(&self, root: PathBuf, path: PathBuf) -> Result<(), WorkspaceError>;
    /// 切换到指定路径（不压栈，ExitWorktree{path} 用）
    fn switch_to(&self, path: PathBuf) -> Result<(), WorkspaceError>;
    /// 进入 worktree（压栈 + 切换）
    fn enter(&self, path: Option<PathBuf>, branch: Option<String>) -> Result<WorkspaceFrame, WorkspaceError>;
    /// 退出 worktree（弹栈 + 恢复）
    fn exit(&self) -> Result<WorkspaceFrame, WorkspaceError>;
}
```

### 3.1 消费方

| 消费方 | 方法 | 触发场景 |
|---|---|---|
| **Bash Tool** | `set_path_base` | `cd` 命令 |
| **EnterWorktree Tool** | `enter` | 用户 / agent 请求进入 worktree |
| **ExitWorktree Tool** | `exit` / `switch_to` | 用户 / agent 请求退出 worktree |

### 3.2 边界约束

- `WorkspaceControl` **NEVER** 被 `WorkspaceRead` 的消费方（文件 Tool）依赖。
- `enter` 和 `exit` 的返回值是 `WorkspaceFrame`，供 Tool 向用户报告切换结果。
- `switch_to` 不压栈——它直接切换到目标路径，供 `ExitWorktree { path }` 使用（退出到指定路径而非弹栈）。

## 4. WorkspacePersist

快照端口，提供 Session 落盘与恢复能力：

```rust
pub trait WorkspacePersist: Send + Sync {
    /// 生成可持久化快照
    fn snapshot(&self) -> PersistedWorkspaceContext;
    /// 从快照恢复
    fn restore(&self, dto: &PersistedWorkspaceContext) -> Result<(), WorkspaceError>;
}
```

### 4.1 持久化 DTO

| DTO | 字段 | 说明 |
|---|---|---|
| `PersistedWorkspaceContext` | `path_base: String` | 当前路径基准 |
| | `workspace_root: String` | 当前工作根 |
| | `context_stack: Vec<PersistedWorkspaceFrame>` | 栈快照 |
| `PersistedWorkspaceFrame` | `path_base: String` | 栈帧路径基准 |
| | `workspace_root: String` | 栈帧工作根 |

> DTO 定义在 `share::session_types`，属于 Session 快照的 Shared Kernel。路径序列化为 `String`（`PathBuf::display()`），反序列化时重建 `PathBuf`。

### 4.2 快照边界

- `snapshot` 收集当前 `path_base` + `workspace_root` + `stack` 全量快照。
- `restore` 全量替换内存状态，校验路径存在性（`RestoreInvalidPath`）。
- 快照内嵌 Session DTO 落盘，经 `WorkspacePersist` 端口收集，Workspace BC 不自行驱动持久化。

### 4.3 跨 BC 快照组装

```
Context Management              Workspace BC
     │                               │
     │  snapshot()                   │
     │ ────────────────────────────▶ │
     │                               │ 返回 PersistedWorkspaceContext
     │ ◀──────────────────────────── │
     │                               │
     │  嵌入 Session DTO             │
     │  写入磁盘                     │
     │                               │
     │  restore(dto)                 │
     │ ────────────────────────────▶ │
     │                               │ 校验路径 + 全量替换
     │ ◀──────────────────────────── │
     │  Ok / Err                     │
```

## 5. GitWorktreeOps（内部出站端口）

Workspace BC 的 git 出站端口，封装所有 git 命令执行：

```rust
pub trait GitWorktreeOps: Send + Sync {
    /// git rev-parse --git-common-dir
    fn git_common_dir(&self, path: &Path) -> Result<PathBuf, String>;
    /// git rev-parse --show-toplevel
    fn show_toplevel(&self, path: &Path) -> Result<PathBuf, String>;
    /// 当前路径是否位于 linked git worktree
    fn in_worktree(&self, path: &Path) -> bool;
    /// git worktree add
    fn worktree_add(&self, repo_root: &Path, path: &Path, branch: &str, base: &str) -> Result<(), String>;
    /// 当前分支名（detached HEAD 返回 None）
    fn current_branch(&self, path: &Path) -> Result<Option<String>, String>;
}
```

### 5.1 适配器

| 适配器 | 说明 |
|---|---|
| `GitCli` | 生产适配器，spawn `git` CLI 子进程 |
| `FakeGit` | 测试适配器，内存模拟，用于纯函数规则的单测 |

### 5.2 设计约束

- `GitWorktreeOps` 是 Workspace BC 的**内部出站端口**，不对外暴露。
- `WorkspaceService` 持有 `Arc<dyn GitWorktreeOps>`，`seed_isolated` 时共享。
- `GitCli` **MAY** spawn 子进程（project feature 可 spawn，shared 不可）。
- **NEVER** 在 `shared` 层 spawn 子进程——git CLI 调用只在 project feature 的 `GitCli` 适配器中。

## 6. git 上下文供给

### 6.1 数据流

Context Management 在构建 Context Window 时，经 `WorkspaceRead` 读取工作区信息注入：

| 数据 | 来源 | 注入位置 |
|---|---|---|
| 项目根路径 | `WorkspaceRead::initial_cwd()` | System Prompt / AGENTS.md 路径 |
| 当前工作目录 | `WorkspaceRead::current_path_base()` | System Prompt 上下文 |
| 工作根 | `WorkspaceRead::current_workspace_root()` | System Prompt 上下文 |
| 是否在 worktree | `WorkspaceRead::in_worktree()` | 上下文标记 |
| 当前分支 | `GitWorktreeOps::current_branch()` | 上下文标记 |

### 6.2 边界

- Workspace BC **NEVER** 自行注入 Context Window——它只提供数据源。
- Context Management 经端口读取数据，自行决定注入位置和格式。
- `current_branch` 属于 `GitWorktreeOps`（内部端口），Context Management 需要时经 `WorkspaceRead` 的扩展方法或独立 trait 获取，**NEVER** 直接依赖 `GitWorktreeOps`。

> **Decision**：当前分支信息属于 git 上下文供给。目标态在 `WorkspaceRead` 中增加 `current_branch()` 方法（委托给 `GitWorktreeOps`），使 Context Management 经统一端口获取，不需依赖 `GitWorktreeOps`。此扩展在 S5 迁移时落地。

## 7. 目标目录结构

从 COLA（contract / business / gateway / api）迁移到 Screaming Architecture：

### 7.1 当前结构

```
project/src/
├── api.rs                  # re-export contract + gateway
├── contract.rs             # re-export business types
├── gateway.rs              # re-export business services
├── business.rs             # mod declarations
├── business/
│   ├── git_ops.rs          # GitWorktreeOps + GitCli + FakeGit
│   ├── workspace_service.rs # WorkspaceService
│   ├── workspace_state.rs  # WorkspaceState + 纯函数规则
│   ├── workspace_state_tests.rs
│   └── workspace_types.rs  # WorkspaceFrame + traits + errors
└── lib.rs
```

### 7.2 目标结构

```
project/src/
├── workspace/
│   ├── state.rs            # WorkspaceState + 纯函数转换规则
│   ├── frame.rs            # WorkspaceFrame
│   ├── service.rs          # WorkspaceService（单一可变状态源 + seed_isolated）
│   ├── error.rs            # WorkspaceError
│   └── port.rs             # WorkspaceRead / Control / Persist trait
├── git/
│   ├── port.rs             # GitWorktreeOps trait
│   └── cli.rs              # GitCli 生产适配器 + FakeGit 测试适配器
├── api.rs                  # BC 对外 facade
└── lib.rs
```

> **Decision**：目录迁移在 S5 Runtime 模块迁移阶段执行，不阻塞本设计定稿。迁移时保持所有 trait 和类型的公开 API 不变，只移动文件位置。

## 8. 相关文档

- Workspace 领域模型：[01-domain-model.md](01-domain-model.md)
- 模块入口：[README.md](README.md)
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md) §4 / §6 / §8
- Runtime 端口与装配：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Context Management Session：[../context-management/01-session.md](../context-management/01-session.md)
- 迁移治理：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：三端口定义、GitWorktreeOps、持久化 DTO、git 上下文供给、目标目录结构 | #791 |
