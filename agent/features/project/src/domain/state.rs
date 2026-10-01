#[cfg(test)]
use super::constants::DEFAULT_WORKTREE_DIR;
use super::constants::{DEFAULT_WORKTREE_BASE, UNNAMED_WORKSPACE_SEGMENT};
use std::path::{Path, PathBuf};

use share::session_types::{
    PersistedWorkspaceContext, PersistedWorkspaceFrame, ProjectIdentityData, WorkspaceId,
    WorktreeKind,
};

use crate::domain::git::{GitWorktreeOps, RepositoryProbe};
use crate::domain::types::{GitProbeError, WorkspaceData, WorkspaceError, WorkspaceRestoreError};

/// Workspace 聚合根：字段全私有，不变量（containment / git 位置 / 栈形态）
/// 只能经本 impl 的方法修改。生产代码 NEVER 直接读写字段；白盒测试位于
/// 子模块 `state_tests`，允许构造非法形态以验证校验逻辑本身。
#[derive(Clone)]
pub struct WorkspaceState {
    project_identity: ProjectIdentityData,
    workspace_root: PathBuf,
    path_base: PathBuf,
    worktree_kind: WorktreeKind,
    /// 已解析的 worktree 默认创建根目录（生产由 wiring 注入配置值）。
    worktrees_root: PathBuf,
    stack: Vec<WorkspaceData>,
}

impl WorkspaceState {
    /// Test constructor.
    #[cfg(test)]
    pub fn new(cwd: PathBuf) -> Self {
        Self::from_verified(
            ProjectIdentityData {
                initial_cwd: cwd.display().to_string(),
                git_common_dir: Some(cwd.join(".git").display().to_string()),
            },
            cwd.clone(),
            cwd.clone(),
            WorktreeKind::Primary,
            cwd.join(DEFAULT_WORKTREE_DIR),
        )
    }

    pub fn from_verified(
        project_identity: ProjectIdentityData,
        workspace_root: PathBuf,
        path_base: PathBuf,
        worktree_kind: WorktreeKind,
        worktrees_root: PathBuf,
    ) -> Self {
        Self {
            project_identity,
            workspace_root,
            path_base,
            worktree_kind,
            worktrees_root,
            stack: Vec::new(),
        }
    }

    /// 从当前状态派生隔离实例：继承身份、root、base、kind 与 worktree 根，
    /// 清空栈。唯一的跨实例派生入口（子 agent 种子）。
    pub fn derive_isolated(&self) -> Self {
        Self {
            project_identity: self.project_identity.clone(),
            workspace_root: self.workspace_root.clone(),
            path_base: self.path_base.clone(),
            worktree_kind: self.worktree_kind,
            worktrees_root: self.worktrees_root.clone(),
            stack: Vec::new(),
        }
    }

    // ── 只读访问面（service 读点专用；跨 crate 观测一律走窄 trait）──

    pub fn project_identity(&self) -> &ProjectIdentityData {
        &self.project_identity
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn path_base(&self) -> &Path {
        &self.path_base
    }

    pub fn worktree_kind(&self) -> WorktreeKind {
        self.worktree_kind
    }

    pub fn workspace_id(&self) -> WorkspaceId {
        WorkspaceId::derive(
            &self.project_identity,
            &self.workspace_root.display().to_string(),
        )
    }
    pub fn resolve(&self, rel: &Path) -> PathBuf {
        if rel.is_absolute() {
            rel.to_path_buf()
        } else {
            self.path_base.join(rel)
        }
    }

    // ── 白盒测试构造面（仅测校验逻辑时构造特定形态）──

    /// 强制设置位置（绕过校验），用于构造异常形态被测方。
    #[cfg(test)]
    pub fn force_position(&mut self, path_base: PathBuf, workspace_root: PathBuf) {
        self.path_base = path_base;
        self.workspace_root = workspace_root;
    }

    /// 直接压栈一帧（绕过 git 校验），用于构造栈形态被测方。
    #[cfg(test)]
    pub fn push_frame(&mut self, frame: WorkspaceData) {
        self.stack.push(frame);
    }

    #[cfg(test)]
    pub fn stack_depth(&self) -> usize {
        self.stack.len()
    }
}

fn sanitize_branch_for_path(branch: &str) -> Result<String, WorkspaceError> {
    let mut s = String::new();
    let mut last_dash = false;
    for ch in branch.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            s.push(ch);
            last_dash = false;
        } else if !last_dash {
            s.push('-');
            last_dash = true;
        }
    }
    let s = s.trim_matches(|c| matches!(c, '.' | '_' | '-')).to_string();
    if s.is_empty() {
        return Err(WorkspaceError::InvalidBranch);
    }
    Ok(s)
}

/// 按 worktree 根目录下的隔离段对仓库命名：取 workspace_root 的目录名并做与
/// 分支相同的字符白名单清洗，避免不同仓库的同名分支在共享根目录下冲突。
fn workspace_repo_segment(state: &WorkspaceState) -> Result<String, WorkspaceError> {
    let repo_dir_name = state
        .workspace_root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| UNNAMED_WORKSPACE_SEGMENT.to_string());
    sanitize_branch_for_path(&repo_dir_name)
}

fn resolve_worktree_path(
    state: &WorkspaceState,
    path: Option<PathBuf>,
    branch: Option<&str>,
) -> Result<PathBuf, share::error::DomainError> {
    match path.filter(|value| !value.as_os_str().is_empty()) {
        Some(p) if p.is_absolute() => Ok(p),
        Some(p) => Ok(state.path_base.join(p)),
        None => match branch {
            Some(b) if !b.trim().is_empty() => Ok(state
                .worktrees_root
                .join(workspace_repo_segment(state)?)
                .join(sanitize_branch_for_path(b)?)),
            _ => Err(WorkspaceError::MissingPathAndBranch.into()),
        },
    }
}

fn resolve_worktree_base(base: Option<&str>) -> &str {
    base.filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_WORKTREE_BASE)
}

impl WorkspaceState {
    /// 变更当前工作目录（path_base），必须位于 workspace root 内。
    pub fn change_directory(&mut self, path: PathBuf) -> Result<(), share::error::DomainError> {
        let canonical = path
            .canonicalize()
            .map_err(|_| WorkspaceError::PathNotFound(path.clone()))?;
        if !canonical.is_dir() {
            return Err(WorkspaceError::NotDirectory(canonical).into());
        }
        let canonical_root = self
            .workspace_root
            .canonicalize()
            .map_err(|_| WorkspaceError::PathNotFound(self.workspace_root.clone()))?;
        if !canonical.starts_with(&canonical_root) {
            return Err(WorkspaceError::PathOutsideWorkspaceRoot {
                path: canonical,
                root: canonical_root,
            }
            .into());
        }
        self.path_base = canonical;
        Ok(())
    }
}

/// Canonicalize `target` and verify it lives in the same repo as `state.workspace_root`.
/// Returns `(canonical_path, worktree_root, actual_kind)` on success.
fn validate_in_repo(
    state: &WorkspaceState,
    git: &dyn GitWorktreeOps,
    target: &Path,
) -> Result<(PathBuf, PathBuf, WorktreeKind), WorkspaceError> {
    let canonical = target
        .canonicalize()
        .map_err(|_| WorkspaceError::PathNotFound(target.to_path_buf()))?;
    if !canonical.is_dir() {
        return Err(WorkspaceError::NotDirectory(canonical));
    }
    let worktree_root = git
        .show_toplevel(&canonical)
        .map_err(WorkspaceError::GitOperationFailed)?;
    let probe = git
        .probe_repository(&canonical)
        .map_err(WorkspaceError::GitProbeFailed)?;
    match probe {
        RepositoryProbe::Git {
            canonical_top_level,
            canonical_common_dir,
            worktree_kind,
        } => {
            if !state
                .project_identity
                .git_common_dir
                .as_deref()
                .is_some_and(|expected| canonical_common_dir == Path::new(expected))
            {
                return Err(WorkspaceError::RepoMismatch {
                    path: worktree_root,
                    repo_root: state.workspace_root.clone(),
                });
            }
            if canonical_top_level != worktree_root {
                return Err(WorkspaceError::GitProbeFailed(GitProbeError::InvalidOutput));
            }
            Ok((canonical, worktree_root, worktree_kind))
        }
        RepositoryProbe::NonGit => {
            Err(WorkspaceError::GitProbeFailed(GitProbeError::InvalidOutput))
        }
    }
}

impl WorkspaceState {
    /// 进入（必要时创建）linked worktree，压栈前一帧。
    pub fn enter(
        &mut self,
        git: &dyn GitWorktreeOps,
        path: Option<PathBuf>,
        branch: Option<String>,
        base: Option<String>,
    ) -> Result<WorkspaceData, share::error::DomainError> {
        if self.worktree_kind == WorktreeKind::NonGit {
            return Err(WorkspaceError::UnsupportedForNonGit.into());
        }
        let mut next_stack = self.stack.clone();
        if !next_stack.is_empty() {
            match git
                .is_linked_worktree(&self.path_base)
                .map_err(WorkspaceError::GitOperationFailed)?
            {
                false => next_stack.clear(),
                true => {
                    return Err(WorkspaceError::NestedWorktree {
                        current_workspace_root: self.workspace_root.clone(),
                        current_path_base: self.path_base.clone(),
                    }
                    .into());
                }
            }
        }
        let target = resolve_worktree_path(self, path, branch.as_deref())?;
        if !target.exists() {
            let b = branch
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .ok_or(WorkspaceError::MissingPathAndBranch)?;
            git.worktree_add(
                &self.workspace_root,
                &target,
                b,
                resolve_worktree_base(base.as_deref()),
            )
            .map_err(WorkspaceError::GitOperationFailed)?;
        }
        let (canonical, worktree_root, worktree_kind) = validate_in_repo(self, git, &target)?;
        if worktree_kind != WorktreeKind::Linked {
            return Err(WorkspaceError::NotLinkedWorktree { path: canonical }.into());
        }
        let frame = WorkspaceData {
            id: self.workspace_id(),
            path_base: self.path_base.clone(),
            workspace_root: self.workspace_root.clone(),
            worktree_kind: self.worktree_kind,
        };
        next_stack.push(frame.clone());
        self.stack = next_stack;
        self.workspace_root = worktree_root;
        self.path_base = canonical;
        self.worktree_kind = worktree_kind;
        Ok(frame)
    }

    /// 退出当前 worktree，弹栈并恢复上一帧（git 位置必须仍自洽）。
    pub fn exit(
        &mut self,
        git: &dyn GitWorktreeOps,
    ) -> Result<WorkspaceData, share::error::DomainError> {
        if self.worktree_kind == WorktreeKind::NonGit {
            return Err(WorkspaceError::UnsupportedForNonGit.into());
        }
        let prev = self
            .stack
            .last()
            .cloned()
            .ok_or(WorkspaceError::EmptyStack)?;
        let (canonical, worktree_root, worktree_kind) =
            validate_in_repo(self, git, &prev.path_base)?;
        if canonical != prev.path_base
            || worktree_root != prev.workspace_root
            || worktree_kind != prev.worktree_kind
        {
            return Err(WorkspaceError::GitProbeFailed(GitProbeError::InvalidOutput).into());
        }
        self.stack.pop();
        self.workspace_root = worktree_root;
        self.path_base = canonical;
        self.worktree_kind = worktree_kind;
        Ok(prev)
    }

    /// 会话持久化快照（Writer 面唯一出口）。
    pub fn snapshot(&self) -> PersistedWorkspaceContext {
        PersistedWorkspaceContext {
            workspace_id: self.workspace_id(),
            project_identity: self.project_identity.clone(),
            path_base: self.path_base.display().to_string(),
            workspace_root: self.workspace_root.display().to_string(),
            worktree_kind: self.worktree_kind,
            context_stack: self
                .stack
                .iter()
                .map(|f| PersistedWorkspaceFrame {
                    path_base: f.path_base.display().to_string(),
                    workspace_root: f.workspace_root.display().to_string(),
                    worktree_kind: f.worktree_kind,
                })
                .collect(),
        }
    }

    /// 用恢复令牌整体替换当前状态（一次性按值消费）。
    pub fn commit_restore(&mut self, prepared: WorkspaceRestoreData) {
        *self = prepared.candidate;
    }
}

#[must_use]
pub struct WorkspaceRestoreData {
    candidate: WorkspaceState,
}

impl WorkspaceRestoreData {
    pub fn project_identity(&self) -> &ProjectIdentityData {
        &self.candidate.project_identity
    }
}

impl std::fmt::Debug for WorkspaceRestoreData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceRestoreData")
            .finish_non_exhaustive()
    }
}

fn restore_path(raw: &str) -> Result<PathBuf, WorkspaceRestoreError> {
    let path = PathBuf::from(raw);
    if raw.is_empty() || !path.exists() || !path.is_dir() {
        return Err(WorkspaceRestoreError::PathNotFound {
            path: path.to_string_lossy().into_owned(),
        });
    }
    path.canonicalize()
        .map_err(|_| WorkspaceRestoreError::PathNotFound {
            path: path.to_string_lossy().into_owned(),
        })
}

fn validate_containment(path: &Path, root: &Path) -> Result<(), WorkspaceRestoreError> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(WorkspaceRestoreError::PathOutsideWorkspaceRoot {
            path: path.to_string_lossy().into_owned(),
            root: root.to_string_lossy().into_owned(),
        })
    }
}

fn probe_restore(
    git: &dyn GitWorktreeOps,
    path: &Path,
) -> Result<RepositoryProbe, WorkspaceRestoreError> {
    git.probe_repository(path)
        .map_err(WorkspaceRestoreError::GitProbeFailed)
}

fn validate_git_location(
    git: &dyn GitWorktreeOps,
    path: &Path,
    expected_root: Option<&Path>,
    expected_common: &Path,
    expected_kind: Option<WorktreeKind>,
) -> Result<(), WorkspaceRestoreError> {
    match probe_restore(git, path)? {
        RepositoryProbe::Git {
            canonical_top_level,
            canonical_common_dir,
            worktree_kind,
        } if canonical_common_dir == expected_common
            && expected_root.is_none_or(|root| canonical_top_level == root)
            && expected_kind.is_none_or(|kind| worktree_kind == kind) =>
        {
            Ok(())
        }
        _ => Err(WorkspaceRestoreError::RepositoryMismatch),
    }
}

/// restore 阶段一产物：已恢复并通过 containment 校验的路径 + 回退资格。
struct RestoredPaths {
    initial_cwd: PathBuf,
    workspace_root: PathBuf,
    path_base: PathBuf,
    /// path_base 是 cwd 语义（上次工作目录），不是会话身份：目录被外部替换为
    /// 嵌套仓库、删除或移动后，对无 worktree 历史的普通会话（Primary + 空栈）
    /// 回退 workspace_root，与「shell cwd 被删回退 HOME」同语义，不丢任何会话
    /// 数据；worktree 上下文损坏必须走显式 exit 协议，保持 fail-closed。
    path_base_fallback_eligible: bool,
}

/// 阶段一：恢复身份三路径并校验 containment（含 path_base 回退资格判定）。
fn restore_identity(
    dto: &PersistedWorkspaceContext,
) -> Result<RestoredPaths, WorkspaceRestoreError> {
    if dto.project_identity.initial_cwd.is_empty() {
        return Err(WorkspaceRestoreError::InvalidProjectIdentity);
    }

    let initial_cwd = restore_path(&dto.project_identity.initial_cwd)?;
    let workspace_root = restore_path(&dto.workspace_root)?;
    let path_base_fallback_eligible =
        dto.worktree_kind == WorktreeKind::Primary && dto.context_stack.is_empty();
    let path_base = match restore_path(&dto.path_base) {
        Ok(path) => path,
        Err(WorkspaceRestoreError::PathNotFound { path }) if path_base_fallback_eligible => {
            log::warn!(
                target: crate::LOG_TARGET,
                "workspace restore path_base fallback: persisted path_base={path} 不存在，回退 workspace_root={}",
                workspace_root.display()
            );
            workspace_root.clone()
        }
        Err(restore_error) => return Err(restore_error),
    };
    validate_containment(&path_base, &workspace_root)?;
    Ok(RestoredPaths {
        initial_cwd,
        workspace_root,
        path_base,
        path_base_fallback_eligible,
    })
}

/// 阶段二：从持久化栈构造 frame 序列（每帧校验 containment）。
fn restore_stack(
    dto: &PersistedWorkspaceContext,
) -> Result<Vec<WorkspaceData>, WorkspaceRestoreError> {
    let mut stack = Vec::with_capacity(dto.context_stack.len());
    for persisted in &dto.context_stack {
        let frame_root = restore_path(&persisted.workspace_root)?;
        let frame_base = restore_path(&persisted.path_base)?;
        validate_containment(&frame_base, &frame_root)?;
        stack.push(WorkspaceData {
            id: dto.workspace_id.clone(),
            path_base: frame_base,
            workspace_root: frame_root,
            worktree_kind: persisted.worktree_kind,
        });
    }
    Ok(stack)
}

/// 阶段三：校验 git 位置（common dir / toplevel / kind 三向一致），
/// 产出 canonical 身份；path_base 校验失败时按回退资格调整。
fn restore_git_location(
    git: &dyn GitWorktreeOps,
    dto: &PersistedWorkspaceContext,
    paths: &mut RestoredPaths,
    stack: &[WorkspaceData],
) -> Result<ProjectIdentityData, WorkspaceRestoreError> {
    match dto.project_identity.git_common_dir.as_deref() {
        Some(common) if !common.is_empty() => {
            let common = PathBuf::from(common);
            if !common.is_absolute()
                || dto.worktree_kind == WorktreeKind::NonGit
                || stack.len() > 1
                || stack
                    .iter()
                    .any(|frame| frame.worktree_kind != WorktreeKind::Primary)
                || (!stack.is_empty() && dto.worktree_kind != WorktreeKind::Linked)
            {
                return Err(WorkspaceRestoreError::InvalidStackShape);
            }

            validate_git_location(git, &paths.initial_cwd, None, &common, None)?;
            validate_git_location(
                git,
                &paths.workspace_root,
                Some(&paths.workspace_root),
                &common,
                Some(dto.worktree_kind),
            )?;
            // path_base 是 cwd 语义（上次工作目录），不是会话身份：目录被外部
            // 替换为嵌套仓库、删除或移动后，回退 workspace_root 与「shell cwd
            // 被删回退 HOME」同语义，不丢任何会话数据。仅对无 worktree 历史
            // 的普通会话（Primary + 空栈）放行回退；worktree 上下文损坏必须
            // 走显式 exit 协议，保持 fail-closed。
            if let Err(restore_error) = validate_git_location(
                git,
                &paths.path_base,
                Some(&paths.workspace_root),
                &common,
                Some(dto.worktree_kind),
            ) {
                if paths.path_base_fallback_eligible {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "workspace restore path_base fallback: persisted path_base={} 校验失败（{restore_error}），回退 workspace_root={}",
                        paths.path_base.display(),
                        paths.workspace_root.display()
                    );
                    paths.path_base = paths.workspace_root.clone();
                } else {
                    return Err(restore_error);
                }
            }
            for frame in stack {
                validate_git_location(
                    git,
                    &frame.workspace_root,
                    Some(&frame.workspace_root),
                    &common,
                    Some(frame.worktree_kind),
                )?;
                validate_git_location(
                    git,
                    &frame.path_base,
                    Some(&frame.workspace_root),
                    &common,
                    Some(frame.worktree_kind),
                )?;
            }
            Ok(ProjectIdentityData {
                initial_cwd: paths.initial_cwd.to_string_lossy().into_owned(),
                git_common_dir: Some(common.to_string_lossy().into_owned()),
            })
        }
        Some(_) => Err(WorkspaceRestoreError::InvalidProjectIdentity),
        None => {
            if dto.worktree_kind != WorktreeKind::NonGit
                || !stack.is_empty()
                || paths.workspace_root != paths.initial_cwd
            {
                return Err(WorkspaceRestoreError::InvalidStackShape);
            }
            if !matches!(
                probe_restore(git, &paths.initial_cwd)?,
                RepositoryProbe::NonGit
            ) || !matches!(
                probe_restore(git, &paths.path_base)?,
                RepositoryProbe::NonGit
            ) {
                return Err(WorkspaceRestoreError::RepositoryMismatch);
            }
            Ok(ProjectIdentityData {
                initial_cwd: paths.initial_cwd.to_string_lossy().into_owned(),
                git_common_dir: None,
            })
        }
    }
}

/// 阶段四：id 派生比对 + 组装候选状态（恢复令牌）。
fn restore_candidate(
    live_state: &WorkspaceState,
    dto: &PersistedWorkspaceContext,
    paths: RestoredPaths,
    stack: Vec<WorkspaceData>,
    canonical_identity: ProjectIdentityData,
) -> Result<WorkspaceRestoreData, WorkspaceRestoreError> {
    let expected_id =
        WorkspaceId::derive(&canonical_identity, &paths.workspace_root.to_string_lossy());
    if dto.workspace_id != expected_id {
        return Err(WorkspaceRestoreError::WorkspaceIdMismatch);
    }

    Ok(WorkspaceRestoreData {
        candidate: WorkspaceState {
            project_identity: canonical_identity,
            workspace_root: paths.workspace_root,
            path_base: paths.path_base,
            worktree_kind: dto.worktree_kind,
            // 恢复不改变运行中进程的 worktree 目录配置：继承 live state。
            worktrees_root: live_state.worktrees_root.clone(),
            stack,
        },
    })
}

impl WorkspaceState {
    /// 校验持久化快照并构造恢复令牌；NEVER 修改 live 状态。
    pub fn prepare_restore(
        live_state: &WorkspaceState,
        dto: &PersistedWorkspaceContext,
        git: &dyn GitWorktreeOps,
    ) -> Result<WorkspaceRestoreData, WorkspaceRestoreError> {
        let mut paths = restore_identity(dto)?;
        let stack = restore_stack(dto)?;
        let canonical_identity = restore_git_location(git, dto, &mut paths, &stack)?;
        restore_candidate(live_state, dto, paths, stack, canonical_identity)
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
