# Workspace 领域模型

> 层级：02-modules / project（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#791（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)

## 1. Workspace 聚合根

Workspace 是 Project / Workspace BC 的聚合根，封装工作区上下文的全部状态与不变量。它以 `WorkspaceState` 为内部状态，以 `WorkspaceService` 为外部可变入口。

### 1.1 WorkspaceState

| 字段 | 类型 | 说明 |
|---|---|---|
| `project_identity` | `ProjectIdentity` | 当前已提交的完整项目身份：canonical initial cwd + optional canonical git common dir；普通 worktree 切换时**不变** |
| `workspace_root` | `PathBuf` | 当前工作根目录（worktree 进入时更新为 worktree 根） |
| `path_base` | `PathBuf` | 当前路径基准（bash cd 可变更，worktree enter/exit 时更新） |
| `worktree_kind` | `WorktreeKind` | `NonGit / Primary / Linked`；由 factory / transition / restore probe 后提交，供 `in_worktree()` 纯读 |
| `stack` | `Vec<WorkspaceFrame>` | 上下文栈（worktree enter 压栈，exit 弹栈） |

### 1.2 不变量

| # | 不变量 | 守护方式 |
|---|---|---|
| INV-1 | 完整 `project_identity` 在同一已提交 `WorkspaceState` 内不可变；跨 Session resume 只能以 prepare / commit 全量替换 state | 无 setter；commit token 已包含并一次替换 identity，**NEVER** 在 commit 后重新 probe |
| INV-2 | `workspace_root` 和 `path_base` 必须指向已存在的 canonical 路径 | 初始化、set / switch、enter / exit、prepare_restore 均校验 |
| INV-3 | Git identity 下 root / frame 必须与 `project_identity.initial_cwd` 属同一 git common dir；NonGit identity 下 stack 必须为空且 root 等于 `project_identity.initial_cwd` | git 用 `validate_in_repo`；non-git 禁止 worktree transition |
| INV-4 | 栈非空时禁止 enter（不允许嵌套 worktree） | `enter` 检查 `stack.is_empty()`，残栈自愈除外 |
| INV-5 | 栈帧的 `workspace_root` + `path_base` 必须指向退出前的有效状态 | 压栈时快照当前状态 |
| INV-6 | `path_base` 必须在 `workspace_root` 内或等于它 | 路径解析时保证 |
| INV-7 | `worktree_kind` 必须与已验证的 identity / root 一致；NonGit 对应 NonGit，Git primary / linked 由 repository probe 判定 | factory、enter / exit / switch、prepare token 全量写入；普通 read **NEVER** 重探测 |

### 1.3 路径解析

`resolve(relative)` 将相对路径解析为绝对路径：

- 绝对路径：原样返回。
- 相对路径：`path_base.join(relative)`。

所有 Tool 的文件操作**MUST**经 `WorkspaceRead::resolve()` 解析路径，**NEVER** 自行拼接 `std::env::current_dir()`。

## 2. WorkspaceFrame

### 2.1 结构

| 字段 | 类型 | 说明 |
|---|---|---|
| `path_base` | `PathBuf` | 上一层的路径基准 |
| `workspace_root` | `PathBuf` | 上一层的工作根目录 |
| `worktree_kind` | `WorktreeKind` | 上一层已验证的 NonGit / Primary / Linked 分类 |

### 2.2 栈语义

```
enter worktree:
  1. 校验栈为空（INV-4，残栈自愈除外）
  2. 解析目标路径（resolve_worktree_path）
  3. 若目标不存在，git worktree add 创建
  4. validate_in_repo 校验同源（INV-3）
  5. 压栈当前 { path_base, workspace_root, worktree_kind }（INV-5 / INV-7）
  6. 更新 workspace_root = worktree 根
  7. 更新 path_base = 目标路径（canonicalize）
  8. 更新 worktree_kind = Linked

exit worktree:
  1. 弹栈
  2. 恢复 workspace_root = frame.workspace_root
  3. 恢复 path_base = frame.path_base
  4. 恢复 worktree_kind = frame.worktree_kind
  5. 栈空时返回 EmptyStack 错误
```

### 2.3 残栈自愈

当 `enter` 发现栈非空时，control 用例 **MAY** 通过 fallible `GitWorktreeOps::is_linked_worktree(path_base)` 复核残栈；明确返回 `false` 才可清栈并继续，probe 失败必须返回结构化错误。普通 `WorkspaceRead::in_worktree()` 只读已提交 `worktree_kind`，**NEVER** 执行该自愈 probe。

> **Decision**：残栈自愈是防御性设计，避免崩溃后状态不一致导致 worktree 永久不可用。自愈时记录 warning 日志。

## 3. 状态转换规则

Workspace 用例把 candidate-state 规则与 fallible Git / filesystem seam 分开：规则函数可纯测试；enter / switch / prepare 等 orchestration **MAY** 调用注入的 `GitWorktreeOps`（enter 还可能执行 `worktree_add`），但在全部校验成功前 **NEVER** 修改 live WorkspaceState。这里的保证是“无隐藏 live-state mutation”，**NEVER** 把外部 I/O 冒充纯函数。

### 3.1 转换函数

| 函数 | 签名 | 语义 |
|---|---|---|
| `set_path_base` | `(state, path) → Result` | 更新 path_base（bash cd 用） |
| `set_workspace_root` | `(state, root, path) → Result` | 更新 workspace_root + path_base（worktree enter/exit 用） |
| `enter` | `(state, git, path, branch) → Result<Frame>` | 压栈 + 进入 worktree |
| `exit` | `(state) → Result<Frame>` | 弹栈 + 退出 worktree |
| `switch_to` | `(state, git, path) → Result` | 切换到指定路径（不压栈，ExitWorktree{path} 用） |
| `snapshot` | `(state) → PersistedWorkspaceContext` | 生成持久化快照 |
| `prepare_restore` | `(live_state, dto, git) → Result<PreparedWorkspaceRestore>` | 不修改 live state，完整校验并构造新 state token |
| `commit_restore` | `(state_slot, prepared) → ()` | session-switch gate 内无失败全量替换 |

`set_workspace_root` 是 Project 内部转换 helper，**NEVER** 进入公开 `WorkspaceControl`；公开的 `switch_to` / `enter` / `exit` 在调用它前必须完成同源、identity 与包含关系校验。

### 3.2 规则 / I/O 分离优势

- **可测试**：candidate-state 规则直接做纯单测；配合 `FakeGit` 可覆盖 orchestration，无需真实 git 仓库。
- **可推理**：I/O 只经明确 port，失败时 live state 不变；commit 只消费已验证 token。
- **可复用**：`WorkspaceService` 只持 state slot 与注入端口，统一委托同一组规则 / 用例。

## 4. fork 隔离范式

### 4.1 动机

子 agent（SubAgent）需要一个独立的工作区上下文，但不能影响父 agent 的状态。直接共享 `WorkspaceService` 会导致子 agent 的 worktree 操作干扰父 agent。

### 4.2 范式

`fork()` 从父 agent 当前快照派生独立实例：

```
fn fork(&self) -> Arc<Self>:
  let s = self.lock();
  Arc::new(WorkspaceService {
    state: Mutex::new(WorkspaceState {
      project_identity: s.project_identity.clone(), // 完整继承；不重新 probe
      workspace_root: s.workspace_root.clone(), // 继承当前位置
      path_base: s.path_base.clone(),           // 继承当前位置
      worktree_kind: s.worktree_kind,           // 继承已验证分类
      stack: Vec::new(),                        // 空栈
    }),
    control_operation: Mutex::new(()),          // 子 context 独立写操作串行器
    git: self.git.clone(),                      // 共享 git 端口
  })
```

### 4.3 隔离保证

| 维度 | 父 agent | 子 agent |
|---|---|---|
| `project_identity` | 不变 | 完整继承父的 |
| `workspace_root` | 不受子影响 | 继承父当前的 |
| `path_base` | 不受子影响 | 继承父当前的 |
| `worktree_kind` | 不受子影响 | 继承父当前已验证分类 |
| `stack` | 保持父的 | 空 |
| `Mutex` | 独立锁 | 独立锁 |
| `git` | 共享 | 共享 |

### 4.4 安全铁律

子 agent workspace 能力 ≤ 父 agent：

- 子 agent 继承父的**当前位置**（workspace_root + path_base），可以在这之上 enter / exit worktree。
- 子 agent 继承父的 Project identity 与当前 canonical workspace root，因此初始 `WorkspaceId` 相同；任一方切换 root 后，Project 按 `(ProjectIdentity, canonical workspace_root)` 独立派生其新 ID。`WorkspaceId` 识别工作根，**NEVER** 被当作可变 state 实例 ID。
- 子 agent **不继承栈**——它不能 exit 到父 agent 之前的 worktree。
- 子 agent 的 enter / exit 操作只影响自己的 state，父 agent 无感知。
- `git` 端口共享是安全的（git CLI 是无状态命令）。

> **Decision**：fork 是 Workspace BC 的核心隔离范式，与 Runtime 的 SubAgent ExecutionPolicy 对齐。production wiring handle 的 `derive_isolated()` **MUST** 委托 `WorkspaceService::fork()`，再按需分发新实例的 `WorkspaceRead` / `WorkspaceControl` / `WorkspacePersist` 独立 view；`WorkspaceService` 与 opaque handle **NEVER** 泄漏为业务契约。

## 5. WorkspaceError

### 5.1 错误分类

| 错误 | 语义 | 触发场景 |
|---|---|---|
| `PathNotFound(PathBuf)` | 路径不存在或无法访问 | change_directory / enter / switch_to 时路径无效 |
| `MissingPathAndBranch` | 未提供 path 或 branch | enter 时两者都为 None |
| `InvalidBranch` | branch 名只含分隔符或敏感字符 | sanitize_branch_for_path 返回空 |
| `NestedWorktree` | 已在 worktree 中尝试再 enter | 栈非空且 in_worktree 为 true |
| `RepoMismatch` | 路径不属于当前仓库 | validate_in_repo 校验 git_common_dir 不一致 |
| `EmptyStack` | 栈为空时尝试 exit | exit 时 stack 为空 |
| `UnsupportedForNonGit` | NonGit project 不支持 worktree transition | enter / exit / switch_to；change_directory 仍可在 root 内使用 |
| `GitProbeFailed(GitProbeError)` | git 不可用、权限失败、命令异常或输出损坏 | repository identity 探测 / 校验失败 |
| `GitOperationFailed(GitOperationError)` | 已确认 Git identity 后的具体命令失败 | branch / worktree add / top-level 查询 |

### 5.2 用户消息

所有错误消息使用中文，面向终端用户可读。`WorkspaceError` 实现 `Display` 输出中文消息，`std::error::Error` 供程序化处理。

初始化、普通控制命令与 prepare-commit 恢复使用不同的公开错误协议：`WorkspaceInitError` 只表达 production factory 的 cwd 初始化失败；`WorkspaceError` 表达 live 控制操作；`WorkspaceRestoreError` 表达无副作用 prepare 校验失败。精确公开 variants 与 opaque token 以 [02-ports-and-adapters.md](02-ports-and-adapters.md) 为唯一真相；三类错误 **NEVER** 退化为一个无类别字符串。

## 6. WorkspaceService

### 6.1 定位

`WorkspaceService` 是单个 workspace context 内的有状态 façade 与唯一可变状态持有者，**NEVER** 是生产装配入口：

- 持有 `Mutex<WorkspaceState>`（单一可变状态源）。
- 持有独立 `Mutex<()>` 作为 control-operation 串行器；同一 context 的所有写用例共享它。
- 持有 `Arc<dyn GitWorktreeOps>`（git 出站端口）。
- 实现 `WorkspaceRead` + `WorkspaceControl` + `WorkspacePersist`。
- 提供 `fork()` 派生子实例。
- 只消费构造时注入的 git 出站端口，**NEVER** 自行选择生产适配器。

### 6.2 锁策略

- 使用 `std::sync::Mutex`（非 `tokio::sync::Mutex`），因为 workspace 操作是同步的（git CLI 是同步进程）。
- `lock()` 使用 `unwrap_or_else(|e| e.into_inner())` 处理毒锁——即使持有锁的线程 panic 也能继续。
- `change_directory` / `switch_to` / `enter` / `exit` / `commit_restore` 等所有写用例 **MUST** 先取得同一 `control_operation` mutex，并持有到整个用例结束；因此同一 workspace context 的写操作严格串行。`fork()` 创建自己的串行器，父子 context 仍彼此隔离。
- `in_worktree()` 只在短 state lock 内读取已提交 `worktree_kind`，**NEVER** spawn git。需要 Git / filesystem I/O 的 control 用例在保持 `control_operation` guard 的同时释放 state lock，完成全部 fallible I/O 后才重新短暂取得 state lock、一次提交 candidate；state lock **NEVER** 跨 I/O，读者在提交前只会看到旧的完整状态。
- `prepare_restore` / `commit_restore` **MUST** 由 Context Management 的 exclusive session-switch gate 包围；`commit_restore` 仍取得 `control_operation` mutex。exclusive gate 阻止 prepare token 生成与 commit 之间出现外部 Workspace 写入，Project 内部串行器阻止同一提交段内与其他写用例交错。

### 6.3 构造与装配

| 方法 | 说明 |
|---|---|
| `pub(crate) new(cwd, git: Arc<dyn GitWorktreeOps>)` | 使用注入的 git 出站端口构造模块 façade；只供 Project 内部 wiring 与测试 |
| `project::wire_production_workspace(cwd)` | 仅供 Composition Root 从 crate-root 窄 façade 选择的生产 factory；返回 opaque wiring handle |
| `fork(&self)` | Project 内部派生子 agent 实例，由 wiring handle 的 `derive_isolated()` 委托 |

- `WorkspaceService` 的 Target 构造 **MUST** 只接受注入的 `Arc<dyn GitWorktreeOps>`，并 **MUST** 保持 crate-private。
- `wire_production_workspace(cwd)` **MUST** 在 Project 内部构造私有 `GitCli`，调用 crate-private 构造，并返回字段不公开的 Project-owned wiring handle。
- Composition Root **MUST** 通过调用该 factory 选择 Project 的生产 wiring，**NEVER** 直接命名、持有或构造私有 `GitCli` / `GitWorktreeOps`。
- opaque wiring handle **MUST** 分别提供 `WorkspaceRead`、`WorkspaceControl`、`WorkspacePersist` view 与 Project-owned `derive_isolated()`；业务消费者 **NEVER** 接收该 handle。
- 测试 **MUST** 通过同一 crate-private 构造契约注入 `FakeGit`。

## 7. 相关文档

- Workspace 端口与适配器：[02-ports-and-adapters.md](02-ports-and-adapters.md)
- 模块入口：[README.md](README.md)
- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md) §7
- 系统架构：[../../01-system/04-system-architecture.md](../../01-system/04-system-architecture.md)
- 代码组织规范：[../../01-system/06-code-organization.md](../../01-system/06-code-organization.md)
- Runtime 领域模型（SubAgent / ExecutionPolicy）：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)
- 迁移治理：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Workspace 聚合根、Frame 栈、状态转换规则、fork、错误模型 | #791 |
| 2026-07-14 | 将 WorkspaceService 定位为模块有状态 façade 与 crate-private 注入构造，以 Project-owned factory 向 Composition Root 提供 production wiring，并由 opaque handle 保留隔离派生与三个窄 trait view | [#972](https://github.com/rushsinging/aemeath/issues/972) |
