//! WorkspacePort — Project BC 出站端口。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2。
//! PL 类型细化由 #892 负责；此处只定义最小骨架。

// ─── Published Language（最小骨架，#892 迁移到 project crate） ───

/// Workspace 上下文帧——包含 cwd、git 信息等。
// TODO(#892): 迁移到 project crate 并细化字段。
#[derive(Debug, Clone)]
pub struct WorkspaceFrame {
    /// 当前工作目录。
    pub cwd: std::path::PathBuf,
    /// 工作区根目录。
    pub workspace_root: std::path::PathBuf,
}

// ─── Port trait ───

/// Project BC 的出站端口。
///
/// Sub Run 使用独立快照 frame（`seed_isolated`），改目录不回写父。
pub trait WorkspacePort: Send + Sync {
    /// 返回当前 workspace frame。
    fn current_frame(&self) -> WorkspaceFrame;

    /// 快照父 frame，创建隔离的工作区上下文。
    ///
    /// Sub Run 装配时强制调用，确保子 Run 改目录不影响父 Run。
    fn seed_isolated(&self) -> WorkspaceFrame;
}
