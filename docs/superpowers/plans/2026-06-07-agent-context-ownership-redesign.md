# Agent Context 所有权重构 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把弥散在 5 套类型里的 workspace 可变状态收敛为 project 拥有的单一 `WorkspaceState`(一把锁),feature 经三个能力 trait 访问,子 agent 隔离,git 抽 outbound port。

**Architecture:** project 拥有 workspace 类型 + 规则(`WorkspaceState`/`WorkspaceService`/`WorkspaceRead`/`WorkspaceControl`/`WorkspacePersist`/`GitWorktreeOps`);runtime client 跨 chat 轮次持有 `Arc<WorkspaceService>` 实例;tools 经窄能力 trait 消费;持久化 DTO(`PersistedWorkspaceContext`)留 share。依赖方向不变:`project → share`、`tools → share,project`、`runtime → 全部`。

**Tech Stack:** Rust workspace(cargo)、`std::sync::Mutex`、`async_trait`(仅 runtime AgentRunner)、serde(session DTO)。验证门禁:`cargo check`、`cargo test -p <crate>`、`cargo clippy`、`.agents/hooks/check-architecture-guards.sh`。

**设计依据:** `docs/superpowers/specs/2026-06-07-agent-context-ownership-redesign.md`

**全局约束:**
- 所有改动在 worktree `design/agent-context-ownership-redesign` 内进行,验证全过后才合并回 main。
- **MUST NOT** 手动调格式;每个提交前对改动文件区域跑过 `cargo fmt`(只关注逻辑)。
- DRY:git 调用本计划起以 `GitCli` 为唯一落点,旧 `worktree.rs`/`working_paths.rs` 的内联 git **委托** `GitCli`,不重复。

---

## 阶段顺序与"绿色边界"

| 阶段 | 内容 | 是否可独立编译 |
|---|---|---|
| P1 | share 重命名 DTO + 兼容别名 | ✅ 每步绿 |
| P2 | project 新增机制(GitCli/WorkspaceState/Service/三 trait)+ 单测,**纯增量** | ✅ 每步绿 |
| P3 | 枢轴:tools+runtime 同时切到 `Arc<WorkspaceService>` | ⚠️ 阶段末统一验证 |
| P4 | composition 清理(移除 ProjectGateway) | ✅ |
| P5 | 收尾:移 `WorkingContext` 出 share、删 shim、改/加 guard、全量验证 | ✅ |

> P3 是表示层切换(3 把 `Arc<Mutex>` → 1 把 `Mutex<WorkspaceState>`),中间态无法编译,因此把全部改动列为有序子步骤,在子阶段末做一次 `cargo check`。每处改动给出 file:line 与 before/after,机械执行即可。

---

## Phase 1 — share:重命名持久化 DTO + 兼容别名

**Files:**
- Modify: `agent/shared/src/session_types.rs`

- [ ] **Step 1.1: 写失败测试(serde 字段兼容)**

在 `agent/shared/src/session_types.rs` 末尾追加:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_workspace_context_serde_field_compat() {
        let json = r#"{"path_base":"/a","working_root":"/b","context_stack":[{"path_base":"/c","working_root":"/d"}]}"#;
        let ctx: PersistedWorkspaceContext = serde_json::from_str(json).unwrap();
        assert_eq!(ctx.path_base, "/a");
        assert_eq!(ctx.working_root, "/b");
        assert_eq!(ctx.context_stack.len(), 1);
        assert_eq!(ctx.context_stack[0].path_base, "/c");
        // 旧别名仍可用
        let _legacy: WorkspaceContext = ctx.clone();
        let back = serde_json::to_string(&ctx).unwrap();
        assert_eq!(back, json);
    }
}
```

- [ ] **Step 1.2: 运行测试确认失败**

Run: `cargo test -p share persisted_workspace_context_serde_field_compat`
Expected: FAIL（`PersistedWorkspaceContext` 未定义）

- [ ] **Step 1.3: 重命名 struct 并加别名**

把 `agent/shared/src/session_types.rs:8-23` 改为：

```rust
/// Workspace context for worktree support — persisted session DTO.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct PersistedWorkspaceContext {
    pub path_base: String,
    pub working_root: String,
    #[serde(default)]
    pub context_stack: Vec<PersistedWorkspaceFrame>,
}

/// An entry in the persisted workspace context stack (for nested worktrees).
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct PersistedWorkspaceFrame {
    pub path_base: String,
    pub working_root: String,
}

/// 迁移期兼容别名（P5 删除）。
pub type WorkspaceContext = PersistedWorkspaceContext;
pub type WorkspaceStackEntry = PersistedWorkspaceFrame;
```

- [ ] **Step 1.4: 运行测试确认通过 + 全量 check**

Run: `cargo test -p share persisted_workspace_context_serde_field_compat && cargo check`
Expected: PASS；其余 crate 经别名编译通过。

- [ ] **Step 1.5: 提交**

```bash
git add agent/shared/src/session_types.rs
git commit -m "refactor(share): WorkspaceContext→PersistedWorkspaceContext + 兼容别名"
```

---

## Phase 2 — project:新增 workspace 机制(纯增量)+ 单测

> 全部新代码与旧 `worktree.rs`/`working_paths.rs` 并存;旧内联 git 委托新 `GitCli`(DRY)。完成后 `project::api` 导出新类型,但暂无生产消费方(仅测试)。

**Files:**
- Create: `agent/features/project/src/business/git_ops.rs`
- Create: `agent/features/project/src/business/workspace_state.rs`
- Create: `agent/features/project/src/business/workspace_service.rs`
- Create: `agent/features/project/src/contract.rs`（若已存在则追加；见 Step 2.0）
- Modify: `agent/features/project/src/business.rs`（mod 声明）
- Modify: `agent/features/project/src/business/worktree.rs`（内联 git 委托 GitCli）
- Modify: `agent/features/project/src/business/working_paths.rs`（detect_working_root 委托 GitCli）
- Modify: `agent/features/project/src/api.rs`（导出新 API）

- [ ] **Step 2.0: 确认 project 模块声明结构**

Run: `cat agent/features/project/src/business.rs agent/features/project/src/contract.rs`
Expected: 看到 `business` 的 `mod worktree; mod working_paths;` 等;`contract.rs` 现内容（可能为空或少量）。后续 Step 在此基础上加 `mod git_ops; mod workspace_state; mod workspace_service;`。

### Task 2A: GitWorktreeOps 端口 + GitCli 实现 + FakeGit

- [ ] **Step 2A.1: 写 FakeGit 驱动的失败测试**

Create `agent/features/project/src/business/git_ops.rs`，先只放 trait 与测试桩骨架以驱动编译失败：

```rust
use std::path::{Path, PathBuf};

/// Outbound port for git worktree operations used by workspace transition rules.
pub trait GitWorktreeOps: Send + Sync {
    fn git_common_dir(&self, path: &Path) -> Result<PathBuf, String>;
    fn show_toplevel(&self, path: &Path) -> Result<PathBuf, String>;
    fn in_worktree(&self, path: &Path) -> bool;
    fn worktree_add(&self, repo_root: &Path, path: &Path, branch: &str, base: &str)
        -> Result<(), String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory fake for unit testing transition rules without real git.
    #[derive(Default)]
    pub struct FakeGit {
        pub common_dir: HashMap<PathBuf, PathBuf>,
        pub toplevel: HashMap<PathBuf, PathBuf>,
        pub worktrees: std::collections::HashSet<PathBuf>,
        pub added: Mutex<Vec<PathBuf>>,
    }

    impl GitWorktreeOps for FakeGit {
        fn git_common_dir(&self, path: &Path) -> Result<PathBuf, String> {
            self.common_dir.get(path).cloned().ok_or_else(|| "no common dir".into())
        }
        fn show_toplevel(&self, path: &Path) -> Result<PathBuf, String> {
            self.toplevel.get(path).cloned().ok_or_else(|| "not a repo".into())
        }
        fn in_worktree(&self, path: &Path) -> bool {
            self.worktrees.contains(path)
        }
        fn worktree_add(&self, _repo: &Path, path: &Path, _b: &str, _base: &str) -> Result<(), String> {
            self.added.lock().unwrap().push(path.to_path_buf());
            Ok(())
        }
    }

    #[test]
    fn fake_git_records_worktree_add() {
        let git = FakeGit::default();
        git.worktree_add(Path::new("/repo"), Path::new("/repo/.worktrees/x"), "x", "main").unwrap();
        assert_eq!(git.added.lock().unwrap().len(), 1);
    }
}
```

注：`FakeGit` 放在 `git_ops` 的 `#[cfg(test)] mod tests` 内并 `pub`，供同 crate 其它测试模块 `use super::git_ops::tests::FakeGit`。若 Rust 可见性受限，改为 `#[cfg(test)] pub mod test_support { pub struct FakeGit ... }` 暴露。

- [ ] **Step 2A.2: 注册模块并确认失败**

在 `agent/features/project/src/business.rs` 加：`mod git_ops;`（暂不 pub）。
Run: `cargo test -p project fake_git_records_worktree_add`
Expected: PASS（说明 trait+fake 编译通过；此步实际是建立基线）。

- [ ] **Step 2A.3: 实现 GitCli（搬移现有内联 git）**

在 `git_ops.rs` 的 trait 下方加入 `GitCli`，逻辑搬自 `worktree.rs` 的 `get_git_common_dir`/`in_worktree`/`create_worktree` 的 git 部分与 `working_paths.rs` 的 `detect_working_root`：

```rust
use std::process::Command;

/// Production git adapter. Spawns the `git` CLI (project may spawn; share may not).
pub struct GitCli;

impl GitWorktreeOps for GitCli {
    fn git_common_dir(&self, path: &Path) -> Result<PathBuf, String> {
        let output = Command::new("git")
            .args(["rev-parse", "--git-common-dir"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("git rev-parse --git-common-dir 执行失败: {}", e))?;
        if !output.status.success() {
            return Err("无法获取 git common dir".to_string());
        }
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let p = PathBuf::from(&s);
        if p.is_absolute() {
            Ok(p.canonicalize().unwrap_or(p))
        } else {
            Ok(path.join(&s).canonicalize().unwrap_or_else(|_| path.join(&s)))
        }
    }

    fn show_toplevel(&self, path: &Path) -> Result<PathBuf, String> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("git rev-parse 执行失败: {}", e))?;
        if !output.status.success() {
            return Err(format!("路径 {} 不是 git 仓库或 worktree", path.display()));
        }
        Ok(PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string()))
    }

    fn in_worktree(&self, path: &Path) -> bool {
        Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(path)
            .output()
            .ok()
            .and_then(|o| {
                o.status.success().then(|| {
                    String::from_utf8_lossy(&o.stdout).trim().contains("/.git/worktrees/")
                })
            })
            .unwrap_or(false)
    }

    fn worktree_add(&self, repo_root: &Path, path: &Path, branch: &str, base: &str)
        -> Result<(), String>
    {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建 worktree 父目录失败 {}: {}", parent.display(), e))?;
        }
        let output = Command::new("git")
            .args(["worktree", "add"])
            .arg(path)
            .args(["-b", branch, base])
            .current_dir(repo_root)
            .output()
            .map_err(|e| format!("git worktree add 执行失败: {}", e))?;
        if !output.status.success() {
            return Err(format!(
                "创建 worktree 失败：git worktree add {} -b {} {}\nstdout: {}\nstderr: {}",
                path.display(), branch, base,
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(())
    }
}
```

- [ ] **Step 2A.4: 旧代码委托 GitCli（DRY）**

- `working_paths.rs`：把 `detect_working_root` 内联 git 改为 `crate::business::git_ops::GitCli.show_toplevel(path).unwrap_or_else(|_| path.to_path_buf())`。
- `worktree.rs`：`get_git_common_dir`/`is_same_git_repo`/`in_worktree`/`create_worktree` 改为调用 `GitCli`（保留对外签名）。例如 `get_git_common_dir(path)` body 改为 `GitCli.git_common_dir(path)`。

Run: `cargo test -p project`
Expected: PASS（现有 worktree 测试仍过，证明委托等价）。

- [ ] **Step 2A.5: 提交**

```bash
git add agent/features/project/src/business.rs agent/features/project/src/business/git_ops.rs agent/features/project/src/business/worktree.rs agent/features/project/src/business/working_paths.rs
git commit -m "feat(project): 抽 GitWorktreeOps outbound port + GitCli，旧内联 git 委托之"
```

### Task 2B: 能力 trait + WorkspaceError（project contract）

**Files:** Modify `agent/features/project/src/contract.rs`

- [ ] **Step 2B.1: 定义 WorkspaceFrame / WorkspaceError / 三 trait**

在 `contract.rs` 加入：

```rust
use std::path::{Path, PathBuf};
use share::session_types::PersistedWorkspaceContext;

/// Runtime worktree stack frame（替代 share::tool::WorkingContext）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFrame {
    pub path_base: PathBuf,
    pub working_root: PathBuf,
}

/// Workspace 层集中错误（用户可见消息为中文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceError {
    PathNotFound(PathBuf),
    MissingPathAndBranch,
    InvalidBranch,
    NestedWorktree,
    RepoMismatch { path: PathBuf, repo_root: PathBuf },
    EmptyStack,
    RestoreInvalidPath(PathBuf),
    Git(String),
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceError::PathNotFound(p) => write!(f, "路径不存在或无法访问 {}", p.display()),
            WorkspaceError::MissingPathAndBranch => write!(f, "进入或创建 worktree 时必须提供 path 或 branch"),
            WorkspaceError::InvalidBranch => write!(f, "branch 不能只包含路径分隔符或敏感字符"),
            WorkspaceError::NestedWorktree => write!(f, "已在 worktree 中，请先 ExitWorktree 退出当前 worktree 再进入新的"),
            WorkspaceError::RepoMismatch { path, repo_root } =>
                write!(f, "路径 {} 不属于当前仓库（当前仓库根: {}）", path.display(), repo_root.display()),
            WorkspaceError::EmptyStack => write!(f, "上下文栈为空，没有可恢复的 worktree。可能已经在主工作区。"),
            WorkspaceError::RestoreInvalidPath(p) => write!(f, "恢复工作区失败：路径不存在 {}", p.display()),
            WorkspaceError::Git(m) => write!(f, "{}", m),
        }
    }
}
impl std::error::Error for WorkspaceError {}

/// 读当前 workspace 位置（所有 tool 可用）。
pub trait WorkspaceRead: Send + Sync {
    fn current_root(&self) -> PathBuf;
    fn current_path_base(&self) -> PathBuf;
    fn resolve(&self, rel: &Path) -> PathBuf;
}

/// 运行期 workspace 变更（bash cd + worktree enter/exit）。
pub trait WorkspaceControl: Send + Sync {
    fn set_cwd(&self, path: PathBuf) -> Result<(), WorkspaceError>;
    fn enter(&self, path: Option<PathBuf>, branch: Option<String>)
        -> Result<WorkspaceFrame, WorkspaceError>;
    fn exit(&self) -> Result<WorkspaceFrame, WorkspaceError>;
}

/// session 边界持久化。
pub trait WorkspacePersist: Send + Sync {
    fn snapshot(&self) -> PersistedWorkspaceContext;
    fn restore(&self, dto: &PersistedWorkspaceContext) -> Result<(), WorkspaceError>;
}
```

- [ ] **Step 2B.2: 确认编译**

Run: `cargo check -p project`
Expected: PASS。

- [ ] **Step 2B.3: 提交**

```bash
git add agent/features/project/src/contract.rs
git commit -m "feat(project): 定义 WorkspaceRead/WorkspaceControl/WorkspacePersist + WorkspaceError + WorkspaceFrame"
```

### Task 2C: WorkspaceState + 纯转换规则（TDD）

**Files:** Create `agent/features/project/src/business/workspace_state.rs`；Modify `business.rs`（`mod workspace_state;`）

- [ ] **Step 2C.1: 写失败测试（init / resolve / set_cwd / enter / exit / snapshot / restore）**

Create `workspace_state.rs`：

```rust
use std::path::{Path, PathBuf};

use share::session_types::{PersistedWorkspaceContext, PersistedWorkspaceFrame};

use crate::business::git_ops::GitWorktreeOps;
use crate::contract::{WorkspaceError, WorkspaceFrame};

const DEFAULT_WORKTREE_BASE: &str = "main";
const DEFAULT_WORKTREE_DIR: &str = ".worktrees";

pub struct WorkspaceState {
    pub initial_cwd: PathBuf,
    pub working_root: PathBuf,
    pub path_base: PathBuf,
    pub stack: Vec<WorkspaceFrame>,
}

impl WorkspaceState {
    pub fn new(cwd: PathBuf) -> Self {
        Self { initial_cwd: cwd.clone(), working_root: cwd.clone(), path_base: cwd, stack: Vec::new() }
    }
    pub fn resolve(&self, rel: &Path) -> PathBuf {
        if rel.is_absolute() { rel.to_path_buf() } else { self.path_base.join(rel) }
    }
}

fn sanitize_branch_for_path(branch: &str) -> Result<String, WorkspaceError> {
    let mut s = String::new();
    let mut last_dash = false;
    for ch in branch.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            s.push(ch); last_dash = false;
        } else if !last_dash { s.push('-'); last_dash = true; }
    }
    let s = s.trim_matches(|c| matches!(c, '.' | '_' | '-')).to_string();
    if s.is_empty() { return Err(WorkspaceError::InvalidBranch); }
    Ok(s)
}

fn resolve_worktree_path(state: &WorkspaceState, path: Option<PathBuf>, branch: Option<&str>)
    -> Result<PathBuf, WorkspaceError>
{
    match path {
        Some(p) if p.is_absolute() => Ok(p),
        Some(p) => Ok(state.path_base.join(p)),
        None => match branch {
            Some(b) if !b.trim().is_empty() =>
                Ok(state.path_base.join(DEFAULT_WORKTREE_DIR).join(sanitize_branch_for_path(b)?)),
            _ => Err(WorkspaceError::MissingPathAndBranch),
        },
    }
}

pub fn set_cwd(state: &mut WorkspaceState, git: &dyn GitWorktreeOps, path: PathBuf)
    -> Result<(), WorkspaceError>
{
    let root = git.show_toplevel(&path).map(PathBuf::from).unwrap_or_else(|_| path.clone());
    state.working_root = root;
    state.path_base = path;
    Ok(())
}

pub fn enter(state: &mut WorkspaceState, git: &dyn GitWorktreeOps, path: Option<PathBuf>, branch: Option<String>)
    -> Result<WorkspaceFrame, WorkspaceError>
{
    if !state.stack.is_empty() {
        if !git.in_worktree(&state.path_base) {
            state.stack.clear(); // 残栈自愈（refs #96）
        } else {
            return Err(WorkspaceError::NestedWorktree);
        }
    }
    let target = resolve_worktree_path(state, path, branch.as_deref())?;
    if !target.exists() {
        let b = branch.filter(|v| !v.trim().is_empty()).ok_or(WorkspaceError::MissingPathAndBranch)?;
        git.worktree_add(&state.working_root, &target, &b, DEFAULT_WORKTREE_BASE)
            .map_err(WorkspaceError::Git)?;
    }
    let canonical = target.canonicalize().map_err(|_| WorkspaceError::PathNotFound(target.clone()))?;
    let worktree_root = git.show_toplevel(&canonical).map(PathBuf::from).map_err(WorkspaceError::Git)?;
    if let Ok(a) = git.git_common_dir(&state.working_root) {
        if let Ok(b) = git.git_common_dir(&worktree_root) {
            if a != b {
                return Err(WorkspaceError::RepoMismatch { path: worktree_root, repo_root: state.working_root.clone() });
            }
        }
    }
    let frame = WorkspaceFrame { path_base: state.path_base.clone(), working_root: state.working_root.clone() };
    state.stack.push(frame.clone());
    set_cwd(state, git, canonical)?;
    Ok(frame)
}

pub fn exit(state: &mut WorkspaceState) -> Result<WorkspaceFrame, WorkspaceError> {
    match state.stack.pop() {
        Some(prev) => {
            state.working_root = prev.working_root.clone();
            state.path_base = prev.path_base.clone();
            Ok(prev)
        }
        None => Err(WorkspaceError::EmptyStack),
    }
}

pub fn snapshot(state: &WorkspaceState) -> PersistedWorkspaceContext {
    PersistedWorkspaceContext {
        path_base: state.path_base.display().to_string(),
        working_root: state.working_root.display().to_string(),
        context_stack: state.stack.iter().map(|f| PersistedWorkspaceFrame {
            path_base: f.path_base.display().to_string(),
            working_root: f.working_root.display().to_string(),
        }).collect(),
    }
}

pub fn restore(state: &mut WorkspaceState, dto: &PersistedWorkspaceContext) -> Result<(), WorkspaceError> {
    let path_base = PathBuf::from(&dto.path_base);
    let working_root = PathBuf::from(&dto.working_root);
    if !path_base.exists() { return Err(WorkspaceError::RestoreInvalidPath(path_base)); }
    if !working_root.exists() { return Err(WorkspaceError::RestoreInvalidPath(working_root)); }
    let stack = dto.context_stack.iter().map(|e| WorkspaceFrame {
        path_base: PathBuf::from(&e.path_base),
        working_root: PathBuf::from(&e.working_root),
    }).collect();
    state.path_base = path_base;
    state.working_root = working_root;
    state.stack = stack;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::git_ops::tests::FakeGit;

    fn st(cwd: &str) -> WorkspaceState { WorkspaceState::new(PathBuf::from(cwd)) }

    #[test]
    fn init_consistent() {
        let s = st("/repo");
        assert_eq!(s.working_root, PathBuf::from("/repo"));
        assert_eq!(s.path_base, PathBuf::from("/repo"));
        assert!(s.stack.is_empty());
    }

    #[test]
    fn resolve_relative_uses_path_base() {
        let mut s = st("/repo");
        s.path_base = PathBuf::from("/repo/sub");
        assert_eq!(s.resolve(Path::new("a/b.rs")), PathBuf::from("/repo/sub/a/b.rs"));
        assert_eq!(s.resolve(Path::new("/abs/x")), PathBuf::from("/abs/x"));
    }

    #[test]
    fn exit_empty_stack_errors() {
        let mut s = st("/repo");
        assert_eq!(exit(&mut s), Err(WorkspaceError::EmptyStack));
    }

    #[test]
    fn exit_pops_and_restores() {
        let mut s = st("/repo");
        s.stack.push(WorkspaceFrame { path_base: "/prev".into(), working_root: "/prev".into() });
        s.path_base = "/wt".into(); s.working_root = "/wt".into();
        let prev = exit(&mut s).unwrap();
        assert_eq!(prev.path_base, PathBuf::from("/prev"));
        assert_eq!(s.path_base, PathBuf::from("/prev"));
    }

    #[test]
    fn set_cwd_detects_root() {
        let mut git = FakeGit::default();
        git.toplevel.insert(PathBuf::from("/repo/sub"), PathBuf::from("/repo"));
        let mut s = st("/repo");
        set_cwd(&mut s, &git, PathBuf::from("/repo/sub")).unwrap();
        assert_eq!(s.path_base, PathBuf::from("/repo/sub"));
        assert_eq!(s.working_root, PathBuf::from("/repo"));
    }

    #[test]
    fn snapshot_restore_roundtrip() {
        let mut s = st("/repo");
        s.path_base = "/repo/sub".into();
        s.stack.push(WorkspaceFrame { path_base: "/repo".into(), working_root: "/repo".into() });
        let dto = snapshot(&s);
        let mut s2 = st("/tmp");
        // restore 校验路径存在：用真实存在的临时目录替换
        let dir = std::env::temp_dir();
        let dto2 = PersistedWorkspaceContext {
            path_base: dir.display().to_string(),
            working_root: dir.display().to_string(),
            context_stack: dto.context_stack.clone(),
        };
        restore(&mut s2, &dto2).unwrap();
        assert_eq!(s2.path_base, dir);
        assert_eq!(s2.stack.len(), 1);
    }

    #[test]
    fn restore_invalid_path_fails_whole() {
        let mut s = st("/repo");
        let bad = PersistedWorkspaceContext {
            path_base: "/definitely/not/here/xyz".into(),
            working_root: "/definitely/not/here/xyz".into(),
            context_stack: vec![],
        };
        assert!(matches!(restore(&mut s, &bad), Err(WorkspaceError::RestoreInvalidPath(_))));
        // 状态未被部分修改
        assert_eq!(s.path_base, PathBuf::from("/repo"));
    }

    #[test]
    fn enter_rejects_nested_when_in_worktree() {
        let mut git = FakeGit::default();
        git.worktrees.insert(PathBuf::from("/repo")); // 当前 path_base 在 worktree 中
        let mut s = st("/repo");
        s.stack.push(WorkspaceFrame { path_base: "/prev".into(), working_root: "/prev".into() });
        assert_eq!(enter(&mut s, &git, Some("/other".into()), None), Err(WorkspaceError::NestedWorktree));
    }

    #[test]
    fn enter_missing_path_and_branch_errors() {
        let git = FakeGit::default();
        let mut s = st("/repo");
        assert_eq!(enter(&mut s, &git, None, None), Err(WorkspaceError::MissingPathAndBranch));
    }
}
```

- [ ] **Step 2C.2: 注册模块并跑测试确认失败→实现已含→通过**

在 `business.rs` 加 `mod workspace_state;`。
Run: `cargo test -p project workspace_state`
Expected: PASS（上面实现与测试同文件提交；若有编译错按报错修正）。

- [ ] **Step 2C.3: 提交**

```bash
git add agent/features/project/src/business.rs agent/features/project/src/business/workspace_state.rs
git commit -m "feat(project): WorkspaceState 纯转换规则(enter/exit/set_cwd/snapshot/restore) + FakeGit 单测"
```

### Task 2D: WorkspaceService（单锁，含 seed_isolated）+ 实现三 trait

**Files:** Create `agent/features/project/src/business/workspace_service.rs`；Modify `business.rs`、`api.rs`

- [ ] **Step 2D.1: 写 service 与隔离测试**

Create `workspace_service.rs`：

```rust
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use share::session_types::PersistedWorkspaceContext;

use crate::business::git_ops::{GitCli, GitWorktreeOps};
use crate::business::workspace_state::{self as rules, WorkspaceState};
use crate::contract::{WorkspaceControl, WorkspaceError, WorkspaceFrame, WorkspacePersist, WorkspaceRead};

/// project 拥有的唯一可变 workspace 状态源（单锁）。
pub struct WorkspaceService {
    state: Mutex<WorkspaceState>,
    git: Arc<dyn GitWorktreeOps>,
}

impl WorkspaceService {
    pub fn new(cwd: PathBuf) -> Arc<Self> {
        Self::with_git(cwd, Arc::new(GitCli))
    }
    pub fn with_git(cwd: PathBuf, git: Arc<dyn GitWorktreeOps>) -> Arc<Self> {
        Arc::new(Self { state: Mutex::new(WorkspaceState::new(cwd)), git })
    }
    /// 从当前快照派生独立实例（继承 root/base、空栈、新锁），供子 agent。
    pub fn seed_isolated(&self) -> Arc<Self> {
        let s = self.lock();
        Arc::new(Self {
            state: Mutex::new(WorkspaceState {
                initial_cwd: s.initial_cwd.clone(),
                working_root: s.working_root.clone(),
                path_base: s.path_base.clone(),
                stack: Vec::new(),
            }),
            git: self.git.clone(),
        })
    }
    fn lock(&self) -> MutexGuard<'_, WorkspaceState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl WorkspaceRead for WorkspaceService {
    fn current_root(&self) -> PathBuf { self.lock().working_root.clone() }
    fn current_path_base(&self) -> PathBuf { self.lock().path_base.clone() }
    fn resolve(&self, rel: &Path) -> PathBuf { self.lock().resolve(rel) }
}

impl WorkspaceControl for WorkspaceService {
    fn set_cwd(&self, path: PathBuf) -> Result<(), WorkspaceError> {
        rules::set_cwd(&mut self.lock(), self.git.as_ref(), path)
    }
    fn enter(&self, path: Option<PathBuf>, branch: Option<String>)
        -> Result<WorkspaceFrame, WorkspaceError>
    {
        rules::enter(&mut self.lock(), self.git.as_ref(), path, branch)
    }
    fn exit(&self) -> Result<WorkspaceFrame, WorkspaceError> {
        rules::exit(&mut self.lock())
    }
}

impl WorkspacePersist for WorkspaceService {
    fn snapshot(&self) -> PersistedWorkspaceContext { rules::snapshot(&self.lock()) }
    fn restore(&self, dto: &PersistedWorkspaceContext) -> Result<(), WorkspaceError> {
        rules::restore(&mut self.lock(), dto)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::git_ops::tests::FakeGit;

    #[test]
    fn seed_isolated_inherits_position_empty_stack_independent_lock() {
        let parent = WorkspaceService::with_git(PathBuf::from("/repo"), Arc::new(FakeGit::default()));
        // 父进入一个伪 worktree 帧
        { let mut s = parent.lock(); s.path_base = "/wt".into(); s.working_root = "/wt".into();
          s.stack.push(WorkspaceFrame { path_base: "/repo".into(), working_root: "/repo".into() }); }
        let child = parent.seed_isolated();
        assert_eq!(child.current_path_base(), PathBuf::from("/wt")); // 继承当前
        // 子退栈应为空（独立空栈）
        assert_eq!(WorkspaceControl::exit(child.as_ref()), Err(WorkspaceError::EmptyStack));
        // 父仍有一帧（不受子影响）
        assert_eq!(parent.lock().stack.len(), 1);
    }
}
```

- [ ] **Step 2D.2: 注册模块、导出 API、跑测试**

- `business.rs`：加 `pub mod git_ops;`（改为 pub，使 api 能导出 GitWorktreeOps）、`pub mod workspace_state;`、`pub mod workspace_service;`（按需 pub）。
- `agent/features/project/src/api.rs`：在现有 `pub use crate::contract::*; pub use crate::gateway::*;` 之外追加：

```rust
pub use crate::business::git_ops::{GitCli, GitWorktreeOps};
pub use crate::business::workspace_service::WorkspaceService;
// contract::* 已带出 WorkspaceRead/WorkspaceControl/WorkspacePersist/WorkspaceError/WorkspaceFrame
```

Run: `cargo test -p project`
Expected: PASS（含隔离测试）。

- [ ] **Step 2D.3: 提交**

```bash
git add agent/features/project/src/business.rs agent/features/project/src/business/workspace_service.rs agent/features/project/src/api.rs
git commit -m "feat(project): WorkspaceService 单锁 + seed_isolated + 三 trait 实现，api 导出"
```

---

## Phase 3 — 枢轴:tools + runtime 切到 Arc<WorkspaceService>

> 中间态不可编译。按子步骤逐一改完后在 **Step 3.末** 统一 `cargo check`。每处给出 file:line 与 before/after。

**Files:**
- Modify: `agent/features/tools/src/contract/context.rs`、`agent/features/tools/src/contract.rs`、`agent/features/tools/src/contract/agent_port.rs`
- Modify: tools 路径消费者：`business/{file_read,file_edit,file_write,glob_tool,grep,lsp,bash,agent_tool}.rs`
- Modify: `agent/features/tools/src/business/worktree.rs`
- Modify: runtime：`core/client/{accessors,from_args,trait_chat,trait_session,trait_accessor,event}.rs`、`business/chat/looping/{loop_runner,tool_context,agent_calls}.rs`、`business/agent/runner/setup.rs`、`core/client/mapping.rs`
- Delete: `agent/features/runtime/src/business/chat/looping/tool_context.rs`（ToolContextParts/build_tool_context）

### Task 3A: 改 ToolContext → ToolExecutionContext（持 WorkspaceService 句柄）

- [ ] **Step 3A.1: 重写 context.rs**

把 `agent/features/tools/src/contract/context.rs` 整体替换为：

```rust
use super::AgentRunner;
use project::api::{WorkspaceControl, WorkspaceRead, WorkspaceService};
use share::tool::{AgentProgressEvent, SessionReminders};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ToolExecutionContext {
    /// Initial workspace root, kept for compatibility with existing callers.
    pub cwd: PathBuf,
    /// 唯一 workspace 状态源句柄（project 拥有）。
    pub workspace: Arc<WorkspaceService>,
    pub cancel: CancellationToken,
    pub read_files: Arc<Mutex<HashSet<String>>>,
    pub agent_runner: Option<Arc<dyn AgentRunner>>,
    pub session_reminders: Option<Arc<Mutex<SessionReminders>>>,
    pub memory_config: share::config::MemoryConfig,
    pub plan_mode: Option<bool>,
    pub allow_all: bool,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
    pub parent_session_id: Option<String>,
}

impl ToolExecutionContext {
    /// 只读 workspace 能力（所有 tool）。
    pub fn workspace_read(&self) -> &dyn WorkspaceRead { self.workspace.as_ref() }
    /// 变更 workspace 能力（仅 bash + worktree 工具；由 guard 约束调用方）。
    pub fn workspace_control(&self) -> &dyn WorkspaceControl { self.workspace.as_ref() }
}

/// 迁移期别名（P5 删除）。
pub type ToolContext = ToolExecutionContext;
```

- [ ] **Step 3A.2: 改 contract.rs：删 WorktreeContextExt，更新导出**

把 `agent/features/tools/src/contract.rs` 改为（删除 WorktreeContextExt 整块与 project 投影 import）：

```rust
//! Published language for the tools feature.

pub mod agent_port;
pub mod context;
pub mod tool;

pub use agent_port::{AgentRunRequest, AgentRunner};
pub use context::{ToolContext, ToolExecutionContext};
pub use share::tool::{AgentToolCallProgress, ImageData, ToolResult};
pub use tool::Tool;

pub use crate::business::mcp::{McpServerConfig, McpToolDef, McpTransportKind};
```

- [ ] **Step 3A.3: agent_port.rs 注释/类型同步**

`agent/features/tools/src/contract/agent_port.rs:4` `use super::context::ToolContext;` 经别名仍有效；保持不变。`AgentRunRequest.ctx: &'a ToolContext` 经别名等价于 `&ToolExecutionContext`，无需改。

### Task 3B: 改 tools 路径消费者(读)→ workspace_read()

> 每处把 `project::api::current_path(&ctx.path_base)` → `ctx.workspace_read().current_path_base()`；`project::api::current_path(&ctx.working_root)` → `ctx.workspace_read().current_root()`。

- [ ] **Step 3B.1: file_read.rs:42-43**

before:
```rust
let path_base = project::api::current_path(&ctx.path_base);
let working_root = project::api::current_path(&ctx.working_root);
```
after:
```rust
let path_base = ctx.workspace_read().current_path_base();
let working_root = ctx.workspace_read().current_root();
```

- [ ] **Step 3B.2: 同样改 file_edit.rs:39-40、file_write.rs:54-55、glob_tool.rs:39-40、grep.rs:41-42、lsp.rs:76-77**

每处 before/after 同 3B.1。

- [ ] **Step 3B.3: bash.rs:78（path_base 读）**

before: `let path_base = current_path(&ctx.path_base);`
after: `let path_base = ctx.workspace_read().current_path_base();`
（同时删除文件顶部对 `current_path` 的 `use`，改用 trait 方法。）

- [ ] **Step 3B.4: agent_tool.rs:88**

before: `let cwd = project::api::current_path(&ctx.path_base);`
after: `let cwd = ctx.workspace_read().current_path_base();`

### Task 3C: bash cd(写)→ workspace_control().set_cwd()

- [ ] **Step 3C.1: bash.rs:158**

before:
```rust
set_working_directory(&ctx.working_root, &ctx.path_base, new_path_base);
```
after:
```rust
if let Err(e) = ctx.workspace_control().set_cwd(new_path_base) {
    return ToolResult::error(e.to_string());
}
```
（删除对 `set_working_directory` 的 `use`；按需调整周边返回类型。）

### Task 3D: worktree 工具 → workspace_control()

- [ ] **Step 3D.1: 重写 worktree.rs 的 enter/exit 调用（114/177/195/181 行段）**

- 顶部：删 `use crate::api::WorktreeContextExt;` 与 `use ...worktree_ops`（project 内部 fn 不再直接调）。保留 `use crate::api::{Tool, ToolContext, ToolResult};`。
- EnterWorktree（原 114-119）：
```rust
match ctx.workspace_control().enter(args.path.as_ref().map(PathBuf::from), args.branch.clone()) {
    Ok(_frame) => {
        let path_base = ctx.workspace_read().current_path_base();
        let working_root = ctx.workspace_read().current_root();
        // ...原成功分支保持...
    }
    Err(e) => ToolResult::error(e.to_string()),
}
```
- ExitWorktree path 变体（原 177-180，"直接切到指定路径"）：语义＝进入指定路径。改为 `ctx.workspace_control().enter(Some(PathBuf::from(&path)), None)`；原先 `enter 后 pop 栈顶` 的 hack（181 `wc.context_stack.lock()...pop()`）**删除**——`enter` 已 push 一帧表示当前位置，符合"切到指定路径"语义；若产品语义需要"切换但不留栈帧"，改用 `set_cwd(PathBuf::from(&path))`（推荐，与 cd 一致，不污染栈）。**采用 `set_cwd`**：
```rust
match ctx.workspace_control().set_cwd(PathBuf::from(&path)) {
    Ok(()) => { /* 原成功分支 */ }
    Err(e) => ToolResult::error(e.to_string()),
}
```
- ExitWorktree stack 变体（原 195-197）：
```rust
match ctx.workspace_control().exit() {
    Ok(_frame) => { /* 原成功分支 */ }
    Err(e) => ToolResult::error(e.to_string()),
}
```

> 注：此处把"ExitWorktree 带 path = 直接切到指定路径"从 enter+pop hack 改为 `set_cwd`，行为更正确（不留多余栈帧）。执行者 **MUST** 在 Step 3.末用现有 worktree TUI/集成测试核对该语义；若原行为被测试固定，则保留 enter 语义并显式 `exit` 抵消。

### Task 3E: runtime client 持有 WorkspaceService（取代 inner.workspace_context）

- [ ] **Step 3E.1: accessors.rs:43 改字段**

before:
```rust
pub(crate) workspace_context: Arc<Mutex<Option<crate::business::session::WorkspaceContext>>>,
```
after:
```rust
pub(crate) workspace: Arc<project::api::WorkspaceService>,
```

- [ ] **Step 3E.2: from_args.rs:38-42 + 236 构造 service**

在 cwd 解析后构造（cwd 已知）；把 handle 字段 `workspace_context: Arc::new(Mutex::new(None))` 改为：
```rust
workspace: project::api::WorkspaceService::new(cwd.clone()),
```
（确保在 `let cwd = ...;` 之后、handle 构造处使用 `cwd.clone()`。）

- [ ] **Step 3E.3: trait_chat.rs:36 / 53 改传 service**

- sink 字段：`workspace_context: me.inner.workspace_context.clone()` → `workspace: me.inner.workspace.clone()`（SdkChatEventSink 字段类型同步改为 `Arc<project::api::WorkspaceService>`，见 event.rs）。
- 传入 ChatLoopContext：原 `workspace_context: inner.workspace_context.lock().ok().and_then(|g| g.clone())` → `workspace: inner.workspace.clone()`（ChatLoopContext 字段改名/改类型，见 3F）。

- [ ] **Step 3E.4: trait_session.rs 保存/恢复改走 snapshot/restore**

- 保存（37-42）：
```rust
let workspace = Some(project::api::WorkspacePersist::snapshot(me.inner.workspace.as_ref()));
```
（`session.workspace: Option<PersistedWorkspaceContext>` 类型不变，snapshot 直接产出该类型。）
- 恢复（97-101）：
```rust
if let Some(ref ws) = session.workspace {
    let _ = project::api::WorkspacePersist::restore(me.inner.workspace.as_ref(), ws);
}
```

- [ ] **Step 3E.5: trait_accessor.rs:52-57 改 snapshot**

before（读 Option）→ after：
```rust
let workspace = Some(project::api::WorkspacePersist::snapshot(me.inner.workspace.as_ref()));
```
（按下游 `project_impl` 期望类型调整：若期望 `Option<WorkspaceContext>`，snapshot 产出 `PersistedWorkspaceContext` 即兼容。）

- [ ] **Step 3E.6: event.rs:237-239 去掉 Option 回写**

`WorkingDirectoryChanged` 事件中原把 `workspace` 写回 `inner.workspace_context` 的逻辑删除——service 已是单一源，工具调用时已直接改它。事件仅用于 UI/SDK 通知:改为从 service 读取快照投影：
```rust
// 替换 *guard = Some(workspace.clone()); 整段
let _ = &sink_workspace; // service 已更新，无需回写
```
（若该事件需要把当前 workspace 投影给 UI，则 `let view = mapping::workspace_context_to_sdk(WorkspacePersist::snapshot(sink_workspace.as_ref()));` 并照原路径发出。执行者 **MUST** 核对 event.rs 上下文确定是否需要 view。）

### Task 3F: loop_runner 用 service 构建 ToolExecutionContext（删 seed 与 ToolContextParts）

- [ ] **Step 3F.1: ChatLoopContext.workspace_context 字段改为 service 句柄**

`loop_runner.rs:44`：
before: `pub workspace_context: Option<crate::business::session::WorkspaceContext>,`
after: `pub workspace: Arc<project::api::WorkspaceService>,`

- [ ] **Step 3F.2: 删除 98-123 的 seed 块**

把 `loop_runner.rs:98-123`（`let (cwd, working_root, path_base, context_stack) = if let Some(workspace) ... else { new_working_paths } ;`）整段删除。改为：
```rust
let workspace = ctx.workspace.clone();
let cwd = workspace.current_root();
```
（`cwd` 后续若用于日志/初始 root，用 `current_root()`。若有对 `working_root`/`path_base`/`context_stack` 局部变量的后续引用，全部改为经 `workspace` 读/控。）

- [ ] **Step 3F.3: 132-147 构造 ToolExecutionContext**

把 `ctx: build_tool_context(ToolContextParts { ... })` 改为直接构造：
```rust
ctx: tools::api::ToolExecutionContext {
    cwd: cwd.clone(),
    workspace: workspace.clone(),
    cancel: cancel.clone(),
    read_files: read_files.clone(),
    agent_runner: agent_runner.clone(),
    session_reminders: Some(session_reminders.clone()),
    memory_config: memory_config.clone(),
    plan_mode: None,
    allow_all,
    max_tool_concurrency,
    max_agent_concurrency,
    agent_semaphore,
    progress_tx: None,
    parent_session_id: Some(session_id.clone()),
},
```

- [ ] **Step 3F.4: 删除 tool_context.rs + 其 use**

- 删除文件 `agent/features/runtime/src/business/chat/looping/tool_context.rs`。
- `loop_runner.rs:12` 删除 `use ...tool_context::{build_tool_context, ToolContextParts};`。
- 其 mod 声明（`looping.rs` 或 `mod.rs` 中 `mod tool_context;`）删除。

### Task 3G: agent_calls.rs（task 快照）改走 snapshot/read

- [ ] **Step 3G.1: agent_calls.rs:113-115**

before:
```rust
let working_root = project::api::current_path(&ag_ctx.working_root);
...
let workspace = ag_ctx.workspace_context();
```
after:
```rust
let working_root = ag_ctx.workspace_read().current_root();
...
let workspace = project::api::WorkspacePersist::snapshot(ag_ctx.workspace.as_ref());
```
（`ag_ctx` 为 `ToolExecutionContext`；snapshot 产出 `PersistedWorkspaceContext`，与原 `workspace_context()` 产出同类型。）

### Task 3H: setup.rs 子 agent → seed_isolated（修隔离 bug）

- [ ] **Step 3H.1: setup.rs:165-184 重写 sub_ctx**

before（`Arc::clone` 父 working_root/path_base，共享可变状态）→ after：
```rust
let sub_ctx = ToolContext {
    cwd: ctx.cwd.clone(),
    workspace: ctx.workspace.seed_isolated(),
    cancel: ctx.cancel.clone(),
    read_files: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
    agent_runner: None,
    session_reminders: ctx.session_reminders.clone(),
    memory_config: ctx.memory_config.clone(),
    plan_mode: ctx.plan_mode,
    allow_all: ctx.allow_all,
    max_tool_concurrency: ctx.max_tool_concurrency,
    max_agent_concurrency: ctx.max_agent_concurrency,
    agent_semaphore: ctx.agent_semaphore.clone(), // 全局限流共享
    progress_tx: None,
    parent_session_id: ctx.parent_session_id.clone(),
};
```
（注:`session_reminders` 设计上应隔离，但当前父子共享;本阶段保持现状不扩大改动，隔离留作后续。**workspace** 与 **read_files** 已隔离。）

- [ ] **Step 3.末: 统一验证**

Run:
```bash
cargo fmt
cargo check
cargo test -p project -p tools -p runtime
```
Expected: 全部 PASS。逐一修复编译错误（多为遗漏的 `current_path`/字段访问点；用 `rg "\.working_root|\.path_base|\.context_stack|current_path\(&ctx|ToolContextParts|workspace_context|WorktreeContextExt" agent` 兜底搜剩余引用）。

- [ ] **Step 3.末.2: 写子 agent 隔离回归测试**

在 `agent/features/tools/src/business/agent_tool_tests.rs` 增（或 runtime 侧）测试：构造父 service→进入伪 worktree→`seed_isolated()`→子 `exit()` 报 EmptyStack 且父 stack 不变；断言父子 `workspace` 非同一 `Arc`（`Arc::ptr_eq` 为 false）。
Run: `cargo test -p tools sub_agent_workspace_isolated`
Expected: PASS。

- [ ] **Step 3.末.3: 提交**

```bash
git add -A
git commit -m "refactor(tools,runtime): ToolContext 切到 Arc<WorkspaceService>，删 ToolContextParts/WorktreeContextExt，子 agent seed_isolated 修隔离"
```

---

## Phase 4 — composition:移除 ProjectGateway

**Files:** Modify `agent/composition/src/app.rs`、`agent/composition/src/project.rs`；Modify `agent/features/project/src/gateway.rs`、`agent/features/project/src/api.rs`、`agent/features/project/src/lib.rs`

- [ ] **Step 4.1: 从 FeatureGateways 移除 project gateway**

`app.rs`：删 `use project::api::ProjectGateway;`、删 `FeatureGateways.project` 字段、`new()` 参数、`wire_default()` 中 `crate::project::wire_project()`。

- [ ] **Step 4.2: 删 composition/src/project.rs 及 lib.rs 声明**

删除文件 `agent/composition/src/project.rs`；`composition/src/lib.rs` 删 `pub mod project;`。

- [ ] **Step 4.3: 删 project 的 ProjectGateway/DefaultProjectGateway/wire_project**

`agent/features/project/src/gateway.rs`：删除 `ProjectGateway` trait、`DefaultProjectGateway`、`wire_project`（及其测试）。若 gateway.rs 清空，则删文件并在 `lib.rs` 去掉 `pub mod gateway;`、`api.rs` 去掉 `pub use crate::gateway::*;`。
> `new_working_paths`/`current_path`/`set_working_directory` 现仅经 `WorkspaceService` 使用；若仍有遗留外部调用，改走 service 或保留为 `business` 内部 fn。

- [ ] **Step 4.4: 验证**

Run: `cargo fmt && cargo check && cargo test -p project -p runtime`
Expected: PASS。

- [ ] **Step 4.5: 提交**

```bash
git add -A
git commit -m "refactor(composition,project): 移除 ProjectGateway/DefaultProjectGateway，WorkspaceService 取而代之"
```

---

## Phase 5 — 收尾:移 WorkingContext 出 share、删 shim、改/加 guard、全量验证

**Files:** Modify `agent/shared/src/tool.rs`、`agent/shared/src/session_types.rs`、`agent/features/tools/src/contract/context.rs`、`.agents/hooks/check-crate-api-boundary.sh`、新增 `.agents/hooks/check-context-architecture.sh`、`.agents/hooks/check-architecture-guards.sh`

- [ ] **Step 5.1: 删除迁移期别名**

- `agent/features/tools/src/contract/context.rs`：删 `pub type ToolContext = ToolExecutionContext;`。
- `agent/shared/src/session_types.rs`：删 `pub type WorkspaceContext = ...;`、`pub type WorkspaceStackEntry = ...;`。
- 全仓搜 `ToolContext`（非 Execution）与 `WorkspaceContext`（非 Persisted）残留并改名：
  Run: `rg -n "\bToolContext\b|\bWorkspaceContext\b|\bWorkspaceStackEntry\b" agent packages apps`
  逐处改为 `ToolExecutionContext` / `PersistedWorkspaceContext` / `PersistedWorkspaceFrame`。

- [ ] **Step 5.2: 移 WorkingContext 出 share**

确认无引用后删除 `agent/shared/src/tool.rs:4-8` 的 `WorkingContext`。
Run: `rg -n "WorkingContext" agent packages apps`
Expected: 仅 `project::contract::WorkspaceFrame` 相关（不同名）；若仍有 `share::tool::WorkingContext` 引用，改为 `project::api::WorkspaceFrame`。

- [ ] **Step 5.3: 删除 project 旧 worktree.rs 过时函数**

删除 `WorktreeWorkingContext`、`enter_worktree`/`exit_worktree`/`workspace_context_from_worktree_context`/`restore_workspace_context`（已被 WorkspaceState 规则取代）。保留仍被 GitCli 复用的纯 helper（若有）或一并移入 workspace_state。更新 `api.rs` 去掉对应再导出。
Run: `cargo check`
Expected: PASS。

- [ ] **Step 5.4: 更新 check-crate-api-boundary.sh（删 WorktreeContextExt 豁免）**

删除 `.agents/hooks/check-crate-api-boundary.sh` 中 `TOOLS_PROJECT_CONTEXT_API_NAMES`、`TOOLS_CONTEXT_PROJECTION_PATH`、`check_tools_project_context_api_line`（行 37-42 / 159-188 及其调用点）。

- [ ] **Step 5.5: 新增 context 架构 guard 脚本**

Create `.agents/hooks/check-context-architecture.sh`，检查（diff 命中 runtime/tools/project/share session_types.rs/tool.rs 时运行）：
1. `ToolExecutionContext` 定义不得含 `working_root`/`path_base`/`context_stack` 字段。
2. `agent/features/tools/` 下不得出现 `PersistedWorkspaceContext`。
3. `WorkspaceState` 仅在 `agent/features/project/` 定义；其它 crate 不得出现 `struct` 同时含 `working_root` 与 `path_base` 与 `context_stack`/`stack` 字段。
4. `WorkspaceControl` 仅被 `tools/src/business/{bash,worktree}.rs` 调用;`WorkspacePersist` 仅被 `runtime/src/core/client/` 调用。
5. `Command::new("git")` 仅出现在 `agent/features/project/src/business/git_ops.rs`。
失败信息须含违反条目与建议修复方向。脚本实现可仿照现有 `check-share-minimal-kernel.sh` 的 python 内嵌正则风格。

- [ ] **Step 5.6: 接入主 guard**

`.agents/hooks/check-architecture-guards.sh`：在子脚本调用列表追加 `"$HOOKS_DIR/check-context-architecture.sh"`。

- [ ] **Step 5.7: 全量验证**

Run:
```bash
cargo fmt
cargo check
cargo clippy --workspace -- -D warnings
cargo test --workspace
bash .agents/hooks/check-architecture-guards.sh
```
Expected: 全部 PASS;guard 全绿。

- [ ] **Step 5.8: 提交**

```bash
git add -A
git commit -m "refactor: 删迁移别名+旧 worktree API，移 WorkingContext 出 share，新增 context 架构 guard"
```

---

## 合并回 main（执行验证全过后）

- [ ] 在 main 工作区 `git pull --ff-only`；如有更新，在 worktree 分支 rebase/merge 最新 main 并重跑 Phase 5 全量验证。
- [ ] 合并：`git merge --no-ff design/agent-context-ownership-redesign`（文档+代码一并入 main）。
- [ ] main 上重跑 `cargo test --workspace && cargo clippy --workspace -- -D warnings && bash .agents/hooks/check-architecture-guards.sh`。
- [ ] 清理：`git worktree remove .worktrees/docs-context-redesign`、`git branch -d design/agent-context-ownership-redesign`。

---

## Self-Review(对照 spec)

- **spec 覆盖**:share 重命名(P1)、project 三 trait+State+Service+Git port(P2)、ToolExecutionContext 去三字段(P3A)、路径消费者改 read(P3B)、bash cd→set_cwd(P3C)、worktree→control(P3D)、runtime 跨轮持有+取代 inner.workspace_context+snapshot/restore(P3E/F)、agent_calls(P3G)、子 agent seed_isolated(P3H)、composition 去 ProjectGateway(P4)、移 WorkingContext+删别名+guard(P5)。spec 各节均有对应任务。
- **占位符**:无 TBD;两处显式标注执行者 MUST 核对(ExitWorktree path 语义、event.rs view 需求)——属真实需读现场上下文,非空泛占位。
- **类型一致**:`ToolExecutionContext`、`WorkspaceService`、`WorkspaceRead/Control/Persist`、`PersistedWorkspaceContext`、`WorkspaceFrame`、`GitWorktreeOps`/`GitCli`/`FakeGit` 全程同名。
- **风险点**:P3 为不可分原子切换,已列穷举改点 + 兜底 `rg` 搜索;`session.workspace` 类型 `Option<PersistedWorkspaceContext>` 经 P1 别名/重命名保持 serde 兼容。
