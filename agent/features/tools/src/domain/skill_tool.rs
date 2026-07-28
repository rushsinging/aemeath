use std::collections::BTreeSet;
use std::path::PathBuf;

/// Per-Run frozen values needed to resolve a Skill invocation.
///
/// The workspace root deliberately stays live and is read from
/// [`ToolExecutionContext::workspace_read`] at call time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillQuerySnapshot {
    pub extra_dirs: Vec<PathBuf>,
    pub available_tools: BTreeSet<String>,
}
