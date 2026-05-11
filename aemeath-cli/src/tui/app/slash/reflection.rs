use aemeath_core::memory::MemoryLayer;
use aemeath_core::reflection::{ReflectionEngine, ReflectionOutput};
use aemeath_llm::types::SystemBlock;
use tokio_util::sync::CancellationToken;

impl super::super::App {
    pub(crate) async fn handle_reflect_command(&mut self, args: &str) {
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
}
