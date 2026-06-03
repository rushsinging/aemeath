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
pub(crate) async fn build_client_from_cli_args(
    args: composition::runtime::AgentArgs,
) -> Result<std::sync::Arc<dyn sdk::AgentClient>, sdk::SdkError> {
    composition::app::build_agent_client(args).await
}

fn initial_tui_resume_id(args: &Args) -> Option<String> {
    args.resume.clone()
}

/// 主聊天逻辑 — 瘦身入口（CLI 通过 composition 装配 runtime）。
pub(crate) async fn run_chat(args: Args) {
    let args = apply_permission_env_override(args);
    let initial_resume_id: Option<String> = initial_tui_resume_id(&args);
    let bootstrap = composition::app::build_agent_bootstrap(args.into())
        .await
        .unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            std::process::exit(1);
        });
    let session_id = bootstrap.session_id.clone();

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
    app.model.runtime.apply(
        crate::tui::model::runtime::intent::RuntimeIntent::SetContextSize(
            bootstrap.context_size as u64,
        ),
    );
    app.status_bar.set_thinking(bootstrap.thinking);

    app.run(bootstrap.client, initial_resume_id)
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
