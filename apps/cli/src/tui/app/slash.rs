mod dialog;
pub mod help;
mod help_display;
mod reflection;
mod suggestions;

use crate::tui::app::UiEvent;
use crate::tui::effect::effect::Effect;

pub(crate) fn resolve_slash_for_delivery(
    router: &dyn sdk::CommandRouterPort,
    input: &str,
) -> Result<sdk::CommandRoute, sdk::CommandParseError> {
    router.resolve(sdk::SlashInput::new(input))
}

impl super::App {
    /// Handle slash commands with an optional UI event sender for background commands.
    /// Returns Some(prompt) if a message should be sent to the LLM (e.g. /review).
    pub(crate) async fn handle_slash_command_with_events(
        &mut self,
        input: &str,
        ui_tx: Option<tokio::sync::mpsc::Sender<UiEvent>>,
    ) -> Option<String> {
        let route = match self.command_router.as_deref() {
            Some(router) => match resolve_slash_for_delivery(router, input) {
                Ok(route) => route,
                Err(error) => {
                    self.append_error_notice(error.to_string());
                    return None;
                }
            },
            None => {
                self.append_error_notice("Command router unavailable.");
                return None;
            }
        };
        let command = match &route {
            sdk::CommandRoute::PromptInjection(command) => command.command.as_str(),
            sdk::CommandRoute::SnapshotQuery { command, .. } => command.command.as_str(),
            sdk::CommandRoute::ApplicationControl { command, .. } => command.command.as_str(),
        };
        let arguments = match &route {
            sdk::CommandRoute::PromptInjection(command) => command.arguments.as_slice(),
            sdk::CommandRoute::SnapshotQuery { command, .. } => command.arguments.as_slice(),
            sdk::CommandRoute::ApplicationControl { command, .. } => command.arguments.as_slice(),
        };
        let has_args = !arguments.is_empty();
        let args = arguments.join(" ");

        if command == "model" && !has_args {
            return self.open_model_selection_dialog();
        }

        match command {
            command if matches!(route, sdk::CommandRoute::PromptInjection(_)) => {
                let Some(skill) = self.find_skill_by_alias(command) else {
                    self.append_error_notice(format!("Prompt command unavailable: /{command}"));
                    return None;
                };
                let mut content = skill.content.clone();
                if !args.is_empty() {
                    content = format!("{content}\n\nArguments: {args}");
                }
                self.append_system_notice(format!("[skill: {}]", skill.name));
                return Some(content);
            }
            "exit" => self.layout.request_exit(),
            "clear" => {
                self.clear_conversation().await;
                self.append_system_notice("[conversation cleared]");
            }
            "compact" => {
                // #497 子 issue 0：走 runtime 事件流（ChatInputEvent::Compact →
                // manual_compact），不再直接调 compact_messages().await。
                // spinner / 进度 Gauge / 结果回显全部由 runtime 的
                // PreCompact → CompactProgress → PostCompact → SystemMessage 事件驱动。
                if self.chat.input_event_tx.is_some() {
                    self.chat.push_input_event(sdk::ChatInputEvent::Compact);
                } else {
                    self.append_system_notice("[compact skipped: chat loop not running]");
                }
            }
            "help" => self.show_slash_help(),
            "usage" => {
                let usage = &self.model.conversation.runtime.usage;
                let total = usage.input_tokens + usage.output_tokens;
                self.append_system_notice(format!(
                    "API calls: {} | Tokens: {} in / {} out / {} total",
                    usage.api_calls,
                    sdk::format_tokens(usage.input_tokens),
                    sdk::format_tokens(usage.output_tokens),
                    sdk::format_tokens(total)
                ));
            }
            "save" => {
                if let Some(tx) = ui_tx.clone() {
                    self.execute_effect(Effect::SaveSession { notify: true }, &tx)
                        .await;
                }
            }
            "context" => {
                // #567: EstimateContext 变体已删除，改为本地渲染消息计数。
                self.append_system_notice(format!(
                    "Messages: {}",
                    self.model.conversation.timeline.items().len()
                ));
            }
            "reflect" => {
                let effects = self.handle_reflect_command(&args);
                if let Some(tx) = ui_tx.clone() {
                    for effect in effects {
                        self.execute_effect(effect, &tx).await;
                    }
                }
            }
            "memory"
                if matches!(
                    arguments.first().map(String::as_str),
                    Some("remind" | "reminder" | "reminders")
                ) =>
            {
                if let Some(tx) = ui_tx.clone() {
                    self.execute_effect(Effect::FetchMemoryList, &tx).await;
                }
            }
            "update" => {
                if let Some(tx) = ui_tx.clone() {
                    self.execute_effect(Effect::RunSelfUpdate, &tx).await;
                }
            }
            "paste" => {
                if let Some(tx) = ui_tx.clone() {
                    self.execute_effect(Effect::ReadClipboardImage, &tx).await;
                }
            }
            "images" => {
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
            "clear-images" => {
                self.model.input.document.remove_all_images();
                self.append_system_notice("[pending images cleared]");
            }
            "version" => {
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
            "doctor" => {
                let view = &self.config_view;
                let home = dirs::home_dir();
                let info = format!(
                    "🔧 Doctor\n\n                     \
                     Model: {}\n                     \
                     API Key: {}\n                     \
                     Permission: {}\n                     \
                     Home dir: {}\n                     \
                     Architecture: {}",
                    view.model_name,
                    if view.has_api_key {
                        "✅ set"
                    } else {
                        "❌ not set"
                    },
                    view.permission_mode,
                    home.map(|p| p.display().to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    std::env::consts::ARCH,
                );
                self.append_system_notice(&info);
            }
            "rewind" => {
                // /rewind <N> → 触发 compact（保留 N 条消息的语义通过 compact 实现）
                if self.chat.input_event_tx.is_some() {
                    self.chat.push_input_event(sdk::ChatInputEvent::Compact);
                }
            }
            "cost" => {
                // #567: QueryCost 变体已删除，改为本地从 model 状态渲染。
                let usage = &self.model.conversation.runtime.usage;
                let total = usage.input_tokens + usage.output_tokens;
                self.append_system_notice(format!(
                    "API calls: {} | Tokens: {} in / {} out / {} total",
                    usage.api_calls,
                    sdk::format_tokens(usage.input_tokens),
                    sdk::format_tokens(usage.output_tokens),
                    sdk::format_tokens(total)
                ));
            }
            "status" => {
                // #567: QueryStatus 变体已删除，改为本地渲染状态信息。
                let view = &self.config_view;
                self.append_system_notice(format!(
                    "Model: {} | Permission: {} | Processing: {}",
                    view.model_name, view.permission_mode, self.chat.is_processing,
                ));
            }
            "config" => {
                // #567: QueryConfig 变体已删除，改为本地从 config_view 渲染。
                let view = &self.config_view;
                self.append_system_notice(format!(
                    "Model: {}\nProvider: {}\nAPI Key: {}\nPermission: {}\nContext size: {}\nMarkdown: {}\nVerbose: {}\nLogging: {}",
                    view.model_name,
                    view.provider.as_deref().unwrap_or("auto"),
                    if view.has_api_key { "✅ set" } else { "❌ not set" },
                    view.permission_mode,
                    view.context_size,
                    view.markdown,
                    view.verbose,
                    view.logging_level,
                ));
            }
            "stats" => {
                // #567: QueryStats 变体已删除，改为本地从 model 状态渲染。
                let usage = &self.model.conversation.runtime.usage;
                self.append_system_notice(format!(
                    "Messages: {} | API calls: {} | Tokens: {} total",
                    self.model.conversation.timeline.items().len(),
                    usage.api_calls,
                    sdk::format_tokens(usage.input_tokens + usage.output_tokens)
                ));
            }
            "init" => {
                let force = arguments.first().is_some_and(|p| p == "force");
                if self.chat.input_event_tx.is_some() {
                    self.chat
                        .push_input_event(sdk::ChatInputEvent::InitProject { force });
                }
            }
            "session" => {
                let args = args.clone();
                if self.chat.input_event_tx.is_some() {
                    self.chat
                        .push_input_event(sdk::ChatInputEvent::ManageSession { args });
                }
            }
            "resume" => {
                if let Some(id) = arguments.first() {
                    if self.chat.input_event_tx.is_some() {
                        self.chat
                            .push_input_event(sdk::ChatInputEvent::ResumeSession {
                                id: id.to_string(),
                            });
                    }
                }
            }
            "model" if has_args => {
                // /model <name> — 解析参数并走 SwitchModel 事件流
                let args = args.clone();
                if let Some(prompt) = self.handle_model_with_args(&args).await {
                    return Some(prompt);
                }
            }
            // /memory 的 remind 子命令已被上面截胡
            // 非 remind 子命令走事件流
            "memory" => {
                let args = args.clone();
                // 排除 remind 子命令（已被上面截胡）
                let first_arg = arguments.first().map(String::as_str).unwrap_or("");
                if first_arg != "remind"
                    && first_arg != "reminder"
                    && first_arg != "reminders"
                    && self.chat.input_event_tx.is_some()
                {
                    self.chat
                        .push_input_event(sdk::ChatInputEvent::ManageMemory { args });
                }
            }
            _ => self.append_error_notice(format!("Unsupported command route: /{command}")),
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
        self.handle_input_intent(crate::tui::model::input::intent::InputIntent::Clear);
        self.output_area.clear();
        if self.chat.input_event_tx.is_some() {
            // loop 运行中：发 Reset，由 runtime gate 统一清空。
            // cancel 通过 ProcessingHandle 管理（#567 S4），不再调 ac.cancel()。
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
        // 直接将 selection 字符串转发给 runtime，由 runtime 通过
        // `resolve_model_selection` 解析（#567）。
        if self.chat.input_event_tx.is_some() {
            self.chat
                .push_input_event(sdk::ChatInputEvent::SwitchModel {
                    selection: arg.to_string(),
                });
        }
        None
    }
}
