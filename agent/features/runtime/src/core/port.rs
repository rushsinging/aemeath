use crate::business::chat::request::{NoTuiChatLaunch, TuiChatLaunch};
use async_trait::async_trait;
use hook::api::HookRunner;
use prompt::api::skill::Skill;
use provider::api::LlmClient;
use provider::api::SystemBlock;
use share::config::MemoryConfig;
use std::collections::HashMap;
use std::sync::Arc;
use storage::api::TaskStore;
use storage::api::{Task, TaskSnapshot};
use tools::api::{AgentRunner, ToolRegistry};

#[derive(Clone)]
pub struct ChatRuntimeContext {
    pub client: Arc<LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub agent_runner: Arc<dyn AgentRunner>,
    pub task_store: Arc<TaskStore>,
    pub skills_map: HashMap<String, Skill>,
    pub hook_runner: HookRunner,
    pub memory_config: MemoryConfig,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub allow_all: bool,
    pub context_size: usize,
    pub verbose: bool,
    pub resume: Option<String>,
    /// Language code for prompt/reminder text selection (`"en"` / `"zh"`).
    pub language: String,
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

// ─── 细粒度 Port Trait（P16 新增） ───

/// 任务持久化端口——core/ 层通过此 trait 访问任务存储，不直接依赖 storage::api::TaskStore 方法。
#[async_trait]
pub trait TaskStorePort: Send + Sync {
    async fn snapshot(&self) -> TaskSnapshot;
    async fn restore(&self, snapshot: TaskSnapshot);
    async fn list_current_batch(&self) -> Vec<Task>;
    async fn get_batch_display_map(&self) -> HashMap<String, usize>;
}

/// Provider 信息端口——core/ 层通过此 trait 查询当前 LLM client 的元数据。
pub trait ProviderInfoPort: Send + Sync {
    fn provider_name(&self) -> &str;
    fn model_name(&self) -> &str;
    fn is_reasoning(&self) -> bool;
    fn set_reasoning(&self, enabled: bool);
}

/// Hook 通知端口——core/ 层通过此 trait 发送 hook 通知，不直接依赖 hook::HookRunner。
#[async_trait]
pub trait HookNotificationPort: Send + Sync {
    async fn on_notification(&self, message: &str, kind: &str);
}
