//! Runtime stream payloads rendered or projected by the TUI.
//!
//! Values here are owned by the TUI adapter. They intentionally mirror only
//! fields consumed by TUI model, view, and update paths.
//!
//! Some constructors and structs are not yet exercised by production after
//! the #943 ACL migration; they are retained as DTO reserves and will be
//! consumed by #1246 / #944 5B.

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiTaskState {
    pub session_id: String,
    pub revision: u64,
    pub current_batch: Option<TuiTaskBatch>,
    pub total: usize,
    pub completed: usize,
    pub in_progress: usize,
    pub items: Vec<TuiTaskItem>,
    pub hidden_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiTaskBatch {
    pub id: u64,
    pub summary: Option<String>,
    pub status: TuiTaskBatchStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiTaskBatchStatus {
    Active,
    Paused,
    Archived,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiTaskItem {
    pub id: u64,
    pub sequence: u64,
    pub subject: String,
    pub status: TuiTaskItemStatus,
    pub priority: TuiTaskPriority,
    pub blocked_by_sequences: Vec<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiTaskItemStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiTaskPriority {
    Low,
    Normal,
    High,
    Urgent,
}

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
    Hook,
    SkillRequest,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub(crate) struct TuiSkillRequestMetadata {
    pub(crate) skill: String,
    pub(crate) arguments: String,
    pub(crate) raw_input: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, serde::Serialize)]
pub(crate) enum TuiHookNoticeKind {
    Blocked,
    Failed,
    Info,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub(crate) struct TuiHookNotice {
    pub(crate) point: String,
    pub(crate) kind: TuiHookNoticeKind,
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

impl TuiHookNotice {
    pub(crate) fn title(&self) -> String {
        let status = match self.kind {
            TuiHookNoticeKind::Blocked => "blocked",
            TuiHookNoticeKind::Failed => "failed",
            TuiHookNoticeKind::Info => "message",
        };
        format!("{} hook {status}", self.point)
    }

    pub(crate) fn display_text(&self) -> String {
        let exit_code = self
            .exit_code
            .map_or_else(|| "unknown".to_string(), |code| code.to_string());
        let mut lines = vec![
            self.summary.clone(),
            format!("Command: {}", self.command),
            format!("Exit code: {exit_code}"),
            format!("Reason: {}", self.reason),
        ];
        if !self.stdout_preview.is_empty() {
            let truncated = if self.stdout_truncated {
                " (truncated)"
            } else {
                ""
            };
            lines.push(format!("stdout{truncated}:\n{}", self.stdout_preview));
        }
        if !self.stderr_preview.is_empty() {
            let truncated = if self.stderr_truncated {
                " (truncated)"
            } else {
                ""
            };
            lines.push(format!("stderr{truncated}:\n{}", self.stderr_preview));
        }
        if let Some(output_file) = self.output_file.as_ref() {
            lines.push(format!("Full output: {output_file}"));
        }
        lines.join("\n")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiResumedStepFinalizeCause {
    Completed,
    UserCancelledStep,
    RunTerminated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiResumedSessionStep {
    pub(crate) run_id: String,
    pub(crate) step_id: String,
    pub(crate) messages: Vec<TuiChatMessage>,
    pub(crate) finalize_cause: Option<TuiResumedStepFinalizeCause>,
    pub(crate) duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiDisplayHistoryStepReference {
    pub(crate) run_id: String,
    pub(crate) step_id: String,
    pub(crate) member_name: String,
    pub(crate) estimated_lines: usize,
    pub(crate) user_input_history: Vec<String>,
    pub(crate) finalize_cause: Option<TuiResumedStepFinalizeCause>,
    pub(crate) duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiDisplayHistoryIndex {
    pub(crate) session_id: String,
    pub(crate) generation_revision: u64,
    pub(crate) steps: Vec<TuiDisplayHistoryStepReference>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiDisplayHistoryWindow {
    pub(crate) session_id: String,
    pub(crate) generation_revision: u64,
    pub(crate) steps: Vec<TuiResumedSessionStep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiChatMessage {
    pub(crate) role: String,
    pub(crate) content: Vec<TuiContentBlock>,
    pub(crate) input_id: Option<String>,
    pub(crate) source: TuiMessageSource,
    pub(crate) hook_notice: Option<TuiHookNotice>,
    pub(crate) skill_request: Option<TuiSkillRequestMetadata>,
}

impl TuiContentBlock {
    #[cfg(test)]
    pub(crate) fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
}

impl TuiChatMessage {
    #[cfg(test)]
    pub(crate) fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: vec![TuiContentBlock::text(text)],
            input_id: None,
            source: TuiMessageSource::User,
            hook_notice: None,
            skill_request: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn system_generated_user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: vec![TuiContentBlock::text(text)],
            input_id: None,
            source: TuiMessageSource::SystemGenerated,
            hook_notice: None,
            skill_request: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn skill_request(text: impl Into<String>, payload: TuiSkillRequestMetadata) -> Self {
        Self {
            role: "user".to_string(),
            content: vec![TuiContentBlock::text(text)],
            input_id: None,
            source: TuiMessageSource::SkillRequest,
            hook_notice: None,
            skill_request: Some(payload),
        }
    }

    #[cfg(test)]
    pub(crate) fn hook_notice(text: impl Into<String>, notice: TuiHookNotice) -> Self {
        Self {
            role: "user".to_string(),
            content: vec![TuiContentBlock::text(text)],
            input_id: None,
            source: TuiMessageSource::Hook,
            hook_notice: Some(notice),
            skill_request: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn assistant_text(text: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: vec![TuiContentBlock::text(text)],
            input_id: None,
            source: TuiMessageSource::User,
            hook_notice: None,
            skill_request: None,
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
pub(crate) struct TuiToolResultImage {
    pub(crate) base64: String,
    pub(crate) media_type: String,
}

#[cfg(test)]
#[path = "runtime_view_tests.rs"]
mod tests;
