mod help;
mod memory;
mod reflection;
mod suggestions;

use crate::tui::core::UiEvent;
use ::runtime::api::command::cmd;
use ::runtime::api::command::{CommandContext, CommandRegistry, CommandResult};
use ::runtime::api::state::AppState;
use help::SLASH_HELP_LINES;
use std::sync::Arc;
fn message_to_sdk(message: ::runtime::api::core::message::Message) -> sdk::ChatMessage {
    sdk::ChatMessage {
        role: match message.role {
            ::runtime::api::core::message::Role::User => "user".to_string(),
            ::runtime::api::core::message::Role::Assistant => "assistant".to_string(),
        },
        content: serde_json::to_value(&message.content).unwrap_or(serde_json::Value::Null),
    }
}

fn message_from_sdk(message: &sdk::ChatMessage) -> ::runtime::api::core::message::Message {
    let role = match message.role.as_str() {
        "assistant" => ::runtime::api::core::message::Role::Assistant,
        _ => ::runtime::api::core::message::Role::User,
    };
    let content = serde_json::from_value(message.content.clone()).unwrap_or_else(|_| {
        vec![::runtime::api::core::message::ContentBlock::Text {
            text: String::new(),
        }]
    });
    ::runtime::api::core::message::Message { role, content }
}

impl super::App {
    fn open_model_selection_dialog(&mut self) -> Option<String> {
        let models = self.cmd_exec.models_config.list_models();
        if models.is_empty() {
            self.output_area
                .push_system("No models configured. Add models to ~/.aemeath/config.json");
            return None;
        }
        let current = self.session.current_model_display.clone();
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
        self.layout.active_dialog = Some(crate::tui::display::dialog::Dialog::select(
            "Select Model",
            options,
        ));
        self.layout.dialog_model_keys = keys;
        None
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

        if cmd == "/model" && !has_args {
            return self.open_model_selection_dialog();
        }

        match cmd {
            cmd if cmd == format!("/{}", cmd::EXIT) || cmd == format!("/{}", cmd::QUIT) => {
                self.layout.should_exit = true
            }
            cmd if cmd == format!("/{}", cmd::CLEAR) => {
                self.chat.messages.clear();
                self.chat.pending_images.clear();
                self.input_area.set_pending_images(0);
                self.output_area.clear();
                self.reset_runtime_state().await;
                self.output_area.push_system("[conversation cleared]");
            }
            cmd if cmd == format!("/{}", cmd::COMPACT) => {
                use ::runtime::api::compact;
                let mut runtime_messages: Vec<_> =
                    self.chat.messages.iter().map(message_from_sdk).collect();
                let (compacted, was_compacted) = compact::compact_messages(
                    &mut runtime_messages,
                    &self.chat.system_prompt_text,
                    self.chat.context_size,
                );
                if was_compacted {
                    let old_len = self.chat.messages.len();
                    self.chat.messages = compacted.into_iter().map(message_to_sdk).collect();
                    self.output_area.push_system(&format!(
                        "[compacted: {} → {} messages]",
                        old_len,
                        self.chat.messages.len()
                    ));
                } else {
                    self.output_area.push_system("[no compaction needed]");
                }
            }
            cmd if cmd == format!("/{}", cmd::HELP) => self.show_slash_help(),
            cmd if cmd == format!("/{}", cmd::USAGE) => {
                use ::runtime::api::cost::format_tokens;
                let total = self.chat.total_input_tokens + self.chat.total_output_tokens;
                self.output_area.push_system(&format!(
                    "API calls: {} | Tokens: {} in / {} out / {} total",
                    self.chat.total_api_calls,
                    format_tokens(self.chat.total_input_tokens),
                    format_tokens(self.chat.total_output_tokens),
                    format_tokens(total)
                ));
            }
            "/save" => self.handle_save_command().await,
            "/context" => {
                use ::runtime::api::compact;
                let runtime_messages: Vec<_> =
                    self.chat.messages.iter().map(message_from_sdk).collect();
                let estimated = compact::estimate_messages_tokens(&runtime_messages)
                    + compact::estimate_tokens(&self.chat.system_prompt_text);
                let pct = estimated * 100 / self.chat.context_size.max(1);
                self.output_area.push_system(&format!(
                    "Context window: ~{} / {} tokens ({}%)",
                    estimated, self.chat.context_size, pct
                ));
                self.output_area
                    .push_system(&format!("Messages: {}", self.chat.messages.len()));
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
                    tokio::runtime::Handle::current()
                        .block_on(::runtime::api::image::read_clipboard_image())
                });
                match result {
                    Ok(img) => {
                        let size = img.final_size;
                        self.chat.pending_images.push(img);
                        self.input_area
                            .set_pending_images(self.chat.pending_images.len());
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
                if self.chat.pending_images.is_empty() {
                    self.output_area.push_system("No pending images.");
                } else {
                    self.output_area.push_system(&format!(
                        "Pending images: {}",
                        self.chat.pending_images.len()
                    ));
                    for (i, img) in self.chat.pending_images.iter().enumerate() {
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
                self.chat.pending_images.clear();
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
                    let config = ::runtime::api::core::config::Config::default();
                    let mut ctx = CommandContext::new(
                        Arc::new(state),
                        config,
                        self.session.cwd.to_string_lossy().to_string(),
                        self.session.session_id.clone(),
                    );
                    ctx.models_config = self.cmd_exec.models_config.clone();
                    ctx.current_model = self.session.current_model_display.clone();

                    match cmd_obj.execute(&args, &mut ctx).await {
                        CommandResult::Success(msg) => self.output_area.push_system(&msg),
                        CommandResult::Error(msg) => self.output_area.push_error(&msg),
                        CommandResult::Action(action) => {
                            match action {
                                ::runtime::api::command::CommandAction::Exit => {
                                    self.layout.should_exit = true
                                }
                                ::runtime::api::command::CommandAction::Clear => {
                                    self.chat.messages.clear();
                                    self.chat.pending_images.clear();
                                    self.input_area.set_pending_images(0);
                                    self.output_area.clear();
                                    self.reset_runtime_state().await;
                                    self.output_area.push_system("[cleared]");
                                }
                                ::runtime::api::command::CommandAction::Compact => {
                                    use ::runtime::api::compact;
                                    let mut runtime_messages: Vec<_> =
                                        self.chat.messages.iter().map(message_from_sdk).collect();
                                    let (compacted, was_compacted) = compact::compact_messages(
                                        &mut runtime_messages,
                                        &self.chat.system_prompt_text,
                                        self.chat.context_size,
                                    );
                                    if was_compacted {
                                        self.chat.messages =
                                            compacted.into_iter().map(message_to_sdk).collect();
                                        self.output_area.push_system("[compacted]");
                                    } else {
                                        self.output_area.push_system("[no compaction needed]");
                                    }
                                }
                                ::runtime::api::command::CommandAction::SwitchModel {
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
                                    let api_type =
                                        ::runtime::api::provider::ApiDriverKind::from_str(
                                            api_type.as_str(),
                                        )
                                        .unwrap_or(::runtime::api::provider::ApiDriverKind::OpenAI);

                                    // Build OpenAI chat provider config for Chat Completions drivers.
                                    let openai_config = if matches!(
                                        api_type,
                                        ::runtime::api::provider::ApiDriverKind::Anthropic
                                    ) {
                                        None
                                    } else {
                                        Some(
                                            ::runtime::api::provider::client::OpenAIProviderConfig::from_api_driver(
                                                api_type,
                                                &provider_name,
                                            ),
                                        )
                                    };

                                    // Model config takes priority; keep current reasoning only when unset.
                                    let reasoning = reasoning
                                        .or_else(|| {
                                            self.cmd_exec.client.as_ref().map(|c| c.is_reasoning())
                                        })
                                        .unwrap_or(true);
                                    let reasoning_config = Some(
                                        ::runtime::api::provider::providers::openai_compatible::ReasoningConfig::Bool(
                                            reasoning,
                                        ),
                                    );
                                    let new_client =
                                        ::runtime::api::provider::client::LlmClient::from_config(
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

                                    self.cmd_exec.client = Some(Arc::new(new_client));
                                    if context_window > 0 {
                                        self.chat.context_size = context_window;
                                        self.status_bar.set_context_size(context_window as u64);
                                    }
                                    let display_name = if model_name.is_empty() {
                                        &model_id
                                    } else {
                                        &model_name
                                    };
                                    let display = format!("{}/{}", provider_name, display_name);
                                    self.session.current_model_display = display.clone();
                                    self.status_bar.set_model(&display);
                                    self.status_bar.set_thinking(reasoning);
                                    self.output_area
                                        .push_system(&format!("[switched to {}]", display));
                                }
                                ::runtime::api::command::CommandAction::InjectMessage(prompt) => {
                                    self.output_area.push_system("[reviewing code changes...]");
                                    return Some(prompt);
                                }
                                ::runtime::api::command::CommandAction::RunSkill(content) => {
                                    self.output_area.push_system("[running skill...]");
                                    return Some(content);
                                }
                                ::runtime::api::command::CommandAction::SetThinking(desired) => {
                                    let current = self
                                        .cmd_exec
                                        .client
                                        .as_ref()
                                        .map(|c| c.is_reasoning())
                                        .unwrap_or(true);
                                    let new_state = desired.unwrap_or(!current);
                                    if let Some(ref client) = self.cmd_exec.client {
                                        client.set_reasoning(new_state);
                                    }
                                    let label = if new_state { "ON" } else { "OFF" };
                                    self.output_area
                                        .push_system(&format!("[thinking mode: {}]", label));
                                    self.status_bar.set_thinking(new_state);
                                }
                                ::runtime::api::command::CommandAction::ResumeSession(
                                    session_id,
                                ) => {
                                    match ::runtime::api::session::load_session(&session_id).await {
                                        Ok(s) => {
                                            self.resume_session_messages(
                                                &session_id,
                                                s.messages
                                                    .into_iter()
                                                    .map(message_to_sdk)
                                                    .collect(),
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
    async fn handle_save_command(&mut self) {
        let result = if let Some(agent_client) = &self.agent_client {
            if let Err(e) = agent_client
                .sync_current_messages(self.chat.messages.clone())
                .await
            {
                log::warn!("failed to sync session messages: {e}");
            }
            agent_client.save_current_session().await
        } else {
            Err(sdk::SdkError::Internal(
                "SDK agent client is unavailable".to_string(),
            ))
        };
        match result {
            Ok(()) => self
                .output_area
                .push_system(&format!("[session saved: {}]", self.session.session_id)),
            Err(e) => self
                .output_area
                .push_error(&format!("Failed to save session: {e}")),
        }
    }

    fn show_slash_help(&mut self) {
        for line in SLASH_HELP_LINES {
            self.output_area.push_system(line);
        }
    }
}
