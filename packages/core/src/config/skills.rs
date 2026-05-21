//! 技能配置

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Skill configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillsConfig {
    /// Additional directories to load skills from (in addition to `{cwd}/.agents/skills` and `~/.agents/skills`).
    /// Supports `~` expansion for home directory.
    #[serde(default)]
    pub dirs: Vec<PathBuf>,
}
