use super::ChatBootstrapArgs;

pub fn apply_config_permission_mode(args: &mut ChatBootstrapArgs, snap_allow_all: bool) {
    if args.allow_all {
        return;
    }

    if snap_allow_all {
        args.allow_all = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_with_allow_all(allow_all: bool) -> ChatBootstrapArgs {
        ChatBootstrapArgs {
            allow_all,
            context_size: 128_000,
            ..Default::default()
        }
    }

    #[test]
    fn test_apply_config_permission_mode_keeps_existing_allow_all() {
        let mut args = args_with_allow_all(true);

        apply_config_permission_mode(&mut args, false);

        assert!(args.allow_all);
    }

    #[test]
    fn test_apply_config_permission_mode_enables_allow_all_from_snap() {
        let mut args = args_with_allow_all(false);

        apply_config_permission_mode(&mut args, true);

        assert!(args.allow_all);
    }

    #[test]
    fn test_apply_config_permission_mode_keeps_false_for_non_allow_all_snap() {
        let mut args = args_with_allow_all(false);

        apply_config_permission_mode(&mut args, false);

        assert!(!args.allow_all);
    }
}
