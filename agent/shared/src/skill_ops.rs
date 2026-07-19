use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Legacy DTO retained only for the #914 SkillTool compatibility path.
// New Skill Catalog / Materialization consumers use the Tools-owned Published Language.

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
