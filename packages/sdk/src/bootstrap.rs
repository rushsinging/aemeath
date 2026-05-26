//! 启动参数 DTO。
//!
//! 这些类型属于 SDK 契约层，用于 CLI composition root 将命令行参数传给
//! Runtime 的真实实现；不得依赖 runtime 内部类型。

use std::path::PathBuf;

/// 启动聊天运行时所需的参数。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatBootstrapArgs {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<PathBuf>,
    pub max_tokens: Option<u32>,
    pub verbose: bool,
    pub no_markdown: bool,
    pub context_size: usize,
    pub resume: Option<String>,
    pub allow_all: bool,
    pub max_tool_concurrency: Option<usize>,
    pub max_agent_concurrency: Option<usize>,
    pub no_think: bool,
    pub reasoning_effort: Option<String>,
}
