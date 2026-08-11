use crate::args::Args;

pub(crate) mod no_tui;

/// 从 CLI args 创建 AgentClient（原 runtime_adapter::agent_client_from_args）。
pub(crate) async fn build_client_from_cli_args(
    args: composition::runtime::AgentArgs,
) -> Result<std::sync::Arc<dyn sdk::AgentClient>, sdk::SdkError> {
    composition::app::build_agent_client(args).await
}

fn initial_tui_resume_backing(
    bootstrap: &composition::app::AgentClientBootstrap,
) -> Option<sdk::LocalSessionResumeBacking> {
    bootstrap.startup_resume.clone()
}

fn should_emit_cli_frontend_started_log() -> bool {
    true
}

fn should_emit_quiet_cli_diagnostic_log(quiet: bool) -> bool {
    quiet
}

async fn run_frontend_with_audit_drain<F, Fut, DrainFuture, Error>(
    client: std::sync::Arc<dyn sdk::AgentClient>,
    drain: Option<DrainFuture>,
    frontend: F,
) -> Result<(), Error>
where
    F: FnOnce(std::sync::Arc<dyn sdk::AgentClient>) -> Fut,
    Fut: std::future::Future<Output = Result<(), Error>>,
    DrainFuture: std::future::Future<Output = ()>,
{
    let result = frontend(client).await;
    if let Some(drain) = drain {
        drain.await;
    }
    result
}

/// 主聊天逻辑 — 瘦身入口（CLI 通过 composition 装配 runtime）。
pub(crate) async fn run_chat(args: Args) {
    let quiet = args.quiet;
    let bootstrap = composition::app::build_agent_bootstrap(args.into())
        .await
        .unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            std::process::exit(1);
        });
    let session_id = bootstrap.session_id.clone();
    let frontend_context = composition::delivery_logging::create_session_scope(
        composition::delivery_logging::capture(),
        &session_id,
    );
    composition::delivery_logging::instrument(frontend_context, async move {
        // #636 D3: session lock —— 防止两个 aemeath 实例同时操作同一 session。
        let _session_lock = match crate::session_lock::try_acquire_or_prompt(&session_id, quiet) {
            Ok(lock) => lock,
            Err(crate::session_lock::AcquireError::Denied) => {
                std::process::exit(4);
            }
            Err(e) => {
                eprintln!("Error: session lock acquire failed: {e}");
                std::process::exit(1);
            }
        };
        if should_emit_cli_frontend_started_log() {
            crate::tui::log_info!("chat frontend started: quiet={quiet} session={session_id}");
        }

        if quiet {
            if should_emit_quiet_cli_diagnostic_log(quiet) {
                crate::tui::log_info!("quiet chat started: session={session_id}");
            }
            let client = bootstrap.client.clone();
            let command_router = bootstrap.command_router.clone();
            let quiet_session_id = session_id.clone();
            run_frontend_with_audit_drain(
                client,
                bootstrap.session_audit.as_ref().map(|session_audit| async move {
                    let _ = session_audit.shutdown().await;
                }),
                move |client| async move {
                    crate::chat::no_tui::run_no_tui_chat(
                        client,
                        quiet_session_id,
                        command_router,
                    )
                    .await
                },
            )            .await
            .unwrap_or_else(|error| {
                eprintln!("Error: {error}");
                std::process::exit(1);
            });
            return;
        }

        let startup_resume = initial_tui_resume_backing(&bootstrap);
        let mut app =
            crate::tui::App::new(bootstrap.session_id, bootstrap.cwd, bootstrap.model_display);
        app.agent_client = Some(bootstrap.client.clone());
        app.run_control_client = Some(bootstrap.run_control_client.clone());
        app.display_history_query = Some(bootstrap.display_history_query.clone());
        app.user_agent = bootstrap.user_agent;        app.config_view = bootstrap.config_view.clone();
        app.apply_agent_intent(
            crate::tui::update::intent::AgentIntent::UiPreferences(
                crate::tui::model::ui_preferences::UiPreferencesIntent::MarkdownSpacingChanged(
                    crate::tui::render::output::spacing::MarkdownSpacingPolicy::from(
                        &bootstrap.config_view,
                    ),
                ),
            ),
        );
        app.session.memory_config = bootstrap.memory_config;
        app.set_skill_snapshot(bootstrap.skill_snapshot);
        app.set_commands(bootstrap.command_catalog, bootstrap.command_router);
        // 在 run() 之前设置启动上下文（替代 18 参数注入）
        app.apply_agent_intent(
            crate::tui::update::intent::AgentIntent::RuntimePresentation(
                crate::tui::model::runtime_presentation::RuntimePresentationIntent::ContextSize(
                    bootstrap.context_size as u64,
                ),
            ),
        );
        app.apply_agent_intent(
            crate::tui::update::intent::AgentIntent::RuntimePresentation(
                crate::tui::model::runtime_presentation::RuntimePresentationIntent::Thinking(
                    bootstrap.thinking,
                ),
            ),
        );
        if let Some(resume) = startup_resume {
            crate::tui::log_debug!(
                "resume_lifecycle boundary=cli_to_tui stage=startup_view_received session_id={} steps={} messages={}",
                resume.session_id,
                resume.steps.len(),
                resume
                    .steps
                    .iter()
                    .map(|step| step.messages().count())
                    .sum::<usize>()
            );
            app.restore_startup_backing(resume);
        } else {
            crate::tui::log_debug!(
                "resume_lifecycle boundary=cli_to_tui stage=startup_view_absent session_id={}",
                app.session.session_id()
            );
        }
        let client = bootstrap.client.clone();
        run_frontend_with_audit_drain(
            client,
            bootstrap.session_audit.as_ref().map(|session_audit| async move {
                let _ = session_audit.shutdown().await;
            }),
            move |client| async move { app.run(client).await },
        )        .await
        .unwrap_or_else(|error| {
            crate::tui::log_error!("TUI error: {error}");
            std::process::exit(1);
        });
        println!("aemeath --resume {}", session_id);
    })
    .await;
}

#[cfg(test)]
#[path = "chat_tests.rs"]
mod tests;
