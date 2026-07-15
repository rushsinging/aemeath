//! Runtime 不变共享件——跨 session/loop/tool 传递的同一组资源。
//!
//! 详见 `docs/design/03-runtime-design.md` §"Runtime Context 职责边界"。

use std::collections::HashMap;
use std::sync::Arc;

use context::skill::Skill;
use hook::api::HookRunner;
use provider::api::{LlmClient, SystemBlock};
use share::config::MemoryConfig;
use storage::api::TaskStore;
use tools::api::{AgentRunner, ToolRegistry};

/// Runtime 不变共享件——跨 session/loop/tool 传递的同一组资源。
///
/// 所有 `Arc` 字段指向同一份底层实例，克隆开销极低。
/// session 级以下的所有 context（`ChatLoopContext` / `ToolExecutionContext` 等）
/// 持有此结构的 clone，不再各自重复声明这些字段。
#[derive(Clone)]
pub struct RuntimeResources {
    // ── 服务句柄（Arc 共享）──
    pub client: Arc<LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub task_store: Arc<TaskStore>,
    pub hook_runner: HookRunner,
    pub agent_runner: Arc<dyn AgentRunner>,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,

    // ── 配置（值类型，session 期间不变）──
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub memory_config: MemoryConfig,
    pub skills_map: HashMap<String, Skill>,
    pub context_size: usize,
    pub allow_all: bool,
    /// Language code for prompt/reminder text selection (`"en"` / `"zh"`).
    pub language: String,

    // ── Reasoning Graph 配置（session 级，loop 时实例化）──
    /// Reasoning Graph 配置。`enabled=false` 时不创建 graph 实例（None）。
    pub reasoning_graph_config: Option<crate::application::reasoning_graph::GraphRuntimeConfig>,
}
