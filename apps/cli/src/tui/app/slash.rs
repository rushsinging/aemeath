mod dialog;
pub mod help;
mod help_display;
mod reflection;
mod suggestions;

use super::update::UpdateResult;
use crate::tui::app::App;
use crate::tui::effect::effect::Effect;

pub(crate) fn resolve_slash_for_delivery(
    router: &dyn sdk::CommandRouterPort,
    input: &str,
) -> Result<sdk::CommandRoute, sdk::CommandParseError> {
    router.resolve(sdk::SlashInput::new(input))
}

impl App {
    /// 纯同步 slash 分发：只做状态写入并返回 `UpdateResult`，所有 I/O
    /// 经 `Effect` 由 run_loop 的统一 effect 循环执行。
    pub(crate) fn handle_slash_command(&mut self, input: &str) -> UpdateResult {
        let route = if let Some(request) = self.skill_completion_catalog.resolve(input) {
            sdk::CommandRoute::SkillRequest(request)
        } else {
            match self.command_router.as_deref() {
                Some(router) => match resolve_slash_for_delivery(router, input) {
                    Ok(route) => route,
                    Err(error) => {
                        self.append_error_notice(error.to_string());
                        return UpdateResult::none();
                    }
                },
                None => {
                    self.append_error_notice("Command router unavailable.");
                    return UpdateResult::none();
                }
            }
        };
        let command = match &route {
            sdk::CommandRoute::SkillRequest(command) => command.command.as_str(),
            sdk::CommandRoute::SnapshotQuery { command, .. } => command.command.as_str(),
            sdk::CommandRoute::ApplicationControl { command, .. } => command.command.as_str(),
        };
        let arguments = match &route {
            sdk::CommandRoute::SkillRequest(command) => command.arguments.as_slice(),
            sdk::CommandRoute::SnapshotQuery { command, .. } => command.arguments.as_slice(),
            sdk::CommandRoute::ApplicationControl { command, .. } => command.arguments.as_slice(),
        };
        let has_args = !arguments.is_empty();
        let args = arguments.join(" ");

        if command == "model" && !has_args {
            // #740：用已回填缓存呈现对话框（或挂起等待），并附带 ListModels 刷新。
            return UpdateResult::one(self.open_model_selection_dialog());
        }

        match command {
            _command if matches!(route, sdk::CommandRoute::SkillRequest(_)) => {
                let sdk::CommandRoute::SkillRequest(request) = route else {
                    unreachable!("matched SkillRequest route")
                };
                crate::tui::log_debug!(
                    "skill_request boundary=tui_slash_to_runtime skill={} arguments_len={} raw_input_len={} raw_input_preview={:?}",
                    request.skill,
                    args.len(),
                    input.len(),
                    input.chars().take(120).collect::<String>()
                );
                let event = sdk::ChatInputEvent::SkillRequest(sdk::SkillRequest {
                    input_id: sdk::InputId::new_v7(),
                    skill: request.skill,
                    arguments: args,
                    raw_input: input.to_string(),
                });
                UpdateResult::one(Effect::SendChatInputEvent { event })
            }
            "exit" => {
                self.layout.request_exit();
                UpdateResult::none()
            }
            "clear" => {
                let effects = self.clear_conversation().into_iter().collect();
                self.append_system_notice("[conversation cleared]");
                UpdateResult {
                    effects,
                    spawn_effect: None,
                }
            }
            "compact" => {
                // 走 Runtime typed 事件流（ChatInputEvent::Compact → manual_compact），
                // 不在 TUI 直接压缩；进度与结果仅由 Runtime Activity/结果事件驱动。
                UpdateResult::one(Effect::SendChatInputEvent {
                    event: sdk::ChatInputEvent::Compact,
                })
            }
            "help" => {
                self.show_slash_help();
                UpdateResult::none()
            }
            "usage" | "cost" => {
                let usage = &self.model.conversation.runtime.usage;
                let total = usage.input_tokens + usage.output_tokens;
                self.append_system_notice(format!(
                    "API calls: {} | Tokens: {} in / {} out / {} total",
                    usage.api_calls,
                    sdk::format_tokens(usage.input_tokens),
                    sdk::format_tokens(usage.output_tokens),
                    sdk::format_tokens(total)
                ));
                UpdateResult::none()
            }
            "context" => {
                // #567: EstimateContext 变体已删除，改为本地渲染消息计数。
                self.append_system_notice(format!(
                    "Messages: {}",
                    self.model.conversation.timeline.items().len()
                ));
                UpdateResult::none()
            }
            "reflect" => {
                let effects = self.handle_reflect_command(&args);
                UpdateResult {
                    effects,
                    spawn_effect: None,
                }
            }
            "memory"
                if matches!(
                    arguments.first().map(String::as_str),
                    Some("remind" | "reminder" | "reminders")
                ) =>
            {
                UpdateResult::one(Effect::FetchMemoryList)
            }
            "update" => UpdateResult::one(Effect::RunSelfUpdate),
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
                UpdateResult::none()
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
                    home.map(|path| path.display().to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    std::env::consts::ARCH,
                );
                self.append_system_notice(&info);
                UpdateResult::none()
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
                UpdateResult::none()
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
                UpdateResult::none()
            }
            "init" => {
                let force = arguments.first().is_some_and(|param| param == "force");
                UpdateResult::one(Effect::SendChatInputEvent {
                    event: sdk::ChatInputEvent::InitProject { force },
                })
            }
            "session" => {
                let args = args.clone();
                UpdateResult::one(Effect::SendChatInputEvent {
                    event: sdk::ChatInputEvent::ManageSession { args },
                })
            }
            "resume" => {
                if let Some(id) = arguments.first() {
                    let id = id.to_string();
                    return UpdateResult::one(Effect::SendChatInputEvent {
                        event: sdk::ChatInputEvent::ResumeSession { id },
                    });
                }
                UpdateResult::none()
            }
            "model" if has_args => {
                // /model <name> — 解析参数并走 SwitchModel 事件流
                let effects = self.handle_model_with_args(&args).into_iter().collect();
                UpdateResult {
                    effects,
                    spawn_effect: None,
                }
            }
            // /memory 的 remind 子命令已被上面截胡
            // 非 remind 子命令走事件流
            "memory" => {
                let args = args.clone();
                // 排除 remind 子命令（已被上面截胡）
                let first_arg = arguments.first().map(String::as_str).unwrap_or("");
                if first_arg != "remind" && first_arg != "reminder" && first_arg != "reminders" {
                    return UpdateResult::one(Effect::SendChatInputEvent {
                        event: sdk::ChatInputEvent::ManageMemory { args },
                    });
                }
                UpdateResult::none()
            }
            _ => {
                self.append_error_notice(format!("Unsupported command route: /{command}"));
                UpdateResult::none()
            }
        }
    }

    /// #391 方案 B：清空会话。
    ///
    /// 即时清 TUI 状态（messages/output/输入框），loop 运行时经
    /// `ChatInputEvent::Reset`（SendChatInputEvent Effect）让 runtime idle gate
    /// 统一清空 runtime messages；loop 未运行时 fallback 到 `reset_runtime_state`
    /// 本地清理。
    ///
    /// `SessionReset` 事件回来后经 `Effect::ResetRuntimeState` 再做完整清理
    ///（sync agent_client + clear_tasks）；loop 不再被 drop，保持存活。
    fn clear_conversation(&mut self) -> Option<Effect> {
        self.handle_input_intent(crate::tui::model::input::intent::InputIntent::Clear);
        self.output_area.clear();
        if self.chat.input_event_tx.is_some() {
            // loop 运行中：发 Reset，由 runtime gate 统一清空。
            // cancel 通过 ProcessingHandle 管理（#567 S4），不再调 ac.cancel()。
            Some(Effect::SendChatInputEvent {
                event: sdk::ChatInputEvent::Reset,
            })
        } else {
            // loop 未运行（如启动前）→ 直接本地清理。
            self.reset_runtime_state();
            None
        }
    }

    /// 解析 /model <name> 参数，经 `SwitchModel` 事件流由 runtime 通过
    /// `resolve_model_selection` 解析（#567）。
    fn handle_model_with_args(&mut self, args: &str) -> Option<Effect> {
        let arg = args.trim();
        if arg.is_empty() {
            return None;
        }
        Some(Effect::SendChatInputEvent {
            event: sdk::ChatInputEvent::SwitchModel {
                selection: arg.to_string(),
            },
        })
    }
}
