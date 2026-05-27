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

/// 从 CLI args 创建 AgentClient（原 runtime_adapter::agent_client_from_args）。
pub(crate) async fn agent_client_from_args(
    args: sdk::ChatBootstrapArgs,
) -> Result<std::sync::Arc<dyn sdk::AgentClient>, sdk::SdkError> {
    Ok(std::sync::Arc::new(
        ::runtime::api::client::from_args(args).await?,
    ))
}

/// 记录当前 turn（原 runtime_adapter::set_current_turn）。
pub(crate) fn set_current_turn(turn: usize) {
    ::runtime::api::bootstrap::set_current_turn(turn);
}

fn initial_tui_resume_id(args: &Args) -> Option<String> {
    args.resume.clone()
}

/// 主聊天逻辑 — 瘦身入口（CLI 唯一接触 runtime::api 的装配层）。
pub(crate) async fn run_chat(args: Args) {
    // 初始化所有内置命令（自动注册到全局 CommandRegistry）
    ::runtime::api::command::commands::init_all();

    let args = apply_permission_env_override(args);
    let initial_resume_id: Option<String> = initial_tui_resume_id(&args);
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

    // 在 run() 之前设置启动上下文（替代 18 参数注入）
    app.status_bar
        .set_permission_mode(if launch.allow_all { "AllowAll" } else { "AskMe" });
    app.chat.context_size = launch.context_size;
    app.status_bar.set_context_size(launch.context_size as u64);
    app.status_bar.set_thinking(launch.client.is_reasoning());

    app.run(
        std::sync::Arc::new(client) as std::sync::Arc<dyn sdk::AgentClient>,
        initial_resume_id,
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
