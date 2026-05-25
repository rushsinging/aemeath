mod prompt;
mod runtime;
mod setup;

use crate::cli::Args;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChatModeSelection {
    NoTui,
    Tui,
}

pub(super) fn chat_mode_selection(args: &Args) -> ChatModeSelection {
    if args.no_tui || !args.tui {
        ChatModeSelection::NoTui
    } else {
        ChatModeSelection::Tui
    }
}

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

    fn args_with_mode(tui: bool, no_tui: bool) -> Args {
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
            allow_all: false,
            tui,
            no_tui,
            max_tool_concurrency: None,
            max_agent_concurrency: None,
            no_think: false,
            reasoning_effort: None,
        }
    }

    #[test]
    fn test_chat_mode_selection_uses_tui_by_default() {
        let args = args_with_mode(true, false);

        assert_eq!(chat_mode_selection(&args), ChatModeSelection::Tui);
    }

    #[test]
    fn test_chat_mode_selection_no_tui_flag_wins() {
        let args = args_with_mode(true, true);

        assert_eq!(chat_mode_selection(&args), ChatModeSelection::NoTui);
    }

    #[test]
    fn test_chat_mode_selection_disabled_tui_uses_no_tui() {
        let args = args_with_mode(false, false);

        assert_eq!(chat_mode_selection(&args), ChatModeSelection::NoTui);
    }

    #[test]
    fn test_permission_env_override_only_accepts_allow_all() {
        assert!(permission_env_override(Some("allow_all")));
        assert!(!permission_env_override(Some("ask")));
        assert!(!permission_env_override(None));
    }
}
