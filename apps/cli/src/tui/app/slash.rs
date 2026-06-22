mod dialog;
mod help;
mod help_display;
mod reflection;
mod suggestions;

use crate::tui::app::UiEvent;
use crate::tui::effect::effect::Effect;
use crate::tui::model::runtime::intent::RuntimeIntent;

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
                self.clear_conversation().await;
                self.append_system_notice("[conversation cleared]");
            }
            cmd if cmd == format!("/{}", cmd::COMPACT) => {
                if let Some(ref ac) = self.agent_client {
                    // 设置 spinner phase 为 Compacting
                    self.model.runtime.apply(RuntimeIntent::SetSpinnerPhase(
                        crate::tui::model::runtime::spinner::SpinnerPhase::Compacting,
                    ));
                    match ac
                        .compact_messages(
                            self.chat.messages.clone(),
                            &self.chat.system_prompt_text,
                            self.chat.context_size,
                        )
                        .await
                    {
                        Ok((compacted, was_compacted)) => {
                            // 停止 spinner
                            self.model.runtime.apply(RuntimeIntent::StopSpinner);
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
                            // 停止 spinner
                            self.model.runtime.apply(RuntimeIntent::StopSpinner);
                            self.append_error_notice(format!("compact failed: {}", e));
                        }
                    }
                } else {
                    self.append_system_notice("[compact skipped: no agent client]");
                }
            }
            cmd if cmd == format!("/{}", cmd::HELP) => self.show_slash_help(),
            cmd if cmd == format!("/{}", cmd::USAGE) => {
                let usage = &self.model.runtime.usage;
                let total = usage.input_tokens + usage.output_tokens;
                self.append_system_notice(format!(
                    "API calls: {} | Tokens: {} in / {} out / {} total",
                    usage.api_calls,
                    sdk::format_tokens(usage.input_tokens),
                    sdk::format_tokens(usage.output_tokens),
                    sdk::format_tokens(total)
                ));
            }
            "/save" => {
                if let Some(tx) = ui_tx.clone() {
                    self.execute_effect(Effect::SaveSession { notify: true }, &tx)
                        .await;
                }
            }
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
                let effects = self.handle_reflect_command(&args);
                if let Some(tx) = ui_tx.clone() {
                    for effect in effects {
                        self.execute_effect(effect, &tx).await;
                    }
                }
            }
            "/memory" | "/mem"
                if matches!(
                    parts.get(1).copied(),
                    Some("remind" | "reminder" | "reminders")
                ) =>
            {
                if let Some(tx) = ui_tx.clone() {
                    self.execute_effect(Effect::FetchMemoryList, &tx).await;
                }
            }
            "/update" => {
                if let Some(tx) = ui_tx.clone() {
                    self.execute_effect(Effect::RunSelfUpdate, &tx).await;
                }
            }
            "/paste" => {
                if let Some(tx) = ui_tx.clone() {
                    self.execute_effect(Effect::ReadClipboardImage, &tx).await;
                }
            }
            "/images" => {
                let spans = &self.model.input.document.image_spans;
                if spans.is_empty() {
                    self.append_system_notice("No pending images.");
                } else {
                    let mut text = format!("Pending images: {}", spans.len());
                    for span in spans.iter() {
                        text.push_str(&format!(
                            "\n  {}. [Image #{}] ({} bytes)",
                            span.index, span.index, span.image.final_size
                        ));
                    }
                    self.append_system_notice(text);
                }
            }
            "/clear-images" => {
                self.model.input.document.remove_all_images();
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

    /// #391 方案 B：清空会话。
    ///
    /// 即时清 TUI 状态（messages/output/输入框），同时经 `ChatInputEvent::Reset`
    /// 让 runtime idle gate 统一清空 runtime messages。busy 时先 `cancel()` 使 loop
    /// 回 idle 处理 Reset；loop 未运行时 fallback 到 `reset_runtime_state`。
    ///
    /// `SessionReset` 事件回来后经 `Effect::ResetRuntimeState` 再做完整清理
    ///（sync agent_client + clear_tasks）；loop 不再被 drop，保持存活。
    async fn clear_conversation(&mut self) {
        self.chat.messages.clear();
        self.handle_input_intent(crate::tui::model::input::intent::InputIntent::Clear);
        self.output_area.clear();
        if self.chat.input_event_tx.is_some() {
            // loop 运行中：发 Reset，由 runtime gate 统一清空。
            if let Some(ref ac) = self.agent_client {
                ac.cancel();
            }
            self.chat.push_input_event(sdk::ChatInputEvent::Reset);
        } else {
            // loop 未运行（如启动前）→ 直接本地清理。
            self.reset_runtime_state().await;
        }
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
                self.clear_conversation().await;
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
                driver,
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
                        driver,
                        max_tokens,
                        context_window,
                        reasoning,
                    };
                    match ac.switch_model(params).await {
                        Ok(result) => {
                            if result.context_window > 0 {
                                self.chat.context_size = result.context_window;
                                self.model.runtime.apply(
                                    crate::tui::model::runtime::intent::RuntimeIntent::SetContextSize(
                                        result.context_window as u64,
                                    ),
                                );
                            }
                            self.session.current_model_display = result.display_name.clone();
                            // model 真相归 RuntimeModel，StatusBar 渲染时直接消费 StatusViewModel。
                            self.model.runtime.apply(
                                crate::tui::model::runtime::intent::RuntimeIntent::SetProviderModel {
                                    provider: self.model.runtime.provider.clone(),
                                    model_id: Some(result.display_name.clone()),
                                },
                            );
                            if let Some(ra) = result.reasoning_active {
                                self.model.runtime.apply(RuntimeIntent::SetThinking(ra));
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
                            self.model
                                .runtime
                                .apply(RuntimeIntent::SetThinking(new_state));
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
