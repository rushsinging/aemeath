# Workspace 端口与适配器

> 层级：02-modules / project（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#791（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文只描述 Project 模块的目标态；实现路径与迁移进度统一见 [Migration Governance](../../03-engineering/migration-governance.md)。

## 1. 端口体系

Workspace BC 暴露三个对外 trait + 一个内部出站端口：

| 端口 | 方向 | 消费方 | 职责 |
|---|---|---|---|
| `WorkspaceRead` | 对外（入站方向） | 文件 / Bash / Worktree Tool 与 Context Management | 只读访问当前工作区位置与身份 |
| `WorkspaceControl` | 对外（入站方向） | Bash Tool / EnterWorktree Tool / ExitWorktree Tool | 变更工作区（cd / enter / exit worktree） |
| `WorkspacePersist` | 对外（入站方向） | Context Management | 快照收集 / prepare-commit 恢复 |
| `GitWorktreeOps` | 内部出站 | WorkspaceService | git 命令执行 |

> 三个对外 trait 均由 `WorkspaceService` 实现。消费方按需依赖对应 trait，降低耦合。只读文件类 Tool **NEVER** 依赖 `WorkspaceControl`；Bash / EnterWorktree / ExitWorktree 同时获得 Read 与各自所需的 Control 方法，以读取转换后的结果状态。Runtime 只编排 Tool 流程，**NEVER** 作为 `WorkspaceControl` 的直接消费者。

## 2. WorkspaceRead

只读端口，提供当前工作区位置信息：

```rust
pub trait WorkspaceRead: Send + Sync {
    /// 当前 canonical workspace root 的稳定标识
    fn workspace_id(&self) -> WorkspaceId;
    /// 当前项目身份；Memory 与 Session 切换以此为准
    fn project_identity(&self) -> ProjectIdentity;
    /// 当前工作根目录
    fn current_workspace_root(&self) -> PathBuf;
    /// 当前路径基准
    fn current_path_base(&self) -> PathBuf;
    /// 将相对路径解析为绝对路径
    fn resolve(&self, rel: &Path) -> PathBuf;
    /// 当前是否位于 linked git worktree
    fn in_worktree(&self) -> bool;
    /// 当前分支名（detached HEAD 或 NonGit 返回 None）
    fn current_branch(&self) -> Result<Option<String>, WorkspaceError>;
}
```

### 2.1 消费方

| 消费方 | 用途 |
|---|---|
| **文件 Tool**（Read / Write / Edit / Glob / Grep） | `resolve()` 将相对路径解析为绝对路径 |
| **Bash Tool** | `current_path_base()` 作为命令执行的 cwd |
| **Context Management** | `project_identity()` / `current_workspace_root()` 获取项目身份与上下文路径 |
| **Tool Scope builder** | `workspace_id()` / `current_workspace_root()` 取得调用开始时的稳定值快照 |

### 2.2 路径解析语义

| 输入 | resolve 输出 |
|---|---|
| `/abs/path` | `/abs/path`（原样） |
| `relative/path` | `path_base.join("relative/path")` |

> **Decision**：`resolve` 不做存在性校验——它只做路径拼接。存在性由调用方（Tool）在操作时检查。这避免了 `resolve` 变成 I/O 操作。

`in_worktree()` **MUST** 从已提交 WorkspaceState（例如非空 stack / 已验证 frame）纯读取，**NEVER** 临时 spawn git；NonGit 恒为 `false`。`current_branch()` 在 NonGit 返回 `Ok(None)`；Git identity 才调用 `GitWorktreeOps`，探测 / 命令失败返回结构化 `WorkspaceError`，**NEVER** 把失败伪装成 detached HEAD。

## 3. WorkspaceControl

变更端口，提供工作区位置变更能力：

```rust
pub trait WorkspaceControl: Send + Sync {
    /// 在当前 workspace root 内执行 bash cd 用例
    fn change_directory(&self, path: PathBuf) -> Result<(), WorkspaceError>;
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
| **Bash Tool** | `change_directory` | `cd` 命令 |
| **EnterWorktree Tool** | `enter` | 用户 / agent 请求进入 worktree |
| **ExitWorktree Tool** | `exit` / `switch_to` | 用户 / agent 请求退出 worktree |

### 3.2 边界约束

- `WorkspaceControl` **NEVER** 被只读文件类 Tool 依赖；只有 Bash / EnterWorktree / ExitWorktree Tool **MAY** 获得 Control。
- Bash / EnterWorktree / ExitWorktree **MUST** 同时获得同一 `WorkspaceWiring` 的 `WorkspaceRead`，在转换完成后读取 path、root 与 branch 生成现有 Tool 结果；Control 权限 **NEVER** 因此扩散给第四个 Tool。
- `change_directory` **MUST** canonicalize 并验证路径存在且位于当前 `workspace_root` 内，再委托 Project 私有 `set_path_base` 转换；`switch_to` / `enter` / `exit` **MUST** 通过 Project 内部转换统一守护 Project identity、包含关系与 repository identity。
- `set_workspace_root` 只可作为 Project 私有状态转换 helper，**NEVER** 出现在公开 Control port；否则调用方可绕过同源校验和 Project identity。
- `enter` 和 `exit` 的返回值是被压入或弹出的 `WorkspaceFrame`；变更后的状态 **MUST** 经同一实例的 `WorkspaceRead` 读取。
- `switch_to` 不压栈——它直接切换到目标路径，供 `ExitWorktree { path }` 使用（退出到指定路径而非弹栈）。
- NonGit project 只支持 `change_directory`；`enter` / `exit` / `switch_to` **MUST** 返回 `UnsupportedForNonGit`，**NEVER** 尝试 git 命令或制造 frame。

## 4. WorkspacePersist

快照端口，提供 Session 落盘与恢复能力：

```rust
pub trait WorkspacePersist: Send + Sync {
    /// 生成可持久化快照
    fn snapshot(&self) -> PersistedWorkspaceContext;
    /// 完整校验并构造不透明恢复令牌；NEVER 修改 live state
    fn prepare_restore(
        &self,
        dto: &PersistedWorkspaceContext,
    ) -> Result<PreparedWorkspaceRestore, WorkspaceRestoreError>;
    /// 在 session-switch gate 内消费已验证令牌；MUST 无失败、无取消点
    fn commit_restore(&self, prepared: PreparedWorkspaceRestore);
}
```

`PreparedWorkspaceRestore` 与 prepare 错误属于公开端口协议：

```rust
#[must_use]
pub struct PreparedWorkspaceRestore { /* private: 完整 WorkspaceState */ }

impl PreparedWorkspaceRestore {
    /// prepare 已 canonicalize / probe / 校验的 identity；供后续 Memory / Config prepare 使用
    pub fn project_identity(&self) -> &ProjectIdentity;
}

pub enum WorkspaceRestoreError {
    InvalidProjectIdentity,
    PathNotFound { path: String },
    PathOutsideWorkspaceRoot { path: String, root: String },
    InvalidStackShape,
    RepositoryMismatch,
    WorkspaceIdMismatch,
    GitProbeFailed(GitProbeError),
}
```

token 的类型名因 `WorkspacePersist` 跨 crate 消费而公开，但字段与构造器 **MUST** 保持 Project-private；唯一只读 accessor `project_identity()` 返回 Project 已验证的 canonical identity，**NEVER** 暴露 candidate state 的其他字段。token 不实现 `Clone`、`Serialize` 或 `Deserialize`，只允许 `commit_restore` 按值消费一次，因此不是 Session DTO。`WorkspaceRestoreError` **MUST** 保留结构化类别，路径展示 **MUST** 经过安全化，**NEVER** 用任意字符串替代协议语义。

### 4.1 持久化 DTO

| DTO | 字段 | 说明 |
|---|---|---|
| `PersistedWorkspaceContext` | `workspace_id: WorkspaceId` | 当前 canonical workspace root 的稳定标识；prepare 时验证与 identity/root 一致 |
| | `project_identity: ProjectIdentity` | 项目身份；包含 canonical `initial_cwd`，供 Memory 与跨项目 resume 使用 |
| | `path_base: String` | 当前路径基准 |
| | `workspace_root: String` | 当前工作根 |
| | `worktree_kind: WorktreeKind` | 当前 root 已验证的 `NonGit / Primary / Linked` 分类 |
| | `context_stack: Vec<PersistedWorkspaceFrame>` | 栈快照 |
| `PersistedWorkspaceFrame` | `path_base: String` | 栈帧路径基准 |
| | `workspace_root: String` | 栈帧工作根 |
| | `worktree_kind: WorktreeKind` | 栈帧 root 的已验证分类 |
| `ProjectIdentity` | `initial_cwd: String` | Project-owned Published Language；同一已提交 WorkspaceState 内不可变 |
| | `git_common_dir: Option<String>` | git workspace 为 canonical common dir；普通目录为 `None` |
| `WorkspaceId` | opaque string | Project-owned Published Language；由 `(ProjectIdentity, canonical workspace_root)` 确定，fork 继承当前位置时保持，切换 root 时随之变化 |

> DTO 与身份值对象由 Project 发布，Session 只内嵌其 wire copy。路径序列化为 `String`，进入 live state 前 **MUST** canonicalize。旧 Session 缺少 `workspace_id` / `project_identity` 时，#894 的兼容 ACL **MUST** 先以旧 `LegacySessionDto.cwd` 推导 canonical `ProjectIdentity`，再结合 snapshot 的 canonical workspace root 推导 `WorkspaceId`，然后调用 Target port；`WorkspacePersist` **NEVER** 接收半升级 DTO。新 writer **NEVER** 另写独立 cwd 真相。

Target Session writer **MUST** 始终写出 `PersistedWorkspaceContext`。legacy `workspace: None + cwd` **MUST** 先 canonicalize cwd，并升级为 `project_identity.initial_cwd = workspace_root = path_base = cwd`、空 stack、按完整 identity + root 派生 `WorkspaceId` 的规范 snapshot；若 cwd 位于 git repo，兼容 reader同时记录 canonical git common dir，否则使用 NonGit identity。兼容来源只进诊断，**NEVER** 保留 active slot 的旧 Workspace。

旧 `workspace: Some` snapshot 及其 frame 允许 wire `worktree_kind` 缺失；兼容 reader **MUST** 在构造 Target DTO 前对当前 root 与每个 frame root 分别执行实际 repository / linked-worktree probe，填入 `NonGit / Primary / Linked`，并传播任一 `GitProbeError`。它 **NEVER** 按 stack 是否为空猜测 kind，也 **NEVER** 默认成 Primary。新 writer **MUST** 为当前 state 与每个 frame 始终写出 kind。

### 4.2 快照边界

- `snapshot` 收集 identity + `path_base` + `workspace_root` + `worktree_kind` + `stack` 全量快照。
- `prepare_restore` **MUST** 在不修改 live state 的前提下 canonicalize 并完整校验：identity、当前 root / path、每个 stack frame 均存在；每个 `path_base` 位于对应 `workspace_root` 内；stack shape 满足不允许嵌套 worktree的不变量。git identity 下，initial/root/全部 frame **MUST** probe 为同一 canonical git common dir，并验证 / 构造每一层 `worktree_kind`；non-git identity 下，实际 repository probe **MUST** 明确返回 `RepositoryProbe::NonGit`，同时 DTO `git_common_dir` 为 `None`、stack 为空、root 等于 `initial_cwd`、path 位于 root 内。路径已经变成 Git repo 时 **NEVER** 以伪 NonGit identity 恢复。任一检查失败只返回结构化错误，live state **MUST** 不变。
- `PreparedWorkspaceRestore` 是 Project-owned、不可伪造且一次性消费的 opaque token，封装完整的新 `WorkspaceState`；Context Management **NEVER** 读取其内部字段。
- `commit_restore` **MUST** 只在 Context Management 持有排他 session-switch gate、Main Run admission 与其他 Read / Control / Persist 观察者均被阻断时调用，并以一次锁内替换提交完整 state。令牌已经过完整验证，因此 commit **MUST** 无失败、无异步等待、无取消点。
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
     │  prepare_restore(dto)         │
     │ ────────────────────────────▶ │
     │                               │ 完整校验；live state 不变
     │ ◀──────────────────────────── │
     │  Prepared token / Err         │
     │                               │
     │  commit_restore(token)        │
     │ ────────────────────────────▶ │
     │                               │ gate 内无失败全量替换
```

## 5. GitWorktreeOps（内部出站端口）

Workspace BC 的 git 出站端口，封装所有 git 命令执行：

```rust
pub(crate) trait GitWorktreeOps: Send + Sync {
    /// 一次区分 Git repo、合法 NonGit 与探测失败
    fn probe_repository(&self, path: &Path)
        -> Result<RepositoryProbe, GitProbeError>;
    /// Git identity 内查询 canonical top level
    fn show_toplevel(&self, path: &Path) -> Result<PathBuf, GitOperationError>;
    /// 当前路径是否位于 linked git worktree
    fn is_linked_worktree(&self, path: &Path) -> Result<bool, GitOperationError>;
    /// git worktree add
    fn worktree_add(&self, repo_root: &Path, path: &Path, branch: &str, base: &str)
        -> Result<(), GitOperationError>;
    /// 当前分支名（detached HEAD 返回 None）
    fn current_branch(&self, path: &Path)
        -> Result<Option<String>, GitOperationError>;
}

pub(crate) enum RepositoryProbe {
    Git {
        canonical_top_level: PathBuf,
        canonical_common_dir: PathBuf,
        worktree_kind: WorktreeKind,
    },
    NonGit,
}

pub enum GitProbeError {
    GitUnavailable,
    PermissionDenied,
    CommandFailed { exit_code: Option<i32> },
    InvalidOutput,
}

pub enum GitOperationError {
    GitUnavailable,
    PermissionDenied,
    CommandFailed { exit_code: Option<i32> },
    InvalidOutput,
}
```

`GitCli::probe_repository` 只有在 git 明确返回“not a repository”时才产生 `RepositoryProbe::NonGit`；可执行文件不存在、权限拒绝、信号退出、非预期 status 或不可解析输出 **MUST** 返回 `GitProbeError`。这一区分是 NonGit 支持的前提，**NEVER** 用 `Result<PathBuf, String>` 把探测失败吞成普通目录。

### 5.1 适配器

| 适配器 | 说明 |
|---|---|
| `GitCli` | 生产适配器，spawn `git` CLI 子进程 |
| `FakeGit` | 测试适配器，内存模拟，用于纯函数规则的单测 |

### 5.2 设计约束

- `GitWorktreeOps` 是 Workspace BC 的**内部出站端口**，不对外暴露。
- `WorkspaceService` 持有 `Arc<dyn GitWorktreeOps>`，`fork` 时共享。
- `GitCli` **MAY** spawn 子进程（project feature 可 spawn，shared 不可）。
- **NEVER** 在 `shared` 层 spawn 子进程——git CLI 调用只在 project feature 的 `GitCli` 适配器中。

### 5.3 Production wiring 可见性

`GitWorktreeOps` 与 `GitCli` 保持 Project 私有，因此 Composition Root **NEVER** 直接命名或构造它们。Project **MUST** 从 capability crate root 的窄 façade 暴露仅供 Composition Root 调用的 production factory；这不是要求建立通用 `api/` 层：

```text
project::wire_production_workspace(cwd) -> Result<WorkspaceWiring, WorkspaceInitError>

WorkspaceWiring（字段私有）
├── read() -> Arc<dyn WorkspaceRead>
├── control() -> Arc<dyn WorkspaceControl>
├── persist() -> Arc<dyn WorkspacePersist>
└── derive_isolated() -> WorkspaceWiring
```

`WorkspaceInitError` **MUST** 至少区分 `PathNotFound`、`NotDirectory`、`PermissionDenied`、`CanonicalizeFailed` 与 `GitProbeFailed(GitProbeError)`；只有明确 `RepositoryProbe::NonGit` 才是合法普通目录，探测失败 **NEVER** 被降级为 NonGit。

- `WorkspaceService::new(cwd, git)` **MUST** 接受注入的 `Arc<dyn GitWorktreeOps>` 并保持 crate-private；production factory 在 Project 内部构造私有 `GitCli` 后调用它，测试通过同一入口注入 `FakeGit`。
- `wire_production_workspace(cwd)` **MUST** 要求 cwd 可访问且为目录，在 Project 内 canonicalize；若位于 git repository 则记录 canonical git common dir，普通非 git 目录则建立 `NonGit` identity。路径不存在、不可访问或不可 canonicalize 时返回结构化 `WorkspaceInitError`，**NEVER** 以未校验路径建立 wiring。
- Composition Root **MUST** 通过调用 `wire_production_workspace(cwd)` 选择生产 wiring，**NEVER** 持有私有 git adapter 或出站 port。
- `WorkspaceWiring` 是字段私有的 composition-only handle，**NEVER** 是第四个稳定业务契约，也 **NEVER** 合并三个窄 trait。它 **MUST** 只向业务消费者分发所需 trait view，并由 `derive_isolated()` 在 Project 内部保留 fork 隔离语义。
- Composition **MUST** 将 Main handle 保存在 composition-internal、active-main-session-slot-lifetime 的 `CompositionWorkspaceScope`；同一 Session 的全部 Main Run 复用它，运行期 resume 在 gate 内替换其完整 live state。Project production factory **NEVER** 按 Run 重复调用。Sub **MUST** 从父 handle 调用 `derive_isolated()` 创建 Run-lifetime child scope。同一 scope 的 `read()` / `persist()` view 交给 Context Management backing implementation，`read()` / 按需 `control()` view 交给 Tool backing implementation。
- RuntimeContext **NEVER** 持有 Project trait、`WorkspaceWiring` 或 composition workspace scope；`WorkspaceMode` 只由 Composition 解释，Project **NEVER** 为此发布额外的 Runtime Workspace façade。
- 架构守卫 **MUST** 将 production factory 与 opaque handle 的跨 crate 消费限制在 `agent/composition`；其他 feature **NEVER** import 或调用该 wiring surface。

## 6. git 上下文供给

### 6.1 数据流

Context Management 在构建 Context Window 时，经 `WorkspaceRead` 读取工作区信息注入：

| 数据 | 来源 | 注入位置 |
|---|---|---|
| 项目身份 / 根路径 | `WorkspaceRead::project_identity()` | Memory 分区、System Prompt / AGENTS.md 路径 |
| 当前工作目录 | `WorkspaceRead::current_path_base()` | System Prompt 上下文 |
| 工作根 | `WorkspaceRead::current_workspace_root()` | System Prompt 上下文 |
| 是否在 worktree | `WorkspaceRead::in_worktree()` | 上下文标记 |
| 当前分支 | `WorkspaceRead::current_branch()` | 上下文标记 |

### 6.2 边界

- Workspace BC **NEVER** 自行注入 Context Window——它只提供数据源。
- Context Management 经端口读取数据，自行决定注入位置和格式。
- `WorkspaceService` **MUST** 在内部将 `WorkspaceRead::current_branch()` 委托给 `GitWorktreeOps::current_branch()`；Context Management **NEVER** 直接依赖内部 `GitWorktreeOps`。

## 7. 目标代码组织

Project 以 `workspace` 能力、`git` 外部 seam 和窄 crate-root façade 组织。以下是当前复杂度下的非规范性投影，不是其他 capability 必须复制的目录模板；Rust 模块采用 2018+ 的同名文件与目录并存形状，**NEVER** 新增 `mod.rs`：

```
project/src/
├── lib.rs                  # 窄 crate façade + composition-only factory；不建立通用 api 层
├── workspace.rs            # Workspace Published Language、窄 views 与能力根
├── workspace/
│   ├── state.rs            # WorkspaceState + 纯函数转换规则
│   ├── frame.rs            # WorkspaceFrame
│   ├── switch.rs           # switch / restore 用例与 WorkspaceService
│   └── error.rs            # WorkspaceError
└── git.rs                  # 私有 GitWorktreeOps + 单一 GitCli adapter；FakeGit 放测试支持中
```

- crate root **MUST** 是 Project 的唯一跨 capability 公开 façade。面向业务消费者的稳定表面 **MUST** 只发布 `WorkspaceRead` / `WorkspaceControl` / `WorkspacePersist` 及其 Published Language；`wire_production_workspace` 与 opaque handle 是仅供 Composition Root 的 wiring 例外，**NEVER** 成为业务契约。新增 `api.rs` / `api/` **MUST** 先满足[代码组织规范](../../01-system/06-code-organization.md)的独立升级条件，不能因“跨模块公开”自动生成。
- `workspace.rs` 与 `workspace/` **MUST** 共同拥有 workspace 状态、转换规则、用例和三个窄 view；稳定 trait **MUST** 靠近其消费用例定义，**NEVER** 为对称性创建空泛 `port.rs`。每个 Main / Sub workspace context **MUST** 各有且仅有一个 `WorkspaceService`，同一 context 内 **NEVER** 复制第二份可变状态或缓存。
- `git.rs` **MUST** 先将 `GitWorktreeOps` 与当前唯一 `GitCli` adapter 共置在 Project 内部；只有出现多个独立 adapter 或独立演进压力时才升级为 `git/` 技术子目录，**NEVER** 为单文件集成预建目录，也 **NEVER** 把 git CLI 或 wire detail 泄漏到对外 façade。
- 该结构 **MUST** 遵循 [代码组织规范](../../01-system/06-code-organization.md) 的 capability-first、use-case colocation 与 ports on demand 判据，**NEVER** 被解释为其他 feature 统一复制的通用目录模板。

## 8. 相关文档

- Workspace 领域模型：[01-domain-model.md](01-domain-model.md)
- 模块入口：[README.md](README.md)
- 系统级代码组织规范：[../../01-system/06-code-organization.md](../../01-system/06-code-organization.md)
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md) §4 / §6 / §8
- Runtime 端口与装配：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Context Management Session：[../context-management/01-session.md](../context-management/01-session.md)
- 架构守卫运行时真相：[../../03-engineering/architecture-guards.md](../../03-engineering/architecture-guards.md)
- 迁移治理：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：三端口定义、GitWorktreeOps、持久化 DTO、git 上下文供给、目标目录结构 | #791 |
| 2026-07-14 | 对齐 capability-first Project 目标树、三个窄 trait 与直接消费者；以 opaque production factory / active-main-session-slot scope 隔离私有 git adapter；补 Project identity 与 prepare-commit restore，移除 Runtime Workspace façade | [#972](https://github.com/rushsinging/aemeath/issues/972) |
