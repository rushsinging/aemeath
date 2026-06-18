//! Update feature 装配。

use std::sync::Arc;

use sdk::UpdateService;
use share::config::paths;
use update::api::UpdateGateway;

/// Update service handle。
pub type UpdateServiceHandle = Arc<dyn UpdateService>;

/// 装配 UpdateGateway，返回 `Arc<dyn UpdateService>`。
pub fn wire_update() -> UpdateServiceHandle {
    let gateway = UpdateGateway::new(paths::global_update_check_path());
    Arc::new(gateway)
}
