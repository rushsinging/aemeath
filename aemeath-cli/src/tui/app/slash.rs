use crate::tui::completion::{generate_suggestions, SuggestionContext};
use aemeath_core::command::cmd;
use aemeath_core::command::{CommandContext, CommandRegistry, CommandResult};
use aemeath_core::memory::MemoryLayer;
use aemeath_core::reflection::{ReflectionEngine, ReflectionOutput};
use aemeath_core::session;
use aemeath_core::state::AppState;
use aemeath_llm::types::SystemBlock;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

impl super::App {
    /// Handle slash commands. Returns Some(prompt) if a message should be sent to the LLM (e.g. /review).
    pub(crate) async fn handle_slash_command(&mut self, input: &str) -> Option<String> {
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
                self.handle_reflect_command(&args).await;
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
                                            let msg_count = s.messages.len();
                                            self.session_created_at = Some(s.created_at);
                                            self.session_id = session_id.clone();
                                            self.status_bar.set_session_id(&session_id);
                                            self.messages.clear();
                                            self.pending_images.clear();
                                            let mut msgs = s.messages;
                                            aemeath_core::message::sanitize_messages(&mut msgs);
                                            let trimmed = msg_count - msgs.len();
                                            // Check for deeper integrity issues
                                            let integrity =
                                                aemeath_core::message::check_message_integrity(
                                                    &msgs,
                                                );
                                            let auto_repaired = if integrity.has_issues() {
                                                aemeath_core::message::deep_clean_messages(
                                                    &mut msgs,
                                                )
                                            } else {
                                                0
                                            };
                                            // Render history into output_area
                                            for i in 0..msgs.len() {
                                                let subsequent = if i + 1 < msgs.len() {
                                                    Some(&msgs[i + 1])
                                                } else {
                                                    None
                                                };
                                                self.render_history_message(&msgs[i], subsequent);
                                            }
                                            self.messages = msgs;
                                            self.output_area.push_system(&format!(
                                                "[resumed session {} ({} messages)]",
                                                session_id, msg_count
                                            ));
                                            if trimmed > 0 {
                                                self.output_area.push_system(&format!(
                                                    "[trimmed {} incomplete tool-call message(s)]",
                                                    trimmed
                                                ));
                                            }
                                            if auto_repaired > 0 {
                                                self.output_area.push_system(&format!(
                                                    "[repaired {} message(s): removed orphaned tool results and fixed role ordering]",
                                                    auto_repaired
                                                ));
                                            }
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

    async fn handle_reflect_command(&mut self, args: &str) {
        if !self.memory_config.reflection.enabled {
            self.output_area.push_error("Reflection 系统已禁用。");
            return;
        }

        match args.trim() {
            "" => self.run_llm_reflection().await,
            "apply" => self.apply_pending_reflection(),
            "stats" | "history" => self
                .output_area
                .push_system("Reflection stats/history 将在打磨阶段支持。"),
            other => self
                .output_area
                .push_error(&format!("未知 reflect 子命令: {other}")),
        }
    }

    async fn run_llm_reflection(&mut self) {
        let Some(client) = self.client.clone() else {
            self.output_area
                .push_error("当前没有可用的 LLM client，无法执行 Reflection。");
            return;
        };

        let store = match self.open_reflection_memory_store() {
            Ok(store) => store,
            Err(error) => {
                self.output_area.push_error(&error);
                return;
            }
        };

        let memories = match store.list(Some(MemoryLayer::Project)) {
            Ok(memories) => memories,
            Err(error) => {
                self.output_area.push_error(&error.to_string());
                return;
            }
        };
        let project_memory = ReflectionEngine::memory_summary(&memories);
        let recent_summary = ReflectionEngine::recent_messages_summary(&self.messages, 6000);
        let prompt = ReflectionEngine::build_prompt(&project_memory, &recent_summary);
        let messages = vec![aemeath_core::message::Message::user(prompt)];
        let system = vec![SystemBlock::dynamic(
            "你是 Aemeath 的 Reflection 子系统。只输出 JSON，不要输出 Markdown 或解释。"
                .to_string(),
        )];
        let cancel = CancellationToken::new();

        self.output_area.push_system("[reflection: calling LLM...]");
        let response = match client
            .stream_message_raw(&system, &messages, &[], Box::new(|_| {}), &cancel)
            .await
        {
            Ok(response) => response,
            Err(error) => {
                self.output_area
                    .push_error(&format!("Reflection LLM 调用失败: {error}"));
                return;
            }
        };

        self.total_api_calls += 1;
        self.last_input_tokens = response.usage.input_tokens as u64;
        self.total_input_tokens += response.usage.input_tokens as u64;
        self.total_output_tokens += response.usage.output_tokens as u64;
        self.status_bar.set_tokens(
            self.total_input_tokens,
            self.total_output_tokens,
            self.last_input_tokens,
        );

        let text = response.assistant_message.text_content();
        let output = match ReflectionEngine::parse_output(&text) {
            Ok(output) => output,
            Err(error) => {
                self.output_area
                    .push_error(&format!("Reflection 输出解析失败: {error}"));
                return;
            }
        };

        let formatted = ReflectionEngine::format_output(&output);
        self.output_area.push_system(&formatted);

        if self.memory_config.reflection.auto_apply_suggestions {
            self.apply_reflection_output(output);
        } else {
            let suggestion_count = output.suggested_memories.len();
            let outdated_count = output.outdated_memories.len();
            self.pending_reflection = Some(output);
            if suggestion_count > 0 || outdated_count > 0 {
                self.output_area.push_system(&format!(
                    "[reflection: {suggestion_count} 条建议记忆、{outdated_count} 条过时标记待应用；运行 /reflect apply]"
                ));
            }
        }
    }

    fn apply_pending_reflection(&mut self) {
        let Some(output) = self.pending_reflection.clone() else {
            self.output_area
                .push_system("没有待应用的 Reflection 建议。");
            return;
        };

        if self.apply_reflection_output(output) {
            self.pending_reflection = None;
        }
    }

    fn apply_reflection_output(&mut self, output: ReflectionOutput) -> bool {
        let mut store = match self.open_reflection_memory_store() {
            Ok(store) => store,
            Err(error) => {
                self.output_area.push_error(&error);
                return false;
            }
        };

        match ReflectionEngine::apply_output(&output, &mut store) {
            Ok(applied) => {
                self.output_area.push_system(&format!(
                    "[reflection applied: 新增/合并 {} 条记忆，标记 {} 条过时记忆]",
                    applied.suggestions_added, applied.outdated_marked
                ));
                true
            }
            Err(error) => {
                self.output_area
                    .push_error(&format!("应用 Reflection 建议失败: {error}"));
                false
            }
        }
    }

    fn open_reflection_memory_store(&self) -> Result<aemeath_core::memory::MemoryStore, String> {
        let base_dir = aemeath_core::memory::memory_base_dir();
        let project_hash = aemeath_core::memory::project_hash_from_path(&self.cwd);
        aemeath_core::memory::MemoryStore::new(
            base_dir,
            project_hash,
            self.memory_config.max_entries,
            self.memory_config.similarity_threshold,
        )
        .map_err(|error| error.to_string())
    }

    /// Update suggestions based on current input
    pub(crate) fn update_suggestions(&mut self) {
        let input = self.input_area.get_text();
        let (_row, col) = self.input_area.cursor_position();
        // Convert column (char count) to byte offset
        let cursor_offset = input
            .char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(input.len());

        let models: Vec<(String, String)> = self
            .models_config
            .list_models()
            .into_iter()
            .map(|(p, m)| (p, if m.name.is_empty() { m.id } else { m.name }))
            .collect();

        let skills: Vec<(String, String, Vec<String>)> = self
            .skills
            .values()
            .map(|s| (s.name.clone(), s.description.clone(), s.aliases.clone()))
            .collect();

        // Build command list from CommandRegistry (single source of truth)
        let registry = CommandRegistry::global();
        let commands: Vec<(String, String, Vec<String>)> = registry
            .list()
            .into_iter()
            .map(|cmd| {
                (
                    cmd.name.clone(),
                    cmd.description.clone(),
                    cmd.aliases.clone(),
                )
            })
            .collect();

        let ctx = SuggestionContext {
            input,
            cursor_offset,
            cwd: self.cwd.clone(),
            models,
            skills,
            commands,
            sessions: self.cached_sessions.clone(),
        };

        let suggestions = generate_suggestions(&ctx);
        self.input_area.set_suggestions(suggestions);
    }
}
