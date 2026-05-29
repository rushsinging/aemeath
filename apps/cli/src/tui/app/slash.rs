mod dialog;
mod help;
mod help_display;
mod memory;
mod reflection;
mod save;
mod suggestions;

use crate::tui::app::UiEvent;

/// 内置命令名常量（不再依赖 runtime::api）
mod cmd {
    pub const EXIT: &str = "exit";
    pub const QUIT: &str = "quit";
    pub const CLEAR: &str = "clear";
    pub const COMPACT: &str = "compact";
    pub const HELP: &str = "help";
    pub const USAGE: &str = "usage";
    pub const REFLECT: &str = "reflect";
}

impl super::App {
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
                self.layout.request_exit()
            }
            cmd if cmd == format!("/{}", cmd::CLEAR) => {
                self.chat.messages.clear();
                self.chat.clear_pending_images();
                self.handle_input_intent(
                    crate::tui::model::input::intent::InputIntent::SetAttachmentCount(0),
                );
                self.output_area.clear();
                self.reset_runtime_state().await;
                self.append_system_notice("[conversation cleared]");
            }
            cmd if cmd == format!("/{}", cmd::COMPACT) => {
                if let Some(ref ac) = self.agent_client {
                    match ac
                        .compact_messages(
                            self.chat.messages.clone(),
                            &self.chat.system_prompt_text,
                            self.chat.context_size,
                        )
                        .await
                    {
                        Ok((compacted, was_compacted)) => {
                            if was_compacted {
                                let old_len = self.chat.messages.len();
                                self.chat.messages = compacted;
                                self.append_system_notice(format!(
                                    "[compacted: {} → {} messages]",
                                    old_len,
                                    self.chat.messages.len()
                                ));
                            } else {
                                self.append_system_notice("[no compaction needed]");
                            }
                        }
                        Err(e) => {
                            self.append_error_notice(format!("compact failed: {}", e));
                        }
                    }
                } else {
                    self.append_system_notice("[compact skipped: no agent client]");
                }
            }
            cmd if cmd == format!("/{}", cmd::HELP) => self.show_slash_help(),
            cmd if cmd == format!("/{}", cmd::USAGE) => {
                let total = self.chat.total_input_tokens + self.chat.total_output_tokens;
                self.append_system_notice(format!(
                    "API calls: {} | Tokens: {} in / {} out / {} total",
                    self.chat.total_api_calls,
                    sdk::format_tokens(self.chat.total_input_tokens),
                    sdk::format_tokens(self.chat.total_output_tokens),
                    sdk::format_tokens(total)
                ));
            }
            "/save" => self.handle_save_command().await,
            "/context" => {
                if let Some(ref ac) = self.agent_client {
                    match ac
                        .estimate_context(&self.chat.messages, &self.chat.system_prompt_text)
                        .await
                    {
                        Ok(est) => {
                            self.append_system_notice(format!(
                                "Context window: ~{} / {} tokens ({:.0}%)",
                                est.estimated_tokens, est.context_size, est.usage_percentage
                            ));
                            self.append_system_notice(format!(
                                "Messages: {}",
                                self.chat.messages.len()
                            ));
                            if est.usage_percentage > 80.0 {
                                self.append_system_notice("[auto-compaction will trigger at 80%]");
                            }
                        }
                        Err(e) => {
                            self.append_error_notice(format!("context estimate failed: {}", e));
                        }
                    }
                } else {
                    // Fallback: simple message count
                    self.append_system_notice(format!("Messages: {}", self.chat.messages.len()));
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
                self.handle_memory_command(ui_tx).await;
            }
            "/paste" => {
                let result = if let Some(agent_client) = self.agent_client.clone() {
                    tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current()
                            .block_on(agent_client.read_clipboard_image())
                    })
                } else {
                    Err(sdk::SdkError::Internal(
                        "missing SDK agent client".to_string(),
                    ))
                };
                match result {
                    Ok(img) => {
                        let size = img.final_size;
                        let count = self.chat.add_pending_image(img);
                        self.handle_input_intent(
                            crate::tui::model::input::intent::InputIntent::SetAttachmentCount(
                                count,
                            ),
                        );
                        self.append_system_notice(format!(
                            "[clipboard image added ({} bytes)]",
                            size
                        ));
                    }
                    Err(e) => {
                        self.append_error_notice(format!("Failed to read clipboard: {e}"));
                    }
                }
            }
            "/images" => {
                if self.chat.pending_image_count() == 0 {
                    self.append_system_notice("No pending images.");
                } else {
                    let mut text = format!("Pending images: {}", self.chat.pending_image_count());
                    for (i, img) in self.chat.pending_images().iter().enumerate() {
                        text.push_str(&format!(
                            "\n  {}. [image {}] ({} bytes)",
                            i + 1,
                            i + 1,
                            img.final_size
                        ));
                    }
                    self.append_system_notice(text);
                }
            }
            "/clear-images" => {
                self.chat.clear_pending_images();
                self.handle_input_intent(
                    crate::tui::model::input::intent::InputIntent::SetAttachmentCount(0),
                );
                self.append_system_notice("[pending images cleared]");
            }
            // Try to execute via AgentClient (delegates to CommandRegistry in runtime)
            _ => {
                let cmd_name = cmd.trim_start_matches('/');
                let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();

                if let Some(ref ac) = self.agent_client {
                    let ctx = sdk::CommandContext {
                        cwd: self.session.cwd.to_string_lossy().to_string(),
                        session_id: self.session.session_id().to_string(),
                        models: vec![], // model list not needed for base commands
                        current_model: self.session.current_model_display.clone(),
                    };
                    match ac.execute_command(cmd_name, &args, ctx).await {
                        Ok(sdk::CommandResult::Success(msg)) => {
                            self.append_system_notice(msg);
                        }
                        Ok(sdk::CommandResult::Error(msg)) => {
                            self.append_error_notice(msg);
                        }
                        Ok(sdk::CommandResult::Action(action)) => {
                            if let Some(prompt) = self.handle_command_action(action).await {
                                return Some(prompt);
                            }
                        }
                        Ok(sdk::CommandResult::Confirm { message, .. }) => {
                            self.append_system_notice(format!("[confirm: {}]", message));
                        }
                        Err(_) => {
                            // Fallback to skill alias lookup
                            if let Some(skill) = self.find_skill_by_alias(cmd_name) {
                                let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
                                let mut content = skill.content.clone();
                                if !args.is_empty() {
                                    content = format!("{content}\n\nArguments: {args}");
                                }
                                self.append_system_notice(format!("[skill: {}]", skill.name));
                                return Some(content);
                            }
                            self.append_error_notice(format!("Unknown command: {cmd}"));
                        }
                    }
                } else if let Some(skill) = self.find_skill_by_alias(cmd_name) {
                    let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
                    let mut content = skill.content.clone();
                    if !args.is_empty() {
                        content = format!("{content}\n\nArguments: {args}");
                    }
                    self.append_system_notice(format!("[skill: {}]", skill.name));
                    return Some(content);
                } else {
                    self.append_error_notice(format!("Unknown command: {cmd}"));
                }
            }
        }
        None
    }

    /// Handle a command action returned by the AgentClient.
    /// Returns Some(prompt) if a message should be sent to the LLM.
    async fn handle_command_action(&mut self, action: sdk::CommandAction) -> Option<String> {
        match action {
            sdk::CommandAction::Exit => {
                self.layout.request_exit();
                None
            }
            sdk::CommandAction::Clear => {
                self.chat.messages.clear();
                self.chat.clear_pending_images();
                self.handle_input_intent(
                    crate::tui::model::input::intent::InputIntent::SetAttachmentCount(0),
                );
                self.output_area.clear();
                self.reset_runtime_state().await;
                self.append_system_notice("[cleared]");
                None
            }
            sdk::CommandAction::Compact => {
                if let Some(ref ac) = self.agent_client {
                    match ac
                        .compact_messages(
                            self.chat.messages.clone(),
                            &self.chat.system_prompt_text,
                            self.chat.context_size,
                        )
                        .await
                    {
                        Ok((compacted, was_compacted)) => {
                            if was_compacted {
                                self.chat.messages = compacted;
                                self.append_system_notice("[compacted]");
                            } else {
                                self.append_system_notice("[no compaction needed]");
                            }
                        }
                        Err(e) => {
                            self.append_error_notice(format!("compact failed: {}", e));
                        }
                    }
                }
                None
            }
            sdk::CommandAction::SwitchModel {
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
                if let Some(ref ac) = self.agent_client {
                    let params = sdk::ModelSwitchParams {
                        provider_name,
                        model_id,
                        model_name,
                        base_url,
                        api_key,
                        api_type,
                        max_tokens,
                        context_window,
                        reasoning,
                    };
                    match ac.switch_model(params).await {
                        Ok(result) => {
                            if result.context_window > 0 {
                                self.chat.context_size = result.context_window;
                                self.status_bar
                                    .set_context_size(result.context_window as u64);
                            }
                            self.session.current_model_display = result.display_name.clone();
                            self.status_bar.set_model(&result.display_name);
                            if let Some(ra) = result.reasoning_active {
                                self.status_bar.set_thinking(ra);
                            }
                            self.append_system_notice(format!(
                                "[switched to {}]",
                                result.display_name
                            ));
                        }
                        Err(e) => {
                            self.append_error_notice(format!("model switch failed: {}", e));
                        }
                    }
                }
                None
            }
            sdk::CommandAction::InjectMessage(prompt) => {
                self.append_system_notice("[reviewing code changes...]");
                Some(prompt)
            }
            sdk::CommandAction::RunSkill(content) => {
                self.append_system_notice("[running skill...]");
                Some(content)
            }
            sdk::CommandAction::SetThinking(desired) => {
                if let Some(ref ac) = self.agent_client {
                    match ac.set_thinking(desired).await {
                        Ok(new_state) => {
                            let label = if new_state { "ON" } else { "OFF" };
                            self.append_system_notice(format!("[thinking mode: {}]", label));
                            self.status_bar.set_thinking(new_state);
                        }
                        Err(e) => {
                            self.append_error_notice(format!("set thinking failed: {}", e));
                        }
                    }
                }
                None
            }
            sdk::CommandAction::ResumeSession(session_id) => {
                if let Some(ref ac) = self.agent_client {
                    match ac.load_session(&session_id).await {
                        Ok(snapshot) => {
                            self.resume_session_messages(
                                &session_id,
                                snapshot.messages,
                                snapshot.created_at.unwrap_or_default(),
                            );
                        }
                        Err(e) => {
                            self.append_error_notice(format!(
                                "Failed to resume session {}: {}",
                                session_id, e
                            ));
                        }
                    }
                }
                None
            }
            _ => {
                self.append_system_notice(format!("[action: {:?}]", action));
                None
            }
        }
    }
}
