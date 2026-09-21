//! 本地 Effect 与平台适配结果回灌到 TUI 的纯值事件。
//! Runtime 业务事件只允许经 `TuiRuntimeEvent` 进入 reducer。

use crate::tui::model::conversation::workspace::WorktreeKind;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WorkspaceMetadataResolved {
    pub root: String,
    pub revision: u64,
    pub branch: Option<String>,
    pub kind: WorktreeKind,
}

/// 本地后台 Effect 和平台适配器回灌到 UI 的事件。
#[derive(Debug)]
pub enum UiEvent {
    Error(String),
    ClipboardImage(sdk::ClipboardImageView),
    /// 粘贴的本地图片引用不可用（路径不存在或已被清理）时，回填原始粘贴文本。
    PasteFallbackToText {
        text: String,
    },
    SystemMessage(String),
    WorkspaceMetadataResolved(WorkspaceMetadataResolved),
    UpdateAvailable {
        current: String,
        latest: String,
        release_url: String,
    },
    DisplayHistoryWindowLoaded {
        window: sdk::DisplayHistoryWindow,
    },
    DisplayHistoryWindowLoadFailed {
        request: sdk::DisplayHistoryWindowRequest,
        message: String,
    },
}
