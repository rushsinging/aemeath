//! 项目上下文。

use serde::{Deserialize, Serialize};

/// 项目上下文快照（Copy 语义，零开销）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectContext {
    /// 当前工作目录。
    pub cwd: String,
    /// path_base（相对路径解析基准）。
    pub path_base: String,
    /// workspace root（git 根目录）。
    pub working_root: String,
    /// 当前 git 分支。
    pub git_branch: Option<String>,
}
