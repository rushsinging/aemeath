pub use crate::business::agent;
pub use crate::business::agent::runner as agent_runner;
pub use crate::business::chat;
pub use crate::business::chat::looping;
pub use crate::business::compact;
pub use crate::business::cost;
pub use crate::business::prompt::build as prompt_build;
pub use crate::business::prompt::prompt_build_ext;
pub use crate::business::prompt::skill_command;
pub use crate::business::reflection;
pub use crate::business::scheduler;
pub use crate::business::session;
pub use crate::business::state;
pub use crate::core::client;
pub use crate::core::command;
pub use crate::core::port;
pub use crate::core::service;
pub use crate::utils::bootstrap;
pub use crate::utils::image;
// 下游 supporting crate 的精确 re-export（DDD §6.4.3 rule4）：
// `runtime::api` 不再以 `pub use <crate>;` 整体转发下游 crate，
// 仅精确暴露 runtime use case 实际消费的子模块 / 类型，避免对外泄漏整个下游 crate。
// `audit` 当前无任何消费点，已移除整体转发。

pub mod hook {
    pub use ::hook::api::*;
}

pub mod policy {
    pub use ::policy::api::{format_warnings, scan_content, SecurityWarning};
}

pub mod project {
    pub use ::project::api::{
        enter_worktree, exit_worktree, workspace_context_from_tool_context,
    };
}

pub mod prompt {
    pub use ::prompt::api::guidance;
    pub use ::prompt::api::skill;
}

pub mod provider {
    pub use ::provider::client;
    pub use ::provider::pool;
    pub use ::provider::provider;
    pub use ::provider::providers;
    pub use ::provider::stream;
    pub use ::provider::types;
    pub use ::provider::{ApiDriverKind, LlmClientPool, LlmError, StreamHandler};
}

pub mod storage {
    pub use ::storage::api::{
        memory_base_dir, persist_oversized_results, project_hash, project_hash_from_path,
        MemoryStore,
    };
}

pub mod tools {
    pub use ::tools::api::{
        is_readonly_command, register_all_tools, register_subagent_tools, McpConnectionManager,
        McpServerConfig,
    };
}

// `core` 仅精确 re-export share 中实际被 runtime use case 消费的领域子模块，
// 不再用 `share::*` 通配整体转发整个 share crate。
pub mod core {
    pub use share::config;
    pub use share::memory;
    pub use share::message;
    pub use share::task;
    pub use share::tool;
}
