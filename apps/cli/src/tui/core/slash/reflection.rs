use tokio::sync::mpsc;

use crate::tui::core::UiEvent;

impl super::super::App {
    pub(crate) async fn handle_reflect_command_with_events(
        &mut self,
        args: &str,
        ui_tx: Option<mpsc::Sender<UiEvent>>,
    ) {
        if !self.session.memory_config.enabled || !self.session.memory_config.reflection.enabled {
            self.output_area.push_error("Reflection 系统已禁用。");
            return;
        }

        match args.trim() {
            "" => self.spawn_llm_reflection(ui_tx, true),
            "apply" => self.apply_pending_reflection(),
            "stats" | "history" => self
                .output_area
                .push_system("Reflection stats/history 将在打磨阶段支持。"),
            other => self
                .output_area
                .push_error(&format!("未知 reflect 子命令: {other}")),
        }
    }

    fn spawn_llm_reflection(&mut self, ui_tx: Option<mpsc::Sender<UiEvent>>, foreground: bool) {
        let Some(agent_client) = self.agent_client.clone() else {
            self.output_area
                .push_error("当前没有可用的 SDK agent client，无法执行 Reflection。");
            return;
        };
        let messages = self.chat.messages.clone();

        if foreground {
            self.output_area.push_system("[reflection: calling LLM...]");
            self.output_area.start_spinner();
            self.output_area.set_spinner_phase("Reflecting...");
            self.chat.is_processing = true;
        }

        if let Some(tx) = ui_tx {
            tokio::spawn(async move {
                if foreground {
                    let _ = tx.send(UiEvent::ReflectionStarted).await;
                }
                match agent_client.run_reflection(messages).await {
                    Ok(output) => {
                        let _ = tx
                            .send(UiEvent::ReflectionUsage {
                                input: output.input_tokens,
                                output: output.output_tokens,
                            })
                            .await;
                        let _ = tx.send(UiEvent::ReflectionDone { output }).await;
                    }
                    Err(error) => {
                        let _ = tx
                            .send(UiEvent::Error(format!("Reflection LLM 调用失败: {error}")))
                            .await;
                    }
                }
            });
        }
    }

    fn apply_pending_reflection(&mut self) {
        let Some(output) = self.chat.pending_reflection.clone() else {
            self.output_area
                .push_system("没有待应用的 Reflection 建议。");
            return;
        };

        if self.apply_reflection_output(output) {
            self.chat.pending_reflection = None;
        }
    }

    pub(crate) fn maybe_auto_reflect(&mut self, ui_tx: &mpsc::Sender<UiEvent>) {
        self.chat.turn_count += 1;
        let reflection = &self.session.memory_config.reflection;
        if !self.session.memory_config.enabled
            || !reflection.enabled
            || reflection.interval_turns == 0
        {
            return;
        }
        if self.chat.pending_reflection.is_some() {
            return;
        }
        if self.chat.turn_count % reflection.interval_turns != 0 {
            return;
        }
        self.spawn_llm_reflection(Some(ui_tx.clone()), false);
    }

    pub(crate) fn apply_reflection_output(&mut self, output: sdk::ReflectionOutputView) -> bool {
        let Some(agent_client) = self.agent_client.clone() else {
            self.output_area
                .push_error("当前没有可用的 SDK agent client，无法应用 Reflection。");
            return false;
        };
        tokio::spawn(async move {
            let _ = agent_client.apply_reflection(output).await;
        });
        self.output_area
            .push_system("[reflection apply 已提交给 SDK memory 能力]");
        true
    }
}
