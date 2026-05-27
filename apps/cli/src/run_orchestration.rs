use crate::args::Args;

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

fn initial_tui_resume_id(args: &Args) -> Option<String> {
    args.resume.clone()
}

/// 主聊天逻辑 — 瘦身入口。
pub(crate) async fn run_chat(args: Args) {
    // 初始化所有内置命令（自动注册到全局 CommandRegistry）
    ::runtime::api::command::commands::init_all();

    let args = apply_permission_env_override(args);
    let initial_resume_id = initial_tui_resume_id(&args);
    let client = ::runtime::api::client::from_args(args.into())
        .await
        .unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            std::process::exit(1);
        });

    let launch = client.tui_launch_context();
    let session_id = launch.session_id.clone();

    let mut app = crate::tui::App::new(launch.session_id.clone(), launch.cwd, launch.model_display);
    app.agent_client =
        Some(std::sync::Arc::new(client.clone()) as std::sync::Arc<dyn sdk::AgentClient>);
    app.session.memory_config = launch.memory_config;
    app.set_skills(launch.skills_map);
    app.session_reminders = launch.session_reminders;
    app.run(
        launch.client,
        launch.registry,
        launch.system_blocks,
        launch.system_prompt_text,
        launch.user_context,
        launch.context_size,
        launch.verbose,
        Some(launch.agent_runner),
        launch.allow_all,
        initial_resume_id,
        launch.task_store,
        launch.max_tool_concurrency,
        launch.max_agent_concurrency,
        launch.agent_semaphore,
    )
    .await
    .unwrap_or_else(|e| {
        log::error!("TUI error: {e}");
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
            no_markdown: false,
            context_size: 128_000,
            resume: Some("session-67".to_string()),
            allow_all: false,
            max_tool_concurrency: None,
            max_agent_concurrency: None,
            no_think: false,
            reasoning_effort: None,
        };

        assert_eq!(initial_tui_resume_id(&args).as_deref(), Some("session-67"));
    }
}
