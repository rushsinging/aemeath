use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogPaths {
    pub app_log: PathBuf,
    pub panic_log: PathBuf,
    pub agent_log: PathBuf,
}

pub fn default_log_paths(home: impl Into<PathBuf>) -> LogPaths {
    let base = home.into().join(".aemeath");
    LogPaths {
        app_log: base.join("aemeath.log"),
        panic_log: base.join("panic.log"),
        agent_log: base.join("agent.log"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_log_paths_use_aemeath_directory() {
        let paths = default_log_paths("/tmp/user");

        assert_eq!(
            paths.app_log,
            PathBuf::from("/tmp/user/.aemeath/aemeath.log")
        );
        assert_eq!(
            paths.panic_log,
            PathBuf::from("/tmp/user/.aemeath/panic.log")
        );
        assert_eq!(
            paths.agent_log,
            PathBuf::from("/tmp/user/.aemeath/agent.log")
        );
    }
}
