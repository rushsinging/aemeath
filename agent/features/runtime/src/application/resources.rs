//! Runtime 不变共享件——跨 session/loop/tool 传递的同一组资源。
//!
//! 详见 `docs/design/03-runtime-design.md` §"Runtime Context 职责边界"。

use std::collections::HashMap;
use std::sync::Arc;

use context::skill::Skill;
use hook::api::HookRunner;
use provider::{LlmClient, SystemBlock};
use share::config::MemoryConfig;
use storage::TaskStore;
use task::TaskAccess;
use tools::{AgentRunner, MemoryPortSource, ToolRegistry};

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
    /// Legacy 持久化兼容句柄（session snapshot/restore、input_gate clear）。
    /// 仅供 #890/#891 迁移前保留，**不得**下发给 tools/reminder/snapshot 日常链路。
    pub task_store: Arc<TaskStore>,
    /// Runtime/Tool 日常状态的唯一来源（#889）：工具 registry、reminder、
    /// status snapshot、finalize 都经此 low-privilege 端口读写 Task 状态。
    pub task_access: Arc<dyn TaskAccess>,
    pub hook_runner: HookRunner,
    pub agent_runner: Arc<dyn AgentRunner>,
    pub tool_result_materializer:
        Arc<crate::application::tool_result_materialization::ToolResultMaterializer>,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    /// Memory domain port（#897 MemoryPort）。Startup 快照，取自
    /// `wiring.committed_memory()`。#871 动态语义由 `memory_source` 提供
    /// （运行时 resolve），此字段仅供非 tool 链路快取。
    pub memory: Arc<dyn memory::MemoryPort>,
    /// #918 PolicyPort — 当前 AllowAllPolicy；后续可接入 permission gate。
    pub policy: Arc<dyn policy::PolicyPort>,

    // ── 配置（值类型，session 期间不变）──
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub memory_config: MemoryConfig,
    /// #871 dynamic memory source — delegates to `wiring.committed_memory()`
    /// at execution time so resume swaps are transparent to already-registered
    /// tools. `memory` above is the startup snapshot; this source is the
    /// runtime-resolved port used by `MemoryTool` via `register_subagent_tools`.
    pub memory_source: Arc<dyn MemoryPortSource>,
    pub skills_map: HashMap<String, Skill>,
    pub context_size: usize,
    pub allow_all: bool,
    /// Language code for prompt/reminder text selection (`"en"` / `"zh"`).
    pub language: String,
}
