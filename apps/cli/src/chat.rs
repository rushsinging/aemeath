use crate::args::Args;

pub(crate) mod no_tui;

/// 从 CLI args 创建 AgentClient（原 runtime_adapter::agent_client_from_args）。
pub(crate) async fn build_client_from_cli_args(
    args: composition::runtime::AgentArgs,
) -> Result<std::sync::Arc<dyn sdk::AgentClient>, sdk::SdkError> {
    composition::app::build_agent_client(args).await
}

fn initial_tui_resume_id(args: &Args) -> Option<String> {
    args.resume.clone()
}

fn should_emit_cli_frontend_started_log() -> bool {
    true
}

fn should_emit_quiet_cli_diagnostic_log(quiet: bool) -> bool {
    quiet
}

/// 主聊天逻辑 — 瘦身入口（CLI 通过 composition 装配 runtime）。
pub(crate) async fn run_chat(args: Args) {
    let quiet = args.quiet;
    let initial_resume_id: Option<String> = initial_tui_resume_id(&args);
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
            crate::chat::no_tui::run_no_tui_chat(
                bootstrap.client,
                session_id,
                bootstrap.command_router,
            )
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
        app.user_agent = bootstrap.user_agent;
        app.session.memory_config = bootstrap.memory_config;
        app.set_skills(bootstrap.skills_map);
        app.set_commands(bootstrap.command_catalog, bootstrap.command_router);

        // 在 run() 之前设置启动上下文（替代 18 参数注入）
        app.status_bar.set_permission_mode(if bootstrap.allow_all {
            "AllowAll"
        } else {
            "AskMe"
        });
        app.apply_agent_intent(crate::tui::update::intent::AgentIntent::Config(
            crate::tui::model::config_provider::ConfigIntent::SetContextSize(
                bootstrap.context_size as u64,
            ),
        ));
        app.apply_agent_intent(crate::tui::update::intent::AgentIntent::Config(
            crate::tui::model::config_provider::ConfigIntent::SetThinking(bootstrap.thinking),
        ));
        app.run(bootstrap.client, initial_resume_id)
            .await
            .unwrap_or_else(|e| {
                crate::tui::log_error!("TUI error: {e}");
                std::process::exit(1);
            });
        println!("aemeath --resume {}", session_id);
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn complete_context(session_id: &str) -> composition::delivery_logging::LogContext {
        composition::delivery_logging::LogContext {
            session_id: Some(session_id.to_string()),
            chat_id: Some("runtime-chat".to_string()),
            turn: Some(7),
            request_id: Some("request-42".to_string()),
            model: Some("model-1".to_string()),
            provider: Some("provider-1".to_string()),
            role: Some("worker".to_string()),
        }
    }

    #[test]
    fn tui_session_context_replaces_parent_with_session_only() {
        let context = composition::delivery_logging::create_session_scope(
            complete_context("parent-session"),
            "bootstrap-session",
        );

        assert_eq!(
            context,
            composition::delivery_logging::LogContext {
                session_id: Some("bootstrap-session".to_string()),
                ..composition::delivery_logging::LogContext::default()
            }
        );
    }

    #[tokio::test]
    async fn concurrent_tui_session_scopes_do_not_leak() {
        composition::delivery_logging::instrument(complete_context("parent-session"), async {
            let first = tokio::spawn(composition::delivery_logging::instrument(
                composition::delivery_logging::create_session_scope(
                    composition::delivery_logging::capture(),
                    "session-a",
                ),
                async {
                    tokio::task::yield_now().await;
                    composition::delivery_logging::capture()
                },
            ));
            let second = tokio::spawn(composition::delivery_logging::instrument(
                composition::delivery_logging::create_session_scope(
                    composition::delivery_logging::capture(),
                    "session-b",
                ),
                async {
                    tokio::task::yield_now().await;
                    composition::delivery_logging::capture()
                },
            ));

            assert_eq!(
                first.await.unwrap(),
                composition::delivery_logging::LogContext {
                    session_id: Some("session-a".to_string()),
                    ..composition::delivery_logging::LogContext::default()
                }
            );
            assert_eq!(
                second.await.unwrap(),
                composition::delivery_logging::LogContext {
                    session_id: Some("session-b".to_string()),
                    ..composition::delivery_logging::LogContext::default()
                }
            );
            assert_eq!(
                composition::delivery_logging::capture(),
                complete_context("parent-session")
            );
        })
        .await;
    }

    #[tokio::test]
    async fn tui_session_scope_exit_restores_complete_parent_scope() {
        let parent = complete_context("parent-session");
        composition::delivery_logging::instrument(parent.clone(), async {
            composition::delivery_logging::instrument(
                composition::delivery_logging::create_session_scope(
                    composition::delivery_logging::capture(),
                    "bootstrap-session",
                ),
                async {
                    assert_eq!(
                        composition::delivery_logging::capture(),
                        composition::delivery_logging::LogContext {
                            session_id: Some("bootstrap-session".to_string()),
                            ..composition::delivery_logging::LogContext::default()
                        }
                    );
                },
            )
            .await;

            assert_eq!(composition::delivery_logging::capture(), parent);
        })
        .await;
    }
}
