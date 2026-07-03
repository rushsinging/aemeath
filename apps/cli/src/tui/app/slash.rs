mod dialog;
mod help;
mod help_display;
mod reflection;
mod suggestions;

use crate::tui::app::UiEvent;
use crate::tui::effect::effect::Effect;

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
                // #497 子 issue 0：走 runtime 事件流（ChatInputEvent::Compact →
                // manual_compact），不再直接调 compact_messages().await。
                // spinner / 进度 Gauge / 结果回显全部由 runtime 的
                // PreCompact → CompactProgress → PostCompact → SystemMessage 事件驱动。
                if self.chat.input_event_tx.is_some() {
                    self.chat.push_input_event(sdk::ChatInputEvent::Compact);
                } else if let Some(ref ac) = self.agent_client {
                    // loop 未运行（如启动前）→ fallback 到直接调用
                    self.model.conversation.spinner.phase =
                        Some(crate::tui::model::conversation::spinner::SpinnerPhase::Compacting);
                    match ac
                        .compact_messages(
                            self.chat.messages.clone(),
                            &self.chat.system_prompt_text,
                            self.chat.context_size,
                        )
                        .await
                    {
                        Ok((compacted, was_compacted)) => {
                            self.model.conversation.spinner.phase = None;
                            self.model.conversation.spinner.running_tool_count = 0;
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
                            self.model.conversation.spinner.phase = None;
                            self.model.conversation.spinner.running_tool_count = 0;
                            self.append_error_notice(format!("compact failed: {}", e));
                        }
                    }
                } else {
                    self.append_system_notice("[compact skipped: no agent client]");
                }
            }
            cmd if cmd == format!("/{}", cmd::HELP) => self.show_slash_help(),
            cmd if cmd == format!("/{}", cmd::USAGE) => {
                let usage = &self.model.conversation.usage;
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
                if self.chat.input_event_tx.is_some() {
                    // #497 子任务 3：走 runtime 事件流
                    //（ChatInputEvent::EstimateContext → idle 分支 → ContextEstimated 事件），
                    // 不再直接调 estimate_context().await。显示由 UiEvent::ContextEstimated 驱动。
                    self.chat
                        .push_input_event(sdk::ChatInputEvent::EstimateContext);
                } else if let Some(ref ac) = self.agent_client {
                    // loop 未运行（如启动前）→ fallback 到直接调用
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
            cmd if cmd == "/version" => {
                let info = format!(
                    "aemeath v{}

Build info:
  Rust version: stable
  Target: {}",
                    env!("CARGO_PKG_VERSION"),
                    std::env::consts::ARCH
                );
                self.append_system_notice(&info);
            }
            cmd if cmd == "/doctor" => {
                let api_key =
                    std::env::var("ANTHROPIC_API_KEY").or_else(|_| std::env::var("CLAUDE_API_KEY"));
                let home = dirs::home_dir();
                let info = format!(
                    "🔧 Doctor\n\n                     \
                     API Key: {}\n                     \
                     Home dir: {}\n                     \
                     Architecture: {}",
                    if api_key.is_ok() {
                        "✅ set"
                    } else {
                        "❌ not set"
                    },
                    home.map(|p| p.display().to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    std::env::consts::ARCH,
                );
                self.append_system_notice(&info);
            }
            cmd if cmd == "/rewind" => {
                // /rewind <N> → 触发 compact（保留 N 条消息的语义通过 compact 实现）
                if self.chat.input_event_tx.is_some() {
                    self.chat.push_input_event(sdk::ChatInputEvent::Compact);
                }
            }
            cmd if cmd == "/cost" => {
                let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
                if self.chat.input_event_tx.is_some() {
                    self.chat
                        .push_input_event(sdk::ChatInputEvent::QueryCost { args });
                }
            }
            cmd if cmd == "/status" => {
                if self.chat.input_event_tx.is_some() {
                    self.chat.push_input_event(sdk::ChatInputEvent::QueryStatus);
                }
            }
            cmd if cmd == "/config" => {
                let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
                if self.chat.input_event_tx.is_some() {
                    self.chat
                        .push_input_event(sdk::ChatInputEvent::QueryConfig { args });
                }
            }
            cmd if cmd == "/stats" => {
                let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
                if self.chat.input_event_tx.is_some() {
                    self.chat
                        .push_input_event(sdk::ChatInputEvent::QueryStats { args });
                }
            }
            cmd if cmd == "/init" => {
                let force = parts.get(1).map(|p| *p == "force").unwrap_or(false);
                if self.chat.input_event_tx.is_some() {
                    self.chat
                        .push_input_event(sdk::ChatInputEvent::InitProject { force });
                }
            }
            cmd if cmd == "/session" => {
                let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
                if self.chat.input_event_tx.is_some() {
                    self.chat
                        .push_input_event(sdk::ChatInputEvent::ManageSession { args });
                }
            }
            cmd if cmd == "/resume" => {
                if let Some(id) = parts.get(1) {
                    if self.chat.input_event_tx.is_some() {
                        self.chat
                            .push_input_event(sdk::ChatInputEvent::ResumeSession {
                                id: id.to_string(),
                            });
                    }
                }
            }
            cmd if cmd == "/model" && has_args => {
                // /model <name> — 解析参数并走 SwitchModel 事件流
                let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
                if let Some(prompt) = self.handle_model_with_args(&args).await {
                    return Some(prompt);
                }
            }
            // /memory 的 remind 子命令已被上面截胡
            // 非 remind 子命令走事件流
            cmd if cmd == "/memory" => {
                let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
                // 排除 remind 子命令（已被上面截胡）
                let first_arg = parts.get(1).copied().unwrap_or("");
                if first_arg != "remind"
                    && first_arg != "reminder"
                    && first_arg != "reminders"
                    && self.chat.input_event_tx.is_some()
                {
                    self.chat
                        .push_input_event(sdk::ChatInputEvent::ManageMemory { args });
                }
            }
            _ => {
                // Skill alias lookup
                let cmd_name = cmd.trim_start_matches('/');
                let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
                if let Some(skill) = self.find_skill_by_alias(cmd_name) {
                    let mut content = skill.content.clone();
                    if !args.is_empty() {
                        content = format!(
                            "{content}

Arguments: {args}"
                        );
                    }
                    self.append_system_notice(format!("[skill: {}]", skill.name));
                    return Some(content);
                }
                self.append_error_notice(format!("Unknown command: {cmd}"));
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

    /// 解析 /model <name> 参数，返回 Some(prompt) 如果需要发起 LLM 调用。
    async fn handle_model_with_args(&mut self, args: &str) -> Option<String> {
        let arg = args.trim();
        if arg.is_empty() {
            return None;
        }
        // 尝试从配置中查找模型
        if let Some(ref ac) = self.agent_client {
            match ac.list_models().await {
                Ok(models) => {
                    // 精确匹配或模糊匹配
                    let found = models
                        .iter()
                        .find(|m| m.id == arg || m.id.ends_with(arg) || m.name == arg);
                    if let Some(model) = found {
                        let params = sdk::ModelSwitchParams {
                            provider_name: model.provider.clone(),
                            model_id: model.id.clone(),
                            model_name: model.name.clone(),
                            base_url: String::new(),
                            api_key: String::new(),
                            driver: String::new(),
                            max_tokens: model.max_tokens,
                            context_window: model.context_window,
                            reasoning: None,
                        };
                        if self.chat.input_event_tx.is_some() {
                            self.chat
                                .push_input_event(sdk::ChatInputEvent::SwitchModel { params });
                        }
                        return None;
                    }
                    self.append_error_notice(format!(
                        "Model '{}' not found. Use /model list to see available models.",
                        arg
                    ));
                }
                Err(e) => {
                    self.append_error_notice(format!("Failed to list models: {}", e));
                }
            }
        }
        None
    }
}
