use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatLaunchOptions {
    pub cwd: PathBuf,
    pub verbose: bool,
    pub markdown: bool,
    pub context_size: usize,
    pub resume: Option<String>,
    pub max_tool_concurrency: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoTuiChatLaunch {
    pub options: ChatLaunchOptions,
}

impl NoTuiChatLaunch {
    pub fn validate(&self) -> Result<(), String> {
        if self.options.max_tool_concurrency == 0 {
            return Err("max_tool_concurrency 必须大于 0".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiChatLaunch {
    pub options: ChatLaunchOptions,
    pub max_agent_concurrency: usize,
    pub session_id: String,
    pub model_display: String,
}

impl TuiChatLaunch {
    pub fn validate(&self) -> Result<(), String> {
        if self.options.max_tool_concurrency == 0 {
            return Err("max_tool_concurrency 必须大于 0".to_string());
        }
        if self.max_agent_concurrency == 0 {
            return Err("max_agent_concurrency 必须大于 0".to_string());
        }
        if self.session_id.is_empty() {
            return Err("TUI 启动必须提供 session_id".to_string());
        }
        if self.model_display.is_empty() {
            return Err("TUI 启动必须提供 model_display".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "chat_launch_tests.rs"]
mod tests;

