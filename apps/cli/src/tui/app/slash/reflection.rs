use crate::tui::effect::effect::Effect;

impl super::super::App {
    /// 处理 /reflect 命令，返回需由调用方执行的副作用 Effect（保持 update 纯净）。
    pub(crate) fn handle_reflect_command(&mut self, args: &str) -> Vec<Effect> {
        if !self.session.memory_config.enabled || !self.session.memory_config.reflection.enabled {
            self.append_error_notice("Reflection 系统已禁用。");
            return Vec::new();
        }

        match args.trim() {
            "" => {
                if self.chat.pending_reflection.is_some() {
                    self.append_system_notice("已有未应用建议，本次将刷新");
                }
                self.prepare_llm_reflection(true).into_iter().collect()
            }
            "apply" => self.apply_pending_reflection().into_iter().collect(),
            "stats" | "history" => {
                self.append_system_notice("Reflection stats/history 将在打磨阶段支持。");
                Vec::new()
            }
            other => {
                self.append_error_notice(format!("未知 reflect 子命令: {other}"));
                Vec::new()
            }
        }
    }

    /// 准备一次 LLM reflection：前台模式下设置 spinner/processing 状态，
    /// 返回 RunReflection Effect 交由 executor 后台执行（不在此处 spawn）。
    fn prepare_llm_reflection(&mut self, foreground: bool) -> Option<Effect> {
        if self.agent_client.is_none() {
            self.append_error_notice("当前没有可用的 SDK agent client，无法执行 Reflection。");
            return None;
        }

        if foreground {
            self.append_system_notice("[reflection: calling LLM...]");
            self.spinner_phase(crate::tui::model::runtime::spinner::SpinnerPhase::Reflecting);
            self.chat.is_processing = true;
        }

        Some(Effect::RunReflection { foreground })
    }

    fn apply_pending_reflection(&mut self) -> Option<Effect> {
        if self.chat.applying_reflection.is_some() {
            self.append_system_notice("Reflection apply 正在进行中");
            return None;
        }

        let Some(output) = self.chat.pending_reflection.take() else {
            self.append_system_notice("没有待应用的 Reflection 建议。");
            return None;
        };

        let effect = self.apply_reflection_output(output.clone());
        if effect.is_some() {
            self.chat.applying_reflection = Some(output);
            self.append_system_notice("[reflection apply 已提交给 SDK memory 能力]");
        } else {
            self.chat.pending_reflection = Some(output);
        }
        effect
    }

    /// 自动 reflection：到达 interval 时返回后台 RunReflection Effect。
    pub(crate) fn maybe_auto_reflect(&mut self) -> Option<Effect> {
        self.chat.turn_count += 1;
        let reflection = &self.session.memory_config.reflection;
        if !self.session.memory_config.enabled
            || !reflection.enabled
            || reflection.interval_turns == 0
        {
            return None;
        }
        if self.chat.pending_reflection.is_some() {
            return None;
        }
        if !self
            .chat
            .turn_count
            .is_multiple_of(reflection.interval_turns)
        {
            return None;
        }
        self.prepare_llm_reflection(false)
    }

    /// 返回 ApplyReflection Effect，将 reflection 输出交由 executor 后台应用。
    pub(crate) fn apply_reflection_output(
        &mut self,
        output: sdk::ReflectionOutputView,
    ) -> Option<Effect> {
        if self.agent_client.is_none() {
            self.append_error_notice("当前没有可用的 SDK agent client，无法应用 Reflection。");
            return None;
        }
        Some(Effect::ApplyReflection { output })
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::app::App;
    use std::path::PathBuf;

    fn make_app() -> App {
        App::new("s".to_string(), PathBuf::from("/tmp"), "m".to_string())
    }

    #[test]
    fn test_handle_reflect_command_disabled_returns_no_effect() {
        let mut app = make_app();
        app.session.memory_config.enabled = false;
        let effects = app.handle_reflect_command("");
        assert!(effects.is_empty());
    }

    #[test]
    fn test_handle_reflect_command_unknown_subcommand_returns_no_effect() {
        let mut app = make_app();
        app.session.memory_config.enabled = true;
        app.session.memory_config.reflection.enabled = true;
        let effects = app.handle_reflect_command("bogus");
        assert!(effects.is_empty());
    }

    #[test]
    fn test_apply_reflection_output_without_client_returns_none() {
        let mut app = make_app();
        // 无 agent_client（App::new 默认无）-> 返回 None 并记录错误。
        let effect = app.apply_reflection_output(sdk::ReflectionOutputView {
            content: "c".to_string(),
            suggested_memories: Vec::new(),
            outdated_memories: Vec::new(),
            input_tokens: 0,
            output_tokens: 0,
            auto_applied: false,
        });
        assert!(effect.is_none());
    }
}
