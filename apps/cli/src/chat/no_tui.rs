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

pub(crate) fn is_exit_command(input: &str) -> bool {
    matches!(input.trim(), "/exit" | "/quit")
}

pub(crate) async fn run_no_tui_chat(
    client: std::sync::Arc<dyn sdk::AgentClient>,
    session_id: String,
) -> Result<(), sdk::SdkError> {
    match input_mode(std::io::stdin().is_terminal()) {
        InputMode::PipeOnce => run_pipe_once(client).await?,
        InputMode::Repl => run_repl_loop(client).await?,
    }
    println!("aemeath --resume {session_id}");
    Ok(())
}

async fn run_pipe_once(client: std::sync::Arc<dyn sdk::AgentClient>) -> Result<(), sdk::SdkError> {
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
        .map_err(|e| sdk::SdkError::Internal(format!("读取 stdin 失败: {e}")))?;
    if input.trim().is_empty() {
        return Ok(());
    }
    run_single_turn(client, input).await
}

async fn run_repl_loop(client: std::sync::Arc<dyn sdk::AgentClient>) -> Result<(), sdk::SdkError> {
    let mut line = String::new();
    loop {
        eprint!("> ");
        std::io::Write::flush(&mut std::io::stderr())
            .map_err(|e| sdk::SdkError::Internal(format!("刷新 stderr 失败: {e}")))?;
        line.clear();
        let bytes = std::io::stdin()
            .read_line(&mut line)
            .map_err(|e| sdk::SdkError::Internal(format!("读取 stdin 失败: {e}")))?;
        if bytes == 0 || is_exit_command(&line) {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        run_single_turn(client.clone(), line.trim_end().to_string()).await?;
        println!();
    }
    Ok(())
}

async fn run_single_turn(
    client: std::sync::Arc<dyn sdk::AgentClient>,
    text: String,
) -> Result<(), sdk::SdkError> {
    let mut stream = client
        .chat_text(sdk::ChatInput {
            text,
            image_paths: Vec::new(),
        })
        .await?;
    while let Some(event) = stream.recv().await {
        render_event(event)?;
    }
    Ok(())
}

fn render_event(event: sdk::ChatEvent) -> Result<(), sdk::SdkError> {
    match event {
        sdk::ChatEvent::Token { text, .. } => print_stdout(&text)?,
        sdk::ChatEvent::BlockComplete { .. } => {}
        sdk::ChatEvent::Thinking { .. }
        | sdk::ChatEvent::MessagesSync(_)
        | sdk::ChatEvent::Usage { .. }
        | sdk::ChatEvent::LiveTps(_)
        | sdk::ChatEvent::TurnChanged(_)
        | sdk::ChatEvent::CurrentTurnChanged(_)
        | sdk::ChatEvent::HookEvent(_)
        | sdk::ChatEvent::WorkingDirectoryChanged { .. }
        | sdk::ChatEvent::TasksChanged
        | sdk::ChatEvent::ConfigReloaded { .. } => {}
        sdk::ChatEvent::ToolCallUpdate { name, .. } => {
            log::trace!(target: "aemeath:tui", "[tool:update] {name}");
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
        sdk::ChatEvent::SystemMessage(message) | sdk::ChatEvent::Error(message) => {
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
                    true,
                    item.default.as_deref(),
                )?;
                answers.push(reply);
            }
            let _ = reply_tx.send(answers);
        }
        sdk::ChatEvent::AgentProgress { event, .. } => {
            eprintln!("[agent] {event}");
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
    fn test_is_exit_command_accepts_exit() {
        assert!(is_exit_command("/exit"));
    }

    #[test]
    fn test_is_exit_command_accepts_quit_with_whitespace() {
        assert!(is_exit_command("  /quit  "));
    }

    #[test]
    fn test_is_exit_command_rejects_regular_text() {
        assert!(!is_exit_command("hello"));
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
