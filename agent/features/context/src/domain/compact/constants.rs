//! compact 子域常量（#1146 双轨归位）。

pub(crate) const EXPLORATORY_TOOLS: &[&str] = &[
    "Read",
    "Grep",
    "Glob",
    "WebFetch",
    "WebSearch",
    "LS",
    "ToolSearch",
];
pub(crate) const SNIPPABLE_TOOLS: &[&str] = &["Read", "Grep", "Glob"];
pub(crate) const WRITE_TOOLS: &[&str] = &["Edit", "Write"];
/// 历史 session summary 的旧标题，解析侧必须兼容。
pub(crate) const LEGACY_TASK_STATE_HEADING: &str = "\n\n## Current TaskData State\n";

/// task companion 的当前写入标题（写入侧与解析侧共用的唯一真相）。
pub const TASK_STATE_HEADING: &str = "\n\n## Current Task State\n";

pub(crate) const CONTENT_ESCAPE_PREFIX: &str = "\\";

pub(crate) const SECTION_HEADINGS: [&str; 9] = [
    "Immutable Constraints",
    "Current Objective",
    "Committed Facts",
    "Uncommitted Working Set",
    "Open Decisions / Risks",
    "Resume Cursor",
    "Required Revalidation",
    "Archived Milestones",
    "Continuation Status",
];
