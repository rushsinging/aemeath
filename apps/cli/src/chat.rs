use crate::args::Args;

pub(crate) mod no_tui;

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

/// 从 CLI args 创建 AgentClient（原 runtime_adapter::agent_client_from_args）。
pub(crate) async fn build_client_from_cli_args(
    args: composition::runtime::AgentArgs,
) -> Result<std::sync::Arc<dyn sdk::AgentClient>, sdk::SdkError> {
    composition::app::build_agent_client(args).await
}

fn initial_tui_resume_id(args: &Args) -> Option<String> {
    args.resume.clone()
}

fn should_clear_stderr_log_env(quiet: bool, verbose: bool) -> bool {
    quiet && !verbose
}

fn should_set_stderr_log_env(verbose: bool) -> bool {
    verbose
}

fn should_emit_cli_frontend_started_log() -> bool {
    true
}

fn should_emit_quiet_cli_diagnostic_log(quiet: bool) -> bool {
    quiet
}

/// 主聊天逻辑 — 瘦身入口（CLI 通过 composition 装配 runtime）。
pub(crate) async fn run_chat(args: Args) {
    if should_set_stderr_log_env(args.verbose) {
        std::env::set_var("AEMEATH_LOG_STDERR", "1");
    } else if should_clear_stderr_log_env(args.quiet, args.verbose) {
        std::env::remove_var("AEMEATH_LOG_STDERR");
    }
    let args = apply_permission_env_override(args);
    let quiet = args.quiet;
    let initial_resume_id: Option<String> = initial_tui_resume_id(&args);
    let bootstrap = composition::app::build_agent_bootstrap(args.into())
        .await
        .unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            std::process::exit(1);
        });
    let session_id = bootstrap.session_id.clone();
    if should_emit_cli_frontend_started_log() {
        crate::tui::log_info!("chat frontend started: quiet={quiet} session={session_id}");
    }

    if quiet {
        if should_emit_quiet_cli_diagnostic_log(quiet) {
            crate::tui::log_info!("quiet chat started: session={session_id}");
        }
        crate::chat::no_tui::run_no_tui_chat(bootstrap.client, session_id)
            .await
            .unwrap_or_else(|e| {
                eprintln!("Error: {e}");
                std::process::exit(1);
            });
        return;
    }

    let mut app =
        crate::tui::App::new(bootstrap.session_id, bootstrap.cwd, bootstrap.model_display);
    app.agent_client = Some(bootstrap.client.clone());
    app.session.memory_config = bootstrap.memory_config;
    app.set_skills(bootstrap.skills_map);

    // 在 run() 之前设置启动上下文（替代 18 参数注入）
    app.status_bar.set_permission_mode(if bootstrap.allow_all {
        "AllowAll"
    } else {
        "AskMe"
    });
    app.chat.context_size = bootstrap.context_size;
    app.model
        .conversation
        .apply(crate::tui::model::conversation::intent::SetContextSize(
            bootstrap.context_size as u64,
        ));
    app.model
        .conversation
        .apply(crate::tui::model::conversation::intent::SetThinking(
            bootstrap.thinking,
        ));
    app.run(bootstrap.client, initial_resume_id)
        .await
        .unwrap_or_else(|e| {
            crate::tui::log_error!("TUI error: {e}");
            std::process::exit(1);
        });
    println!("aemeath --resume {}", session_id);
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

    #[test]
    fn test_initial_tui_resume_id_uses_cli_resume() {
        let args = Args {
            api_key: None,
            base_url: None,
            model: None,
            cwd: None,
            max_tokens: None,
            verbose: false,
            quiet: false,
            no_markdown: false,
            context_size: 128_000,
            resume: Some("session-67".to_string()),
            allow_all: false,
            max_tool_concurrency: None,
            max_agent_concurrency: None,
            no_think: false,
            max_reasoning: None,
        };

        assert_eq!(initial_tui_resume_id(&args).as_deref(), Some("session-67"));
    }

    #[test]
    fn test_should_clear_stderr_log_env_for_quiet_mode() {
        assert!(should_clear_stderr_log_env(true, false));
    }

    #[test]
    fn test_should_clear_stderr_log_env_keeps_user_choice_for_tui_mode() {
        assert!(!should_clear_stderr_log_env(false, false));
    }

    #[test]
    fn test_should_clear_stderr_log_env_verbose_overrides_quiet() {
        assert!(!should_clear_stderr_log_env(true, true));
    }

    #[test]
    fn test_should_set_stderr_log_env_for_verbose() {
        assert!(should_set_stderr_log_env(true));
    }

    #[test]
    fn test_should_set_stderr_log_env_skips_non_verbose() {
        assert!(!should_set_stderr_log_env(false));
    }

    #[test]
    fn test_should_emit_cli_frontend_started_log() {
        assert!(should_emit_cli_frontend_started_log());
    }

    #[test]
    fn test_should_emit_quiet_cli_diagnostic_log_for_quiet_mode() {
        assert!(should_emit_quiet_cli_diagnostic_log(true));
    }

    #[test]
    fn test_should_emit_quiet_cli_diagnostic_log_skips_tui_mode() {
        assert!(!should_emit_quiet_cli_diagnostic_log(false));
    }
}
