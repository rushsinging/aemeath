//! TUI 面向 SDK 的公共契约。
//!
//! 这些类型只描述 TUI 与 runtime 之间的稳定边界，不依赖具体 TUI
//! 渲染库，也不暴露 runtime 内部的 LLM client、tool registry、task store
//! 或取消 token。

use crate::ChatInputEvent;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

pub type EventFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
pub type QueueFuture<'a> = Pin<Box<dyn Future<Output = Option<Vec<String>>> + Send + 'a>>;
pub type InputEventFuture<'a> = Pin<Box<dyn Future<Output = Vec<ChatInputEvent>> + Send + 'a>>;

/// runtime 返回给 TUI 的 chat handle。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatHandle {
    pub id: String,
}

/// TUI 从 runtime 接收 chat 流式事件的 sink。
pub trait ChatEventSink<Event>: Clone + Send + Sync + 'static {
    fn send_event<'a>(&'a self, event: Event) -> EventFuture<'a>;

    fn try_send_event(&self, event: Event);
}

/// runtime 请求 TUI drain 排队输入的端口。
pub trait QueueDrainPort: Send + Sync + 'static {
    fn drain_queued_input<'a>(&'a self) -> QueueFuture<'a>;
}

/// runtime drain 忙碌期间追加输入事件的端口。
pub trait ChatInputEventPort: Send + Sync + 'static {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a>;
}

/// TUI 可直接渲染的任务状态视图。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStatusView {
    pub lines: Vec<String>,
}

/// TUI 可渲染的图片输入视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardImageView {
    pub base64: String,
    pub media_type: String,
    pub final_size: usize,
    pub display_path: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl From<ClipboardImageView> for crate::ToolResultImage {
    fn from(value: ClipboardImageView) -> Self {
        Self {
            base64: value.base64,
            media_type: value.media_type,
        }
    }
}

/// Reflection 建议记忆视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionMemorySuggestionView {
    pub content: String,
    pub layer: String,
}

/// Reflection 输出视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionOutputView {
    pub content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub suggested_memories: Vec<ReflectionMemorySuggestionView>,
    pub outdated_memories: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReflectionConfigView {
    pub enabled: bool,
    pub interval_turns: usize,
    pub auto_apply_suggestions: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryConfigView {
    pub enabled: bool,
    pub max_entries: usize,
    pub similarity_threshold: f32,
    pub reflection: ReflectionConfigView,
}

impl Default for MemoryConfigView {
    fn default() -> Self {
        Self {
            enabled: false,
            max_entries: 0,
            similarity_threshold: 0.0,
            reflection: ReflectionConfigView::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillView {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub content: String,
    pub source: Option<String>,
}

/// SDK 级 TUI 启动上下文。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteKind {
    Empty,
    ImageFile,
    Text,
}

pub fn classify_paste(text: &str) -> PasteKind {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return PasteKind::Empty;
    }
    if is_image_file_path(trimmed) {
        return PasteKind::ImageFile;
    }
    PasteKind::Text
}

pub fn is_image_file_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".bmp")
}

#[derive(Debug, Clone)]
pub struct TuiLaunchContext {
    pub session_id: String,
    pub cwd: PathBuf,
    pub model_display: String,
    pub memory_config: MemoryConfigView,
    pub skills_map: std::collections::HashMap<String, SkillView>,
    pub initial_resume_id: Option<String>,
}

/// 会话 reminder 视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderView {
    pub id: String,
    pub content: String,
    pub done: bool,
    pub created_at: u64,
}

impl ReminderView {
    pub fn active(reminders: &[ReminderView]) -> Vec<&ReminderView> {
        reminders.iter().filter(|r| !r.done).collect()
    }

    pub fn recap_line(reminders: &[ReminderView]) -> Option<String> {
        let active: Vec<&str> = reminders
            .iter()
            .filter(|r| !r.done)
            .map(|r| r.content.as_str())
            .collect();
        if active.is_empty() {
            None
        } else {
            Some(format!("* recap: {}", active.join(" | ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_image_view_keeps_render_fields() {
        let image = ClipboardImageView {
            base64: "abc".to_string(),
            media_type: "image/png".to_string(),
            final_size: 3,
            display_path: Some("/tmp/a.png".to_string()),
            width: Some(10),
            height: Some(20),
        };

        assert_eq!(image.base64, "abc");
        assert_eq!(image.media_type, "image/png");
        assert_eq!(image.final_size, 3);
        assert_eq!(image.display_path.as_deref(), Some("/tmp/a.png"));
    }

    #[test]
    fn test_reflection_output_view_counts_suggestions() {
        let output = ReflectionOutputView {
            content: "summary".to_string(),
            input_tokens: 1,
            output_tokens: 2,
            suggested_memories: vec![ReflectionMemorySuggestionView {
                content: "remember".to_string(),
                layer: "project".to_string(),
            }],
            outdated_memories: vec!["old".to_string()],
        };

        assert_eq!(output.suggested_memories.len(), 1);
        assert_eq!(output.outdated_memories.len(), 1);
        assert_eq!(output.content, "summary");
    }

    #[test]
    fn test_memory_config_view_default_is_disabled_safe() {
        let config = MemoryConfigView::default();

        assert!(!config.enabled);
        assert_eq!(config.max_entries, 0);
        assert!(!config.reflection.enabled);
    }
}
