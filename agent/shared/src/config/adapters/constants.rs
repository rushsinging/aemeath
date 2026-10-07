//! Codex 风格配置路径常量（#1146 归位：自 `paths.rs` 抽出，经 `paths` re-export
//! 保持 `share::config::paths::*` 公共路径不变）。

pub const AGENTS_DIR_ENV: &str = "AEMEATH_AGENTS_DIR";
pub const NEW_CONFIG_FILE: &str = "aemeath.json";
pub const OLD_CONFIG_FILE: &str = "config.json";
pub const AGENTS_MD: &str = "AGENTS.md";
pub const CLAUDE_MD: &str = "CLAUDE.md";
pub const AGENTS_DIR_NAME: &str = ".agents";
pub const CLAUDE_DIR_NAME: &str = ".claude";
pub const OLD_AEMEATH_DIR_NAME: &str = ".aemeath";
pub const SKILLS_DIR_NAME: &str = "skills";
pub const MODELS_DIR_NAME: &str = "models";
pub const SYSTEMONE_DIR_NAME: &str = "systemone";
pub const LOGS_DIR_NAME: &str = "logs";
pub const GUIDANCE_DIR_NAME: &str = "guidance";
pub const MEMORY_DIR_NAME: &str = "memory";
pub const SESSIONS_DIR_NAME: &str = "sessions";
pub const WORKTREES_DIR_NAME: &str = "worktrees";
pub const HOOKS_DIR_NAME: &str = "hooks";
pub const TOOL_RESULTS_DIR_NAME: &str = "tool-results";
pub const MCP_CONFIG_FILE: &str = "mcp.json";
pub const HISTORY_FILE: &str = "history.json";
pub const SETTINGS_FILE: &str = "settings.json";
