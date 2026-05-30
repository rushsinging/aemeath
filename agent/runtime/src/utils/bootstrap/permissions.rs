use super::ChatBootstrapArgs;
use share::config::{Config, PermissionModeConfig};

pub fn apply_config_permission_mode(args: &mut ChatBootstrapArgs, config_file: Option<&Config>) {
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
    use share::config::PermissionConfig;

    fn args_with_allow_all(allow_all: bool) -> ChatBootstrapArgs {
        ChatBootstrapArgs {
            allow_all,
            context_size: 128_000,
            ..Default::default()
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
