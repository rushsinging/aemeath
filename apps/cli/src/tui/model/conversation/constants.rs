//! 纯值常量（#1146 placement 归位）。

/// AskUser 交互块的 id 前缀。
/// AskUser 交互块的 id 前缀。
pub const ASK_USER_BLOCK_ID_PREFIX: &str = "ask-user-";

/// 启动横幅文本，作为对话起始的 System block 注入单一真相源。
/// 启动横幅文本，作为对话起始的 System block 注入单一真相源。
pub const BANNER_LINES: [&str; 4] = [
    "Aemeath - AI Agent",
    "",
    "Type /help for available commands",
    "",
];

pub(crate) const OUTPUT_VIEW_JOURNAL_CAPACITY: usize = 256;

pub(crate) const MAX_LOADED_HISTORY_STEPS: usize = 128;

pub(crate) const DONE_VERBS: [&str; 20] = [
    "Sautéed",
    "Baked",
    "Grilled",
    "Simmered",
    "Roasted",
    "Brewed",
    "Toasted",
    "Stewed",
    "Marinated",
    "Charred",
    "Poached",
    "Steamed",
    "Smoked",
    "Brûléed",
    "Flambéed",
    "Fermented",
    "Pickled",
    "Cured",
    "Seared",
    "Blanched",
];

pub(crate) const STREAM_CAP: usize = 4 * 1024;
