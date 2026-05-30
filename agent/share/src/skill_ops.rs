use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// 目录遍历 loader（含 fs IO）已归位 prompt domain（`prompt::skill::loader`，refs #61 D2）。
// 本模块仅保留 `Skill` DTO 与单文件 parser（被 tools/runtime/prompt 多 crate 依赖的契约）。

/// A skill definition loaded from a markdown file with YAML frontmatter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub content: String,
    pub source_path: PathBuf,
    /// Tools required for this skill to be available
    #[serde(default)]
    pub requires_tools: Vec<String>,
    /// If these skills are available, hide this one (it's a fallback)
    #[serde(default)]
    pub fallback_for: Vec<String>,
    /// Slash command aliases (e.g. ["cm"] means /cm invokes this skill)
    #[serde(default)]
    pub aliases: Vec<String>,
}
