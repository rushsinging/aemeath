use crate::application::main_loop::request::{NoTuiChatLaunch, TuiChatLaunch};
use async_trait::async_trait;

/// `ChatRuntimePort` 方法的入参——runtime 启动时的一次性配置包。
///
/// #1385 Task 7: 退化为真正启动参数（`verbose`/`resume`）。
/// 原先的 `RuntimeResources` bag 已被删除；所有服务由
/// `MainSessionShell` → `RuntimeContextFactory::assemble()` → `RuntimeContext` 装配。
#[derive(Clone)]
pub struct ChatRuntimeContext {
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
