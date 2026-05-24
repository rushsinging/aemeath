//! UI 配置

use serde::{Deserialize, Serialize};

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
            task_list: TaskListConfig::default(),
            task_lifecycle: TaskLifecycleConfig::default(),
        }
    }
}
