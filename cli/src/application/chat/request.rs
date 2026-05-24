use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatLaunchMode {
    NoTui,
    Tui,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatLaunchRequest {
    pub mode: ChatLaunchMode,
    pub session_id: Option<String>,
    pub cwd: PathBuf,
    pub model_display: Option<String>,
    pub verbose: bool,
    pub markdown: bool,
    pub context_size: usize,
    pub resume: Option<String>,
    pub allow_all: bool,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
}

impl ChatLaunchRequest {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.max_tool_concurrency == 0 {
            return Err("max_tool_concurrency 必须大于 0".to_string());
        }
        if self.max_agent_concurrency == 0 {
            return Err("max_agent_concurrency 必须大于 0".to_string());
        }
        match self.mode {
            ChatLaunchMode::NoTui => Ok(()),
            ChatLaunchMode::Tui => {
                if self.session_id.as_deref().unwrap_or_default().is_empty() {
                    return Err("TUI 启动必须提供 session_id".to_string());
                }
                if self.model_display.as_deref().unwrap_or_default().is_empty() {
                    return Err("TUI 启动必须提供 model_display".to_string());
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_request(mode: ChatLaunchMode) -> ChatLaunchRequest {
        ChatLaunchRequest {
            mode,
            session_id: None,
            cwd: PathBuf::from("/tmp/aemeath"),
            model_display: None,
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
    fn test_validate_accepts_no_tui_without_tui_fields() {
        let request = base_request(ChatLaunchMode::NoTui);

        let result = request.validate();

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_validate_accepts_tui_with_required_fields() {
        let mut request = base_request(ChatLaunchMode::Tui);
        request.session_id = Some("session-1".to_string());
        request.model_display = Some("provider/model".to_string());

        let result = request.validate();

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_validate_rejects_tui_missing_session_id() {
        let mut request = base_request(ChatLaunchMode::Tui);
        request.model_display = Some("provider/model".to_string());

        let result = request.validate();

        assert_eq!(result, Err("TUI 启动必须提供 session_id".to_string()));
    }

    #[test]
    fn test_validate_rejects_tui_missing_model_display() {
        let mut request = base_request(ChatLaunchMode::Tui);
        request.session_id = Some("session-1".to_string());

        let result = request.validate();

        assert_eq!(result, Err("TUI 启动必须提供 model_display".to_string()));
    }

    #[test]
    fn test_validate_rejects_zero_tool_concurrency() {
        let mut request = base_request(ChatLaunchMode::NoTui);
        request.max_tool_concurrency = 0;

        let result = request.validate();

        assert_eq!(result, Err("max_tool_concurrency 必须大于 0".to_string()));
    }

    #[test]
    fn test_validate_rejects_no_tui_zero_agent_concurrency() {
        let mut request = base_request(ChatLaunchMode::NoTui);
        request.max_agent_concurrency = 0;

        let result = request.validate();

        assert_eq!(result, Err("max_agent_concurrency 必须大于 0".to_string()));
    }
}
