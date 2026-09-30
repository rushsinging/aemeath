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
