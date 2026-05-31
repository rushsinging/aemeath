//! 聊天相关纯数据状态

/// 聊天会话的所有可变数据（不含视图组件 output_area）
#[derive(Debug)]
pub(crate) struct ChatState {
    pub messages: Vec<sdk::ChatMessage>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_api_calls: u64,
    pub last_input_tokens: u64,
    pub pending_images: Vec<sdk::ClipboardImageView>,
    pub system_prompt_text: String,
    pub context_size: usize,
    pub tool_call_active: bool,
    pub active_tool_call_ids: std::collections::HashSet<String>,
    pub turn_count: usize,
    pub pending_reflection: Option<sdk::ReflectionOutputView>,
    pub input_event_buffer: Option<std::sync::Arc<std::sync::Mutex<Vec<sdk::ChatInputEvent>>>>,
    pub is_processing: bool,
}

impl ChatState {
    pub(crate) fn clear_tool_activity(&mut self) {
        self.tool_call_active = false;
        self.active_tool_call_ids.clear();
    }

    pub(crate) fn start_tool_activity(&mut self) {
        self.tool_call_active = true;
    }

    pub(crate) fn register_tool_call(&mut self, id: String) {
        self.tool_call_active = true;
        self.active_tool_call_ids.insert(id);
    }

    pub(crate) fn has_active_tool_call(&self, id: &str) -> bool {
        self.active_tool_call_ids.contains(id)
    }

    pub(crate) fn finish_tool_call(&mut self, id: &str) -> usize {
        self.active_tool_call_ids.remove(id);
        let remaining = self.active_tool_call_ids.len();
        self.tool_call_active = remaining > 0;
        remaining
    }

    pub(crate) fn record_usage(&mut self, input: u64, output: u64, last_input: u64) {
        self.total_input_tokens += input;
        self.total_output_tokens += output;
        self.total_api_calls += 1;
        self.last_input_tokens = last_input;
    }

    pub(crate) fn add_pending_image(&mut self, image: sdk::ClipboardImageView) -> usize {
        self.pending_images.push(image);
        self.pending_images.len()
    }

    pub(crate) fn clear_pending_images(&mut self) {
        self.pending_images.clear();
    }

    pub(crate) fn drain_pending_images(&mut self) -> Vec<sdk::ClipboardImageView> {
        self.pending_images.drain(..).collect()
    }

    pub(crate) fn start_input_event_buffer(
        &mut self,
    ) -> std::sync::Arc<std::sync::Mutex<Vec<sdk::ChatInputEvent>>> {
        let buffer = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        self.input_event_buffer = Some(buffer.clone());
        buffer
    }

    pub(crate) fn clear_input_event_buffer(&mut self) {
        self.input_event_buffer = None;
    }

    pub(crate) fn push_input_event(&mut self, event: sdk::ChatInputEvent) -> usize {
        let Some(buffer) = &self.input_event_buffer else {
            return 0;
        };
        let Ok(mut events) = buffer.lock() else {
            return 0;
        };
        events.push(event);
        events.len()
    }

    pub(crate) fn pending_image_count(&self) -> usize {
        self.pending_images.len()
    }

    pub(crate) fn pending_images(&self) -> &[sdk::ClipboardImageView] {
        &self.pending_images
    }

    pub(crate) fn reset_runtime_state(&mut self) {
        self.total_input_tokens = 0;
        self.total_output_tokens = 0;
        self.total_api_calls = 0;
        self.last_input_tokens = 0;
        self.clear_tool_activity();
        self.is_processing = false;
        self.pending_reflection = None;
        self.clear_input_event_buffer();
        self.turn_count = 0;
    }

    pub(crate) fn start_processing(&mut self) {
        self.is_processing = true;
    }

    pub(crate) fn stop_processing(&mut self) {
        self.clear_tool_activity();
        self.is_processing = false;
    }
    pub(crate) fn usage_snapshot(&self) -> ChatUsageSnapshot {
        ChatUsageSnapshot {
            total_input_tokens: self.total_input_tokens,
            total_output_tokens: self.total_output_tokens,
            last_input_tokens: self.last_input_tokens,
            total_api_calls: self.total_api_calls,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ChatUsageSnapshot {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub last_input_tokens: u64,
    pub total_api_calls: u64,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_api_calls: 0,
            last_input_tokens: 0,
            pending_images: Vec::new(),
            system_prompt_text: String::new(),
            context_size: 200_000,
            tool_call_active: false,
            active_tool_call_ids: std::collections::HashSet::new(),
            turn_count: 0,
            pending_reflection: None,
            input_event_buffer: None,
            is_processing: false,
        }
    }
}
