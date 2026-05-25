mod prompt;
mod runtime;
mod setup;

use crate::cli::Args;
use ::runtime::api::bootstrap::ChatModeSelection;

pub(super) fn permission_env_override(mode: Option<&str>) -> bool {
    matches!(mode, Some("allow_all"))
}

pub(super) fn apply_permission_env_override(mut args: Args) -> Args {
    if !args.allow_all
        && permission_env_override(std::env::var("AEMEATH_PERMISSION_MODE").ok().as_deref())
    {
        args.allow_all = true;
    }
    args
}

/// 主聊天逻辑（原 main 主体）
pub(crate) async fn run_chat(args: Args) {
    // 初始化所有内置命令（自动注册到全局 CommandRegistry）
    ::runtime::api::core::command::commands::init_all();

    let args = apply_permission_env_override(args);
    let bootstrap = setup::bootstrap_chat(args).await;

    match bootstrap.mode_selection {
        ChatModeSelection::NoTui => runtime::run_no_tui_from_bootstrap(bootstrap).await,
        ChatModeSelection::Tui => runtime::run_tui_from_bootstrap(bootstrap).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_env_override_only_accepts_allow_all() {
        assert!(permission_env_override(Some("allow_all")));
        assert!(!permission_env_override(Some("ask")));
        assert!(!permission_env_override(None));
    }
}
