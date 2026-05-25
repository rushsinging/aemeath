use crate::cli::Args;
use kernel::config::{Config, PermissionModeConfig};

pub(super) fn apply_config_permission_mode(args: &mut Args, config_file: Option<&Config>) {
    if args.allow_all {
        return;
    }

    if config_allows_all(config_file) {
        args.allow_all = true;
    }
}

fn config_allows_all(config_file: Option<&Config>) -> bool {
    config_file
        .map(|config| matches!(config.permissions.mode, PermissionModeConfig::AllowAll))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::config::PermissionConfig;

    fn args_with_allow_all(allow_all: bool) -> Args {
        Args {
            api_key: None,
            base_url: None,
            model: None,
            cwd: None,
            max_tokens: None,
            verbose: false,
            no_markdown: false,
            context_size: 128_000,
            resume: None,
            allow_all,
            tui: true,
            no_tui: false,
            max_tool_concurrency: None,
            max_agent_concurrency: None,
            no_think: false,
            reasoning_effort: None,
        }
    }

    fn config_with_permission_mode(mode: PermissionModeConfig) -> Config {
        Config {
            permissions: PermissionConfig {
                mode,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_apply_config_permission_mode_keeps_existing_allow_all() {
        let config = config_with_permission_mode(PermissionModeConfig::Ask);
        let mut args = args_with_allow_all(true);

        apply_config_permission_mode(&mut args, Some(&config));

        assert!(args.allow_all);
    }

    #[test]
    fn test_apply_config_permission_mode_enables_allow_all_from_config() {
        let config = config_with_permission_mode(PermissionModeConfig::AllowAll);
        let mut args = args_with_allow_all(false);

        apply_config_permission_mode(&mut args, Some(&config));

        assert!(args.allow_all);
    }

    #[test]
    fn test_apply_config_permission_mode_keeps_false_for_non_allow_all_config() {
        let config = config_with_permission_mode(PermissionModeConfig::AutoRead);
        let mut args = args_with_allow_all(false);

        apply_config_permission_mode(&mut args, Some(&config));

        assert!(!args.allow_all);
    }

    #[test]
    fn test_apply_config_permission_mode_keeps_false_without_config() {
        let mut args = args_with_allow_all(false);

        apply_config_permission_mode(&mut args, None);

        assert!(!args.allow_all);
    }
}
