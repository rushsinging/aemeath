use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatLaunchOptions {
    pub cwd: PathBuf,
    pub verbose: bool,
    pub markdown: bool,
    pub context_size: usize,
    pub resume: Option<String>,
    pub allow_all: bool,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
}

impl ChatLaunchOptions {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.max_tool_concurrency == 0 {
            return Err("max_tool_concurrency 必须大于 0".to_string());
        }
        if self.max_agent_concurrency == 0 {
            return Err("max_agent_concurrency 必须大于 0".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NoTuiChatLaunch {
    pub options: ChatLaunchOptions,
}

impl NoTuiChatLaunch {
    pub(crate) fn validate(&self) -> Result<(), String> {
        self.options.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TuiChatLaunch {
    pub options: ChatLaunchOptions,
    pub session_id: String,
    pub model_display: String,
}

impl TuiChatLaunch {
    pub(crate) fn validate(&self) -> Result<(), String> {
        self.options.validate()?;
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
mod tests {
    use super::*;

    fn base_options() -> ChatLaunchOptions {
        ChatLaunchOptions {
            cwd: PathBuf::from("/tmp/aemeath"),
            verbose: false,
            markdown: true,
            context_size: 200_000,
            resume: None,
            allow_all: false,
            max_tool_concurrency: 10,
            max_agent_concurrency: 4,
        }
    }

    #[test]
    fn test_validate_accepts_no_tui_launch() {
        let launch = NoTuiChatLaunch {
            options: base_options(),
        };

        let result = launch.validate();

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_validate_accepts_tui_launch_with_required_fields() {
        let launch = TuiChatLaunch {
            options: base_options(),
            session_id: "session-1".to_string(),
            model_display: "provider/model".to_string(),
        };

        let result = launch.validate();

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_validate_rejects_tui_missing_session_id() {
        let launch = TuiChatLaunch {
            options: base_options(),
            session_id: String::new(),
            model_display: "provider/model".to_string(),
        };

        let result = launch.validate();

        assert_eq!(result, Err("TUI 启动必须提供 session_id".to_string()));
    }

    #[test]
    fn test_validate_rejects_tui_missing_model_display() {
        let launch = TuiChatLaunch {
            options: base_options(),
            session_id: "session-1".to_string(),
            model_display: String::new(),
        };

        let result = launch.validate();

        assert_eq!(result, Err("TUI 启动必须提供 model_display".to_string()));
    }

    #[test]
    fn test_validate_rejects_zero_tool_concurrency() {
        let mut options = base_options();
        options.max_tool_concurrency = 0;
        let launch = NoTuiChatLaunch { options };

        let result = launch.validate();

        assert_eq!(result, Err("max_tool_concurrency 必须大于 0".to_string()));
    }

    #[test]
    fn test_validate_rejects_zero_agent_concurrency() {
        let mut options = base_options();
        options.max_agent_concurrency = 0;
        let launch = NoTuiChatLaunch { options };

        let result = launch.validate();

        assert_eq!(result, Err("max_agent_concurrency 必须大于 0".to_string()));
    }
}
