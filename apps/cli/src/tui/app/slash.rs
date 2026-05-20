mod reflection;
mod suggestions;

use crate::tui::app::UiEvent;
use aemeath_core::command::cmd;
use aemeath_core::command::{CommandContext, CommandRegistry, CommandResult};
use aemeath_core::session;
use aemeath_core::state::AppState;
use std::sync::Arc;

impl super::App {
    fn show_memory_reminders(&mut self) {
        let lines = self
            .session_reminders
            .lock()
            .ok()
            .map(|reminders| {
                reminders
                    .list()
                    .iter()
                    .map(|reminder| {
                        let marker = if reminder.done { "✓" } else { "□" };
                        format!("{marker} {} {}", reminder.id, reminder.content)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if lines.is_empty() {
            self.output_area.push_system("当前没有 session reminder。");
            return;
        }

        self.output_area.push_system("Session Reminders:");
        for line in lines {
            self.output_area.push_system(&line);
        }
    }

    /// Handle slash commands with an optional UI event sender for background commands.
    /// Returns Some(prompt) if a message should be sent to the LLM (e.g. /review).
    pub(crate) async fn handle_slash_command_with_events(
        &mut self,
        input: &str,
        ui_tx: Option<tokio::sync::mpsc::Sender<UiEvent>>,
    ) -> Option<String> {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = *parts.first().unwrap_or(&"");
        let has_args = parts.len() > 1;

        // /model with no args → open selection dialog
        if cmd == "/model" && !has_args {
            let models = self.models_config.list_models();
            if models.is_empty() {
                self.output_area
                    .push_system("No models configured. Add models to ~/.aemeath/config.json");
                return None;
            }
            let current = self.current_model_display.clone();
            let mut options = Vec::new();
            let mut keys = Vec::new();
            for (provider_name, model) in &models {
                let display_name = if model.name.is_empty() {
                    &model.id
                } else {
                    &model.name
                };
                let key = format!("{}/{}", provider_name, display_name);
                let marker = if key == current { " ←" } else { "" };
                options.push(format!(
                    "{}/{} ctx:{}k max:{}k{}",
                    provider_name,
                    display_name,
                    model.context_window / 1000,
                    model.max_tokens / 1000,
                    marker,
                ));
                keys.push(key);
            }
            self.active_dialog = Some(crate::tui::dialog::Dialog::select("Select Model", options));
            self.dialog_model_keys = keys;
            return None;
        }

        match cmd {
            cmd if cmd == format!("/{}", cmd::EXIT) || cmd == format!("/{}", cmd::QUIT) => {
                self.should_exit = true
            }
            cmd if cmd == format!("/{}", cmd::CLEAR) => {
                self.messages.clear();
                self.pending_images.clear();
                self.input_area.set_pending_images(0);
                self.output_area.clear();
                self.reset_runtime_state().await;
                self.output_area.push_system("[conversation cleared]");
            }
            cmd if cmd == format!("/{}", cmd::COMPACT) => {
                use aemeath_core::compact;
                let (compacted, was_compacted) = compact::compact_messages(
                    &mut self.messages,
                    &self.system_prompt_text,
                    self.context_size,
                );
                if was_compacted {
                    let old_len = self.messages.len();
                    self.messages = compacted;
                    self.output_area.push_system(&format!(
                        "[compacted: {} → {} messages]",
                        old_len,
                        self.messages.len()
                    ));
                } else {
                    self.output_area.push_system("[no compaction needed]");
                }
            }
            cmd if cmd == format!("/{}", cmd::HELP) => {
                self.output_area.push_system("Commands:");
                self.output_area
                    .push_system("  /help  /exit  /clear  /compact  /usage  /save  /session");
                self.output_area
                    .push_system("  /paste  /images  /clear-images  /context  /review  /think");
                self.output_area.push_system("");
                self.output_area.push_system("Scrolling:");
                self.output_area
                    .push_system("  Mouse wheel     - scroll 3 lines");
                self.output_area
                    .push_system("  PageUp/PageDown - scroll 10 lines");
                self.output_area
                    .push_system("  Shift+Up/Down   - scroll 1 line");
                self.output_area
                    .push_system("  Shift+Home      - scroll to top");
                self.output_area
                    .push_system("  Shift+End       - scroll to bottom");
                self.output_area.push_system("");
                self.output_area.push_system("Input:");
                self.output_area
                    .push_system("  Enter           - send message");
                self.output_area.push_system("  Alt+Enter       - new line");
                self.output_area
                    .push_system("  Tab             - accept suggestion");
                self.output_area
                    .push_system("  Ctrl+C          - interrupt / exit");
                self.output_area
                    .push_system("  Ctrl+V          - paste image from clipboard");
            }
            cmd if cmd == format!("/{}", cmd::USAGE) => {
                use aemeath_core::cost::format_tokens;
                let total = self.total_input_tokens + self.total_output_tokens;
                self.output_area.push_system(&format!(
                    "API calls: {} | Tokens: {} in / {} out / {} total",
                    self.total_api_calls,
                    format_tokens(self.total_input_tokens),
                    format_tokens(self.total_output_tokens),
                    format_tokens(total)
                ));
            }
            "/save" => {
                let s = self.build_session(self.messages.clone()).await;
                match session::save_session(&s).await {
                    Ok(()) => self
                        .output_area
                        .push_system(&format!("[session saved: {}]", self.session_id)),
                    Err(e) => self
                        .output_area
                        .push_error(&format!("Failed to save session: {e}")),
                }
            }
            "/context" => {
                use aemeath_core::compact;
                let estimated = compact::estimate_messages_tokens(&self.messages)
                    + compact::estimate_tokens(&self.system_prompt_text);
                let pct = estimated * 100 / self.context_size.max(1);
                self.output_area.push_system(&format!(
                    "Context window: ~{} / {} tokens ({}%)",
                    estimated, self.context_size, pct
                ));
                self.output_area
                    .push_system(&format!("Messages: {}", self.messages.len()));
                if pct > 80 {
                    self.output_area
                        .push_system("[auto-compaction will trigger at 80%]");
                }
            }
            cmd if cmd == format!("/{}", cmd::REFLECT) => {
                let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
                self.handle_reflect_command_with_events(&args, ui_tx).await;
            }
            "/memory" | "/mem"
                if matches!(
                    parts.get(1).copied(),
                    Some("remind" | "reminder" | "reminders")
                ) =>
            {
                self.show_memory_reminders();
            }
            "/paste" => {
                // block_in_place allows async call from non-async context in tokio runtime
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(crate::image::read_clipboard_image())
                });
                match result {
                    Ok(img) => {
                        let size = img.final_size;
                        self.pending_images.push(img);
                        self.input_area
                            .set_pending_images(self.pending_images.len());
                        self.output_area
                            .push_system(&format!("[clipboard image added ({} bytes)]", size));
                    }
                    Err(e) => {
                        self.output_area
                            .push_error(&format!("Failed to read clipboard: {e}"));
                    }
                }
            }
            "/images" => {
                if self.pending_images.is_empty() {
                    self.output_area.push_system("No pending images.");
                } else {
                    self.output_area
                        .push_system(&format!("Pending images: {}", self.pending_images.len()));
                    for (i, img) in self.pending_images.iter().enumerate() {
                        self.output_area.push_system(&format!(
                            "  {}. [image {}] ({} bytes)",
                            i + 1,
                            i + 1,
                            img.final_size
                        ));
                    }
                }
            }
            "/clear-images" => {
                self.pending_images.clear();
                self.input_area.set_pending_images(0);
                self.output_area.push_system("[pending images cleared]");
            }
            // Try to execute via CommandRegistry
            _ => {
                let cmd_name = cmd.trim_start_matches('/');
                let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();

                // Try to find command in registry
                let registry = CommandRegistry::global();
                if let Some(cmd_obj) = registry.find(cmd_name) {
                    // Create minimal context for command execution
                    let state = AppState::default();
                    let config = aemeath_core::config::Config::default();
                    let mut ctx = CommandContext::new(
                        Arc::new(state),
                        config,
                        self.cwd.to_string_lossy().to_string(),
                        self.session_id.clone(),
                    );
                    ctx.models_config = self.models_config.clone();
                    ctx.current_model = self.current_model_display.clone();

                    match cmd_obj.execute(&args, &mut ctx).await {
                        CommandResult::Success(msg) => self.output_area.push_system(&msg),
                        CommandResult::Error(msg) => self.output_area.push_error(&msg),
                        CommandResult::Action(action) => {
                            match action {
                                aemeath_core::command::CommandAction::Exit => {
                                    self.should_exit = true
                                }
                                aemeath_core::command::CommandAction::Clear => {
                                    self.messages.clear();
                                    self.pending_images.clear();
                                    self.input_area.set_pending_images(0);
                                    self.output_area.clear();
                                    self.reset_runtime_state().await;
                                    self.output_area.push_system("[cleared]");
                                }
                                aemeath_core::command::CommandAction::Compact => {
                                    use aemeath_core::compact;
                                    let (compacted, was_compacted) = compact::compact_messages(
                                        &mut self.messages,
                                        &self.system_prompt_text,
                                        self.context_size,
                                    );
                                    if was_compacted {
                                        self.messages = compacted;
                                        self.output_area.push_system("[compacted]");
                                    } else {
                                        self.output_area.push_system("[no compaction needed]");
                                    }
                                }
                                aemeath_core::command::CommandAction::SwitchModel {
                                    provider_name,
                                    model_id,
                                    model_name,
                                    base_url,
                                    api_key,
                                    api_type,
                                    max_tokens,
                                    context_window,
                                    reasoning,
                                } => {
                                    // Determine API driver from config's api field.
                                    let api_type = aemeath_core::provider::ApiDriverKind::from_str(
                                        api_type.as_str(),
                                    )
                                    .unwrap_or(aemeath_core::provider::ApiDriverKind::OpenAI);

                                    // Build OpenAI chat provider config for Chat Completions drivers.
                                    let openai_config = if matches!(
                                        api_type,
                                        aemeath_core::provider::ApiDriverKind::Anthropic
                                    ) {
                                        None
                                    } else {
                                        Some(
                                            aemeath_llm::client::OpenAIProviderConfig::from_api_driver(
                                                api_type,
                                                &provider_name,
                                            ),
                                        )
                                    };

                                    // Model config takes priority; keep current reasoning only when unset.
                                    let reasoning = reasoning
                                        .or_else(|| self.client.as_ref().map(|c| c.is_reasoning()))
                                        .unwrap_or(true);
                                    let reasoning_config = Some(
                                        aemeath_llm::providers::openai_compatible::ReasoningConfig::Bool(
                                            reasoning,
                                        ),
                                    );
                                    let new_client = aemeath_llm::client::LlmClient::from_config(
                                        api_type,
                                        api_key,
                                        Some(base_url),
                                        model_id.clone(),
                                        max_tokens,
                                        0,
                                        reasoning,
                                        reasoning_config,
                                        openai_config,
                                    );

                                    self.client = Some(Arc::new(new_client));
                                    if context_window > 0 {
                                        self.context_size = context_window;
                                        self.status_bar.set_context_size(context_window as u64);
                                    }
                                    let display_name = if model_name.is_empty() {
                                        &model_id
                                    } else {
                                        &model_name
                                    };
                                    let display = format!("{}/{}", provider_name, display_name);
                                    self.current_model_display = display.clone();
                                    self.status_bar.set_model(&display);
                                    self.status_bar.set_thinking(reasoning);
                                    self.output_area
                                        .push_system(&format!("[switched to {}]", display));
                                }
                                aemeath_core::command::CommandAction::InjectMessage(prompt) => {
                                    self.output_area.push_system("[reviewing code changes...]");
                                    return Some(prompt);
                                }
                                aemeath_core::command::CommandAction::RunSkill(content) => {
                                    self.output_area.push_system("[running skill...]");
                                    return Some(content);
                                }
                                aemeath_core::command::CommandAction::SetThinking(desired) => {
                                    let current = self
                                        .client
                                        .as_ref()
                                        .map(|c| c.is_reasoning())
                                        .unwrap_or(true);
                                    let new_state = desired.unwrap_or(!current);
                                    if let Some(ref client) = self.client {
                                        client.set_reasoning(new_state);
                                    }
                                    let label = if new_state { "ON" } else { "OFF" };
                                    self.output_area
                                        .push_system(&format!("[thinking mode: {}]", label));
                                    self.status_bar.set_thinking(new_state);
                                }
                                aemeath_core::command::CommandAction::ResumeSession(session_id) => {
                                    match aemeath_core::session::load_session(&session_id).await {
                                        Ok(s) => {
                                            self.resume_session_messages(
                                                &session_id,
                                                s.messages,
                                                s.created_at,
                                            );
                                        }
                                        Err(e) => {
                                            self.output_area.push_error(&format!(
                                                "Failed to resume session {}: {}",
                                                session_id, e
                                            ));
                                        }
                                    }
                                }
                                _ => self
                                    .output_area
                                    .push_system(&format!("[action: {:?}]", action)),
                            }
                        }
                        CommandResult::Confirm { message, .. } => {
                            self.output_area
                                .push_system(&format!("[confirm: {}]", message));
                        }
                    }
                } else if let Some(skill) = self.find_skill_by_alias(cmd_name) {
                    // Match skill alias — inject skill content as user message
                    let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
                    let mut content = skill.content.clone();
                    if !args.is_empty() {
                        content = format!("{content}\n\nArguments: {args}");
                    }
                    self.output_area
                        .push_system(&format!("[skill: {}]", skill.name));
                    return Some(content);
                } else {
                    self.output_area
                        .push_error(&format!("Unknown command: {cmd}"));
                }
            }
        }
        None
    }
}
