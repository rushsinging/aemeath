//! Update feature 装配。

use std::sync::Arc;

use sdk::UpdateService;
use update::api::UpdateGateway;

/// Update service handle。
pub type UpdateServiceHandle = Arc<dyn UpdateService>;

/// 返回未加载项目配置时的默认 User-Agent。
pub fn default_user_agent() -> String {
    share::config::Config::default().api.user_agent
}

/// 装配 UpdateGateway，返回 `Arc<dyn UpdateService>`。
pub fn wire_update(user_agent: impl Into<String>) -> UpdateServiceHandle {
    Arc::new(UpdateGateway::with_user_agent(user_agent.into()))
}
