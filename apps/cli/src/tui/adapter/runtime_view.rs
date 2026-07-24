//! Runtime stream payloads rendered or projected by the TUI.
//!
//! Values here are owned by the TUI adapter. They intentionally mirror only
//! fields consumed by TUI model, view, and update paths.
//!
//! Some constructors and structs are not yet exercised by production after
//! the #943 ACL migration; they are retained as DTO reserves and will be
//! consumed by #1246 / #944 5B.

#![allow(dead_code)]

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TuiContentBlock {
    Text {
        text: String,
    },
    Image {
        media_type: String,
        base64: String,
        placeholder: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
        is_error: bool,
        text: Option<String>,
    },
    Thinking {
        thinking: String,
        signature: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiMessageSource {
    User,
    SystemGenerated,
    StopHook,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiStopHookFeedback {
    pub(crate) summary: String,
    pub(crate) command: String,
    pub(crate) exit_code: Option<i32>,
    pub(crate) reason: String,
    pub(crate) stdout_preview: String,
    pub(crate) stderr_preview: String,
    pub(crate) stdout_truncated: bool,
    pub(crate) stderr_truncated: bool,
    pub(crate) output_file: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiResumedSessionStep {
    pub(crate) run_id: String,
    pub(crate) step_id: String,
    pub(crate) messages: Vec<TuiChatMessage>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiChatMessage {
    pub(crate) role: String,
    pub(crate) content: Vec<TuiContentBlock>,
    pub(crate) input_id: Option<String>,
    pub(crate) source: TuiMessageSource,
    pub(crate) stop_hook: Option<TuiStopHookFeedback>,
}

impl TuiContentBlock {
    pub(crate) fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
}

impl TuiChatMessage {
    pub(crate) fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: vec![TuiContentBlock::text(text)],
            input_id: None,
            source: TuiMessageSource::User,
            stop_hook: None,
        }
    }

    pub(crate) fn system_generated_user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: vec![TuiContentBlock::text(text)],
            input_id: None,
            source: TuiMessageSource::SystemGenerated,
            stop_hook: None,
        }
    }

    pub(crate) fn assistant_text(text: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: vec![TuiContentBlock::text(text)],
            input_id: None,
            source: TuiMessageSource::User,
            stop_hook: None,
        }
    }

    pub(crate) fn text_content(&self) -> String {
        self.content
            .iter()
            .map(|block| match block {
                TuiContentBlock::Text { text } => text.as_str(),
                TuiContentBlock::Image {
                    placeholder: Some(placeholder),
                    ..
                } => placeholder.as_str(),
                _ => "",
            })
            .collect()
    }

    pub(crate) fn is_user_input(&self) -> bool {
        self.role == "user"
            && self.source == TuiMessageSource::User
            && self
                .content
                .iter()
                .any(|block| matches!(block, TuiContentBlock::Text { .. }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiClipboardImage {
    pub(crate) base64: String,
    pub(crate) media_type: String,
    pub(crate) final_size: usize,
    pub(crate) display_path: Option<String>,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiToolResultImage {
    pub(crate) base64: String,
    pub(crate) media_type: String,
}

#[cfg(test)]
#[path = "runtime_view_tests.rs"]
mod tests;
