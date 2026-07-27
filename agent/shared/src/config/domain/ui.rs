//! UI 配置

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

const MAX_MARKDOWN_SPACING_LINES: u8 = 8;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkdownSpacingMode {
    #[default]
    Normal,
    Compact,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SpacingLines(u8);

impl SpacingLines {
    pub fn new(value: u8) -> Result<Self, String> {
        if value <= MAX_MARKDOWN_SPACING_LINES {
            Ok(Self(value))
        } else {
            Err(format!(
                "Markdown 间距必须在 0..={MAX_MARKDOWN_SPACING_LINES} 之间"
            ))
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl Serialize for SpacingLines {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.0)
    }
}

impl<'de> Deserialize<'de> for SpacingLines {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElementSpacingOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<SpacingLines>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<SpacingLines>,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkdownSpacingOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paragraph: Option<ElementSpacingOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<ElementSpacingOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<ElementSpacingOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_block: Option<ElementSpacingOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<ElementSpacingOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blockquote: Option<ElementSpacingOverride>,
}

pub(crate) fn default_true() -> bool {
    true
}

/// Task list display configuration (spinner下方窗口化显示)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskListConfig {
    /// 最大显示行数（不含摘要行）
    #[serde(default = "default_task_max_lines")]
    pub max_lines: usize,
    /// 折叠提示格式。{n} = 隐藏数量
    #[serde(default = "default_fold_hint_format")]
    pub fold_hint_format: String,
}

fn default_task_max_lines() -> usize {
    7
}
fn default_fold_hint_format() -> String {
    "… +{n} more".to_string()
}

impl Default for TaskListConfig {
    fn default() -> Self {
        Self {
            max_lines: 7,
            fold_hint_format: "… +{n} more".to_string(),
        }
    }
}

/// Task lifecycle management configuration (跨轮次生命周期策略)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskLifecycleConfig {
    /// 新 turn 开始时自动清除已完成 batch
    #[serde(default = "default_true")]
    pub auto_clear_completed_on_new_turn: bool,
    /// 中断未完成时弹出提示
    #[serde(default = "default_true")]
    pub interrupt_prompt_enabled: bool,
    /// 中断提示默认动作：pause / continue / discard
    #[serde(default = "default_interrupt_action")]
    pub interrupt_default_action: String,
    /// 沉默提醒阈值（轮数）
    #[serde(default = "default_stale_remind_after_turns")]
    pub stale_remind_after_turns: usize,
    /// 沉默提醒重复间隔（轮数）
    #[serde(default = "default_stale_remind_repeat_interval")]
    pub stale_remind_repeat_interval: usize,
}

fn default_interrupt_action() -> String {
    "pause".to_string()
}
fn default_stale_remind_after_turns() -> usize {
    3
}
fn default_stale_remind_repeat_interval() -> usize {
    5
}

impl Default for TaskLifecycleConfig {
    fn default() -> Self {
        Self {
            auto_clear_completed_on_new_turn: true,
            interrupt_prompt_enabled: true,
            interrupt_default_action: "pause".to_string(),
            stale_remind_after_turns: 3,
            stale_remind_repeat_interval: 5,
        }
    }
}

/// UI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// Enable markdown rendering
    #[serde(default = "default_true")]
    pub markdown: bool,

    /// Enable syntax highlighting
    #[serde(default = "default_true")]
    pub syntax_highlight: bool,

    /// Show progress indicators
    #[serde(default = "default_true")]
    pub progress: bool,

    /// Color output
    #[serde(default = "default_true")]
    pub color: bool,

    /// Verbose output
    #[serde(default)]
    pub verbose: bool,

    /// TUI mode
    #[serde(default = "default_true")]
    pub tui: bool,

    /// Markdown block spacing mode
    #[serde(default)]
    pub markdown_spacing: MarkdownSpacingMode,

    /// Per-element Markdown block spacing overrides
    #[serde(default)]
    pub markdown_spacing_overrides: MarkdownSpacingOverrides,

    /// Task list display configuration
    #[serde(default)]
    pub task_list: TaskListConfig,

    /// Task lifecycle management configuration
    #[serde(default)]
    pub task_lifecycle: TaskLifecycleConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            markdown: true,
            syntax_highlight: true,
            progress: true,
            color: true,
            verbose: false,
            tui: true,
            markdown_spacing: MarkdownSpacingMode::default(),
            markdown_spacing_overrides: MarkdownSpacingOverrides::default(),
            task_list: TaskListConfig::default(),
            task_lifecycle: TaskLifecycleConfig::default(),
        }
    }
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
