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

fn model_display(source_key: &str, model_name: &str, model_id: &str) -> String {
    let display_name = if model_name.is_empty() {
        model_id
    } else {
        model_name
    };
    format!("{}/{}", source_key, display_name)
}

/// 主聊天逻辑 — 瘦身入口。
pub(crate) async fn run_chat(args: Args) {
    // 初始化所有内置命令（自动注册到全局 CommandRegistry）
    ::runtime::api::command::commands::init_all();

    let args = apply_permission_env_override(args);
    let client = ::runtime::api::client::from_args(args.into())
        .await
        .unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            std::process::exit(1);
        });

    let ctx = client.context().clone();
    let session_id = client.session_id().to_string();
    let cwd = client.cwd().to_path_buf();
    let model = client.resolved_model();
    let model_disp = model_display(&model.source_key, &model.model.name, &model.model.id);
    let max_tool = client.max_tool_concurrency();
    let max_agent = client.max_agent_concurrency();
    let context_window = model.model.context_window;

    let mut app = crate::tui::App::new(session_id.clone(), cwd, model_disp);
    app.session.memory_config = ctx.memory_config;
    app.set_skills(ctx.skills_map);
    app.cmd_exec.hook_runner = ctx.hook_runner;
    app.cmd_exec.json_logger = ctx.json_logger;
    app.run(
        ctx.client,
        ctx.registry,
        ctx.system_blocks,
        ctx.system_prompt_text,
        ctx.user_context,
        context_window,
        false, // verbose
        true,  // markdown
        Some(ctx.agent_runner),
        ctx.allow_all,
        None,  // resume
        ctx.task_store,
        max_tool,
        max_agent,
        ctx.agent_semaphore,
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
    fn test_model_display_uses_name_when_present() {
        assert_eq!(
            model_display("zhipu", "GLM-5.1", "glm-5.1"),
            "zhipu/GLM-5.1"
        );
    }

    #[test]
    fn test_model_display_falls_back_to_id_when_name_empty() {
        assert_eq!(
            model_display("openai", "", "gpt-4o"),
            "openai/gpt-4o"
        );
    }

    #[test]
    fn test_model_display_empty_source_still_formats() {
        assert_eq!(model_display("", "Claude", "claude-3"), "/Claude");
    }
}
