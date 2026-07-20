use std::io::IsTerminal;

const MAX_TOOL_OUTPUT_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputMode {
    PipeOnce,
    Repl,
}

pub(crate) fn input_mode(stdin_is_terminal: bool) -> InputMode {
    if stdin_is_terminal {
        InputMode::Repl
    } else {
        InputMode::PipeOnce
    }
}

pub(crate) fn resolve_slash_for_delivery(
    router: &dyn sdk::CommandRouterPort,
    input: &str,
) -> Result<sdk::CommandRoute, sdk::CommandParseError> {
    router.resolve(sdk::SlashInput::new(input))
}

pub(crate) async fn run_no_tui_chat(
    client: std::sync::Arc<dyn sdk::AgentClient>,
    session_id: String,
    command_router: std::sync::Arc<dyn sdk::CommandRouterPort>,
) -> Result<(), sdk::SdkError> {
    match input_mode(std::io::stdin().is_terminal()) {
        InputMode::PipeOnce => run_pipe_once(client, command_router).await?,
        InputMode::Repl => run_repl_loop(client, command_router).await?,
    }
    println!("aemeath --resume {session_id}");
    Ok(())
}

async fn run_pipe_once(
    client: std::sync::Arc<dyn sdk::AgentClient>,
    command_router: std::sync::Arc<dyn sdk::CommandRouterPort>,
) -> Result<(), sdk::SdkError> {
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
        .map_err(|e| sdk::SdkError::Internal(format!("读取 stdin 失败: {e}")))?;
    if input.trim().is_empty() {
        return Ok(());
    }
    run_single_turn(client, input, command_router).await
}

async fn run_repl_loop(
    client: std::sync::Arc<dyn sdk::AgentClient>,
    command_router: std::sync::Arc<dyn sdk::CommandRouterPort>,
) -> Result<(), sdk::SdkError> {
    let mut line = String::new();
    loop {
        eprint!("> ");
        std::io::Write::flush(&mut std::io::stderr())
            .map_err(|e| sdk::SdkError::Internal(format!("刷新 stderr 失败: {e}")))?;
        line.clear();
        let bytes = std::io::stdin()
            .read_line(&mut line)
            .map_err(|e| sdk::SdkError::Internal(format!("读取 stdin 失败: {e}")))?;
        if bytes == 0 {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        if line.trim_start().starts_with('/') {
            match resolve_slash_for_delivery(command_router.as_ref(), &line) {
                Ok(sdk::CommandRoute::ApplicationControl { command, .. })
                    if command.command.as_str() == "exit" =>
                {
                    break;
                }
                Err(error) => {
                    eprintln!("{error}");
                    continue;
                }
                _ => {}
            }
        }
        run_single_turn(
            client.clone(),
            line.trim_end().to_string(),
            command_router.clone(),
        )
        .await?;
        println!();
    }
    Ok(())
}

async fn run_single_turn(
    client: std::sync::Arc<dyn sdk::AgentClient>,
    text: String,
    command_router: std::sync::Arc<dyn sdk::CommandRouterPort>,
) -> Result<(), sdk::SdkError> {
    let reflection_limit = if text.trim_start().starts_with('/') {
        match resolve_slash_for_delivery(command_router.as_ref(), &text) {
            Ok(sdk::CommandRoute::SnapshotQuery { command, .. })
                if command.command.as_str() == "reflect" =>
            {
                command
                    .arguments
                    .as_slice()
                    .first()
                    .and_then(|value| value.parse::<usize>().ok())
            }
            Ok(_) => {
                eprintln!("该命令暂不支持 no-TUI 执行。");
                return Ok(());
            }
            Err(error) => {
                eprintln!("{error}");
                return Ok(());
            }
        }
    } else {
        None
    };
    let (user_input, input_events) = if let Some(limit) = reflection_limit {
        let (tx, port) = crate::tui::effect::session::processing::TuiInputEventPort::channel();
        let _ = tx.send(sdk::ChatInputEvent::QueryReflectionHistory { limit });
        (None, Some(std::sync::Arc::new(port) as _))
    } else {
        (
            Some(sdk::UserInput {
                text,
                images: Vec::new(),
            }),
            None,
        )
    };
    let mut stream = client
        .chat(sdk::ChatRequest {
            user_input,
            queue_drain: None,
            input_events,
        })
        .await?;
    // #636 D1: SIGTERM/SIGHUP 时让 stream 自然结束（runtime 端会 graceful + auto-save）。
    let mut sig_term =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
    let mut sig_hup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup()).ok();
    loop {
        let event = tokio::select! {
            biased;
            ev = stream.recv() => match ev { Some(e) => e, None => break, },
            _ = async { match &mut sig_term { Some(s) => s.recv().await, None => std::future::pending().await } } => {
                log::info!(target: crate::LOG_TARGET, "no_tui: received SIGTERM, draining stream");
                continue;
            }
            _ = async { match &mut sig_hup { Some(s) => s.recv().await, None => std::future::pending().await } } => {
                log::info!(target: crate::LOG_TARGET, "no_tui: received SIGHUP, draining stream");
                continue;
            }
        };
        crate::tui::effect::session::processing::log_sdk_event(&event, "no_tui.recv");
        render_event(event)?;
    }
    Ok(())
}

fn render_event(event: sdk::ChatEvent) -> Result<(), sdk::SdkError> {
    match event {
        sdk::ChatEvent::Token { text, .. } => print_stdout(&text)?,
        sdk::ChatEvent::BlockComplete { .. } => {}
        sdk::ChatEvent::Thinking { .. }
        | sdk::ChatEvent::ModelStreamWaiting { .. }
        | sdk::ChatEvent::ModelInvocationRetrying { .. }
        | sdk::ChatEvent::TurnStarted { .. }
        | sdk::ChatEvent::MicrocompactDone { .. }
        | sdk::ChatEvent::StopHookBlocked { .. }
        | sdk::ChatEvent::PostToolExecutionSync { .. }
        | sdk::ChatEvent::CompactRollback { .. }
        | sdk::ChatEvent::CompactFinished { .. }
        | sdk::ChatEvent::UserMessagesAdopted { .. }
        | sdk::ChatEvent::UserMessagesQueued { .. }
        | sdk::ChatEvent::RunStarted { .. }
        | sdk::ChatEvent::RunStepStarted { .. }
        | sdk::ChatEvent::RunStepCompleted { .. }
        | sdk::ChatEvent::RunStepCancellationRequested { .. }
        | sdk::ChatEvent::RunStepFinalizationStarted { .. }
        | sdk::ChatEvent::RunStepCancelled { .. }
        | sdk::ChatEvent::RunDrainingInput { .. }
        | sdk::ChatEvent::RunTerminationRequested { .. }
        | sdk::ChatEvent::RunTerminated { .. }
        | sdk::ChatEvent::RunCompleted { .. }
        | sdk::ChatEvent::RunFailed { .. }
        | sdk::ChatEvent::RunStuckDetected { .. }
        | sdk::ChatEvent::RunTransitioned { .. }
        | sdk::ChatEvent::RunAwaitingUser { .. }
        | sdk::ChatEvent::RunResumed { .. }
        | sdk::ChatEvent::InteractionRequested { .. }
        | sdk::ChatEvent::RunCancelling { .. }
        | sdk::ChatEvent::RunCancelled { .. }
        | sdk::ChatEvent::Usage { .. }
        | sdk::ChatEvent::LiveTps(_)
        | sdk::ChatEvent::TurnChanged(_)
        | sdk::ChatEvent::CurrentTurnChanged(_)
        | sdk::ChatEvent::HookEvent(_)
        | sdk::ChatEvent::HookMessage(_)
        | sdk::ChatEvent::WorkingDirectoryChanged { .. }
        | sdk::ChatEvent::ConfigChanged { .. }
        | sdk::ChatEvent::ConfigReloaded { .. }
        | sdk::ChatEvent::SessionReset
        | sdk::ChatEvent::UserMessagesWithdrawn { .. }
        | sdk::ChatEvent::GraphPhaseChanged { .. }
        | sdk::ChatEvent::CompactProgress { .. }
        | sdk::ChatEvent::ModelSwitched { .. }
        | sdk::ChatEvent::ThinkingChanged { .. }
        | sdk::ChatEvent::ContextEstimated { .. }
        | sdk::ChatEvent::CommandResultText { .. }
        | sdk::ChatEvent::SessionResumed { .. }
        | sdk::ChatEvent::ToolCallUpdate { .. }
        | sdk::ChatEvent::ModelList { .. }
        | sdk::ChatEvent::ReminderList { .. }
        | sdk::ChatEvent::SessionList { .. }
        | sdk::ChatEvent::ProjectInfo { .. }
        | sdk::ChatEvent::TasksSnapshot { .. }
        | sdk::ChatEvent::CostUpdate { .. } => {}
        sdk::ChatEvent::ReflectionHistory { records } => {
            eprintln!("Reflection history ({}):", records.len());
            for record in records {
                let tokens = record.token_usage.map_or_else(
                    || "n/a".to_string(),
                    |usage| format!("{}/{}", usage.input_tokens, usage.output_tokens),
                );
                let error = record
                    .error_category
                    .map_or_else(|| "none".to_string(), |category| format!("{category:?}"));
                eprintln!(
                    "- timestamp={} trigger={:?} status={:?} counts(deviations/suggestions/outdated)={}/{}/{} apply={:?} error={} tokens(in/out)={} duration={}ms",
                    record.timestamp,
                    record.trigger,
                    record.status,
                    record.deviations,
                    record.suggestions,
                    record.outdated,
                    record.apply_status,
                    error,
                    tokens,
                    record.duration_ms,
                );
            }
        }
        sdk::ChatEvent::ApiError { error, .. } => {
            eprintln!("\n  ✗ API 错误: {error}");
        }
        sdk::ChatEvent::ToolResult {
            tool_name,
            output,
            is_error,
            ..
        } => {
            let output = truncate_tool_output(&output);
            if is_error {
                eprintln!("[tool:{tool_name}:error] {output}");
            } else {
                eprintln!("[tool:{tool_name}] {output}");
            }
        }
        sdk::ChatEvent::SystemMessage(message) => {
            eprintln!("{message}");
        }
        sdk::ChatEvent::Result(result) => {
            print_stdout(&result.text)?;
        }
        sdk::ChatEvent::Done { .. }
        | sdk::ChatEvent::DoneWithDurationMs { .. }
        | sdk::ChatEvent::Cancelled { .. } => {}
        sdk::ChatEvent::ToolCallStart { name, .. } => {
            eprintln!("[tool:start] {name}");
        }
        sdk::ChatEvent::AskUserBatch { items, reply_tx } => {
            let mut answers = Vec::new();
            for item in items {
                let reply = read_ask_user_reply(
                    &item.question,
                    &item.options,
                    item.allow_free_input,
                    item.default.as_deref(),
                )?;
                answers.push(reply);
            }
            let _ = reply_tx.send(sdk::AskUserReply::Answers(answers));
        }
        sdk::ChatEvent::AgentProgress { event, .. } => {
            eprintln!("[agent] {event}");
        }
        sdk::ChatEvent::SessionResumeFailed { kind, id, message } => {
            use sdk::SessionResumeFailureKind;
            let label = match kind {
                SessionResumeFailureKind::NotFound => "session 不存在",
                SessionResumeFailureKind::Corrupt => "session 文件损坏",
                SessionResumeFailureKind::Io => "IO 错误",
            };
            eprintln!("⚠️  恢复失败 [{label}] id={id}: {message}");
            eprintln!("    用 `/sessions` 查看可用会话，或直接开始新会话。");
        }
    }
    Ok(())
}

fn print_stdout(text: &str) -> Result<(), sdk::SdkError> {
    print!("{text}");
    std::io::Write::flush(&mut std::io::stdout())
        .map_err(|e| sdk::SdkError::Internal(format!("刷新 stdout 失败: {e}")))
}

fn truncate_tool_output(output: &str) -> String {
    let mut truncated: String = output.chars().take(MAX_TOOL_OUTPUT_CHARS).collect();
    if truncated.len() < output.len() {
        truncated.push_str("... (truncated)");
    }
    truncated
}

fn read_ask_user_reply(
    question: &str,
    options: &[sdk::OptionItem],
    allow_free_input: bool,
    default: Option<&str>,
) -> Result<String, sdk::SdkError> {
    eprintln!("[ask-user] {question}");
    for (index, option) in options.iter().enumerate() {
        eprintln!("  {}. {}", index + 1, option.title);
        if let Some(description) = &option.description {
            if !description.is_empty() {
                eprintln!("     {description}");
            }
        }
    }
    if let Some(default) = default {
        eprintln!("default: {default}");
    }
    if allow_free_input || options.is_empty() {
        eprint!("answer> ");
    } else {
        eprint!("choice> ");
    }
    std::io::Write::flush(&mut std::io::stderr())
        .map_err(|e| sdk::SdkError::Internal(format!("刷新 stderr 失败: {e}")))?;

    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|e| sdk::SdkError::Internal(format!("读取 stdin 失败: {e}")))?;
    let answer = answer.trim().to_string();
    if answer.is_empty() {
        return Ok(default.unwrap_or_default().to_string());
    }
    if allow_free_input || options.is_empty() {
        return Ok(answer);
    }
    parse_option_answer(&answer, options)
        .or_else(|| default.map(ToOwned::to_owned))
        .ok_or_else(|| sdk::SdkError::Internal(format!("无效选项: {answer}")))
}

fn parse_option_answer(answer: &str, options: &[sdk::OptionItem]) -> Option<String> {
    let index = answer.parse::<usize>().ok()?;
    options
        .get(index.checked_sub(1)?)
        .map(|option| option.title.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_mode_uses_repl_for_terminal_stdin() {
        assert_eq!(input_mode(true), InputMode::Repl);
    }

    #[test]
    fn test_input_mode_uses_pipe_once_for_non_terminal_stdin() {
        assert_eq!(input_mode(false), InputMode::PipeOnce);
    }

    #[test]
    fn no_tui_uses_injected_router_for_exit_alias_and_reflect_arguments() {
        let wiring = composition::tools::wire_commands().expect("command wiring");
        assert!(matches!(
            resolve_slash_for_delivery(wiring.router().as_ref(), "  /quit  "),
            Ok(sdk::CommandRoute::ApplicationControl { command, .. })
                if command.command.as_str() == "exit"
        ));
        assert!(matches!(
            resolve_slash_for_delivery(wiring.router().as_ref(), "/reflect 3"),
            Ok(sdk::CommandRoute::SnapshotQuery { command, .. })
                if command.arguments.as_slice() == ["3"]
        ));
        assert!(resolve_slash_for_delivery(wiring.router().as_ref(), "/reflect 0").is_err());
    }

    #[test]
    fn test_truncate_tool_output_keeps_short_output() {
        assert_eq!(truncate_tool_output("short"), "short");
    }

    #[test]
    fn test_truncate_tool_output_marks_long_output() {
        let output = "x".repeat(MAX_TOOL_OUTPUT_CHARS + 1);

        assert!(truncate_tool_output(&output).ends_with("... (truncated)"));
    }

    #[test]
    fn test_parse_option_answer_returns_title_by_one_based_index() {
        let options = vec![sdk::OptionItem {
            title: "yes".to_string(),
            description: None,
        }];

        assert_eq!(parse_option_answer("1", &options).as_deref(), Some("yes"));
    }

    #[test]
    fn test_parse_option_answer_rejects_invalid_index() {
        let options = vec![sdk::OptionItem {
            title: "yes".to_string(),
            description: None,
        }];

        assert_eq!(parse_option_answer("2", &options), None);
    }
}
