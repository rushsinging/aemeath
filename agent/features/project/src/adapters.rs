pub(crate) mod git;

use std::path::PathBuf;
use std::sync::Arc;

use crate::domain::service::WorkspaceService;

impl WorkspaceService {
    /// 使用生产 Git CLI 适配器创建 workspace 服务。
    pub fn new(cwd: PathBuf) -> Arc<Self> {
        Self::with_git(cwd, Arc::new(git::GitCli))
    }
}
