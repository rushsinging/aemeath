use kernel::memory::MemoryLayer;
use kernel::reflection::{ReflectionEngine, ReflectionOutput};
use provider::types::SystemBlock;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::tui::app::UiEvent;

impl super::super::App {
    pub(crate) async fn handle_reflect_command_with_events(
        &mut self,
        args: &str,
        ui_tx: Option<mpsc::Sender<UiEvent>>,
    ) {
        if !self.memory_config.enabled || !self.memory_config.reflection.enabled {
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
        let messages = vec![kernel::message::Message::user(prompt)];
        let system = vec![SystemBlock::dynamic(
            "你是 Aemeath 的 Reflection 子系统。只输出 JSON，不要输出 Markdown 或解释。"
                .to_string(),
        )];
        let cancel = CancellationToken::new();

        if foreground {
            self.output_area.push_system("[reflection: calling LLM...]");
            self.output_area.start_spinner();
            self.output_area.set_spinner_phase("Reflecting...");
            self.is_processing = true;
        }

        if let Some(tx) = ui_tx {
            tokio::spawn(async move {
                if foreground {
                    let _ = tx.send(UiEvent::ReflectionStarted).await;
                }
                let raw_output = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
                let raw_output_for_callback = raw_output.clone();
                let response = match client
                    .stream_message_raw(
                        &system,
                        &messages,
                        &[],
                        Box::new(move |chunk| {
                            if let Ok(mut output) = raw_output_for_callback.lock() {
                                output.push_str(chunk);
                            }
                        }),
                        &cancel,
                    )
                    .await
                {
                    Ok(response) => response,
                    Err(error) => {
                        let _ = tx
                            .send(UiEvent::Error(format!("Reflection LLM 调用失败: {error}")))
                            .await;
                        return;
                    }
                };

                let _ = tx
                    .send(UiEvent::ReflectionUsage {
                        input: response.usage.input_tokens,
                        output: response.usage.output_tokens,
                    })
                    .await;

                let text = response.assistant_message.text_content();
                let text = if text.trim().is_empty() {
                    raw_output
                        .lock()
                        .map(|output| output.clone())
                        .unwrap_or_default()
                } else {
                    text
                };
                let output = match ReflectionEngine::parse_output(&text) {
                    Ok(output) => output,
                    Err(error) => {
                        log::warn!("Reflection 输出解析失败: {error}");
                        let _ = tx
                            .send(UiEvent::Error(format!("Reflection 输出解析失败: {error}")))
                            .await;
                        return;
                    }
                };

                let _ = tx.send(UiEvent::ReflectionDone { output }).await;
            });
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

    pub(crate) fn maybe_auto_reflect(&mut self, ui_tx: &mpsc::Sender<UiEvent>) {
        self.turn_count += 1;
        let reflection = &self.memory_config.reflection;
        if !self.memory_config.enabled || !reflection.enabled || reflection.interval_turns == 0 {
            return;
        }
        if self.pending_reflection.is_some() {
            return;
        }
        if self.turn_count % reflection.interval_turns != 0 {
            return;
        }
        self.spawn_llm_reflection(Some(ui_tx.clone()), false);
    }

    pub(crate) fn apply_reflection_output(&mut self, output: ReflectionOutput) -> bool {
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

    fn open_reflection_memory_store(&self) -> Result<kernel::memory::MemoryStore, String> {
        let base_dir = kernel::memory::memory_base_dir();
        let project_hash = kernel::memory::project_hash_from_path(&self.cwd);
        kernel::memory::MemoryStore::new(
            base_dir,
            project_hash,
            self.memory_config.max_entries,
            self.memory_config.similarity_threshold,
        )
        .map_err(|error| error.to_string())
    }
}
