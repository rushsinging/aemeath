use crate::application::chat::request::{NoTuiChatLaunch, TuiChatLaunch};
use async_trait::async_trait;

/// `ChatRuntimePort` 方法的入参——runtime 启动时的一次性配置包。
///
/// 持有 [`RuntimeResources`](crate::application::resources::RuntimeResources)（不变共享件）
/// + 启动期专有参数（`verbose` / `resume`）。构造完 `ChatLoopContext` 后不再存活。
#[derive(Clone)]
pub struct ChatRuntimeContext {
    pub resources: crate::application::resources::RuntimeResources,
    pub verbose: bool,
    pub resume: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiChatOutcome {
    pub session_id: String,
}

#[async_trait(?Send)]
pub trait ChatRuntimePort {
    async fn run_no_tui_chat(
        &self,
        launch: NoTuiChatLaunch,
        context: ChatRuntimeContext,
    ) -> Result<(), String>;

    async fn run_tui_chat(
        &self,
        launch: TuiChatLaunch,
        context: ChatRuntimeContext,
    ) -> Result<TuiChatOutcome, String>;
}

/// Hook 通知端口——core/ 层通过此 trait 发送 hook 通知，不直接依赖 hook::HookRunner。
#[async_trait]
pub trait HookNotificationPort: Send + Sync {
    async fn on_notification(&self, message: &str, kind: &str, workspace_root: &std::path::Path);
}
