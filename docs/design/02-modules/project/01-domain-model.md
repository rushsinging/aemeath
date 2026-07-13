# Workspace 领域模型

> 层级：02-modules / project（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#791（S2）

## 1. Workspace 聚合根

Workspace 是 Project / Workspace BC 的聚合根，封装工作区上下文的全部状态与不变量。它以 `WorkspaceState` 为内部状态，以 `WorkspaceService` 为外部可变入口。

### 1.1 WorkspaceState

| 字段 | 类型 | 说明 |
|---|---|---|
| `initial_cwd` | `PathBuf` | 项目启动时的 cwd，worktree 切换时**不变**。memory 等绑定项目身份的读写必须用此路径 |
| `workspace_root` | `PathBuf` | 当前工作根目录（worktree 进入时更新为 worktree 根） |
| `path_base` | `PathBuf` | 当前路径基准（bash cd 可变更，worktree enter/exit 时更新） |
| `stack` | `Vec<WorkspaceFrame>` | 上下文栈（worktree enter 压栈，exit 弹栈） |

### 1.2 不变量

| # | 不变量 | 守护方式 |
|---|---|---|
| INV-1 | `initial_cwd` 创建后不可变 | 无 setter |
| INV-2 | `workspace_root` 和 `path_base` 必须指向已存在的路径 | enter / restore 时校验存在性 |
| INV-3 | `workspace_root` 必须与 `initial_cwd` 属同一 git 仓库 | `validate_in_repo` 校验 `git_common_dir` 一致 |
| INV-4 | 栈非空时禁止 enter（不允许嵌套 worktree） | `enter` 检查 `stack.is_empty()`，残栈自愈除外 |
| INV-5 | 栈帧的 `workspace_root` + `path_base` 必须指向退出前的有效状态 | 压栈时快照当前状态 |
| INV-6 | `path_base` 必须在 `workspace_root` 内或等于它 | 路径解析时保证 |

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

### 2.2 栈语义

```
enter worktree:
  1. 校验栈为空（INV-4，残栈自愈除外）
  2. 解析目标路径（resolve_worktree_path）
  3. 若目标不存在，git worktree add 创建
  4. validate_in_repo 校验同源（INV-3）
  5. 压栈当前 { path_base, workspace_root }（INV-5）
  6. 更新 workspace_root = worktree 根
  7. 更新 path_base = 目标路径（canonicalize）

exit worktree:
  1. 弹栈
  2. 恢复 workspace_root = frame.workspace_root
  3. 恢复 path_base = frame.path_base
  4. 栈空时返回 EmptyStack 错误
```

### 2.3 残栈自愈

当 `enter` 发现栈非空但 `git in_worktree(path_base)` 返回 `false` 时，说明之前的 worktree 退出未正确清栈（如进程崩溃）。此时清空栈并继续 enter，而非报错。

> **Decision**：残栈自愈是防御性设计，避免崩溃后状态不一致导致 worktree 永久不可用。自愈时记录 warning 日志。

## 3. 状态转换规则

所有状态转换是纯函数，接收 `&mut WorkspaceState` + `&dyn GitWorktreeOps`，无隐藏副作用：

### 3.1 转换函数

| 函数 | 签名 | 语义 |
|---|---|---|
| `set_path_base` | `(state, path) → Result` | 更新 path_base（bash cd 用） |
| `set_workspace_root` | `(state, root, path) → Result` | 更新 workspace_root + path_base（worktree enter/exit 用） |
| `enter` | `(state, git, path, branch) → Result<Frame>` | 压栈 + 进入 worktree |
| `exit` | `(state) → Result<Frame>` | 弹栈 + 退出 worktree |
| `switch_to` | `(state, git, path) → Result` | 切换到指定路径（不压栈，ExitWorktree{path} 用） |
| `snapshot` | `(state) → PersistedWorkspaceContext` | 生成持久化快照 |
| `restore` | `(state, dto) → Result` | 从快照恢复 |

### 3.2 纯函数优势

- **可测试**：配合 `FakeGit` 可完整单元测试所有转换路径，无需真实 git 仓库。
- **可推理**：输入 → 输出明确，无隐藏状态变更。
- **可复用**：`WorkspaceService` 只是 `Mutex<WorkspaceState>` + 委托调用纯函数。

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
      initial_cwd: s.initial_cwd.clone(),      // 继承
      workspace_root: s.workspace_root.clone(), // 继承当前位置
      path_base: s.path_base.clone(),           // 继承当前位置
      stack: Vec::new(),                        // 空栈
    }),
    git: self.git.clone(),                      // 共享 git 端口
  })
```

### 4.3 隔离保证

| 维度 | 父 agent | 子 agent |
|---|---|---|
| `initial_cwd` | 不变 | 继承父的 |
| `workspace_root` | 不受子影响 | 继承父当前的 |
| `path_base` | 不受子影响 | 继承父当前的 |
| `stack` | 保持父的 | 空 |
| `Mutex` | 独立锁 | 独立锁 |
| `git` | 共享 | 共享 |

### 4.4 安全铁律

子 agent workspace 能力 ≤ 父 agent：

- 子 agent 继承父的**当前位置**（workspace_root + path_base），可以在这之上 enter / exit worktree。
- 子 agent **不继承栈**——它不能 exit 到父 agent 之前的 worktree。
- 子 agent 的 enter / exit 操作只影响自己的 state，父 agent 无感知。
- `git` 端口共享是安全的（git CLI 是无状态命令）。

> **Decision**：fork 是 Workspace BC 的核心隔离范式，与 Runtime 的 SubAgent ExecutionPolicy 对齐。SubAgent 的 `RuntimeContext` 持有独立的 `WorkspaceService` 实例。

## 5. WorkspaceError

### 5.1 错误分类

| 错误 | 语义 | 触发场景 |
|---|---|---|
| `PathNotFound(PathBuf)` | 路径不存在或无法访问 | enter / switch_to / restore 时路径无效 |
| `MissingPathAndBranch` | 未提供 path 或 branch | enter 时两者都为 None |
| `InvalidBranch` | branch 名只含分隔符或敏感字符 | sanitize_branch_for_path 返回空 |
| `NestedWorktree` | 已在 worktree 中尝试再 enter | 栈非空且 in_worktree 为 true |
| `RepoMismatch` | 路径不属于当前仓库 | validate_in_repo 校验 git_common_dir 不一致 |
| `EmptyStack` | 栈为空时尝试 exit | exit 时 stack 为空 |
| `RestoreInvalidPath(PathBuf)` | 恢复时路径不存在 | restore 时 path_base 或 workspace_root 不存在 |
| `Git(String)` | git 命令执行失败 | GitWorktreeOps 返回 Err |

### 5.2 用户消息

所有错误消息使用中文，面向终端用户可读。`WorkspaceError` 实现 `Display` 输出中文消息，`std::error::Error` 供程序化处理。

## 6. WorkspaceService

### 6.1 定位

`WorkspaceService` 是 Workspace BC 的唯一生产入口：

- 持有 `Mutex<WorkspaceState>`（单一可变状态源）。
- 持有 `Arc<dyn GitWorktreeOps>`（git 出站端口）。
- 实现 `WorkspaceRead` + `WorkspaceControl` + `WorkspacePersist`。
- 提供 `fork()` 派生子实例。

### 6.2 锁策略

- 使用 `std::sync::Mutex`（非 `tokio::sync::Mutex`），因为 workspace 操作是同步的（git CLI 是同步进程）。
- `lock()` 使用 `unwrap_or_else(|e| e.into_inner())` 处理毒锁——即使持有锁的线程 panic 也能继续。
- `in_worktree()` 先克隆 `workspace_root` 释放状态锁，再 spawn git 子进程，避免持锁期间阻塞。

### 6.3 构造

| 方法 | 说明 |
|---|---|
| `new(cwd)` | 生产构造，使用 `GitCli` |
| `with_git(cwd, git)` | 测试构造，注入 `FakeGit` |
| `fork(&self)` | 派生子 agent 实例 |

## 7. 相关文档

- Workspace 端口与适配器：[02-ports-and-adapters.md](02-ports-and-adapters.md)
- 模块入口：[README.md](README.md)
- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md) §7
- Runtime 领域模型（SubAgent / ExecutionPolicy）：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Workspace 聚合根、Frame 栈、状态转换规则、fork、错误模型 | #791 |
