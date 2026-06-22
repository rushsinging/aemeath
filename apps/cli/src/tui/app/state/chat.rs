//! 聊天相关纯数据状态

/// 聊天会话的所有可变数据（不含视图组件 output_area）
#[derive(Debug)]
pub(crate) struct ChatState {
    pub messages: Vec<sdk::ChatMessage>,
    pub system_prompt_text: String,
    pub context_size: usize,
    pub tool_call_active: bool,
    pub active_tool_call_ids: std::collections::HashSet<sdk::ids::ToolCallId>,
    pub turn_count: usize,
    pub pending_reflection: Option<sdk::ReflectionOutputView>,
    pub applying_reflection: Option<sdk::ReflectionOutputView>,
    pub input_event_tx: Option<tokio::sync::mpsc::UnboundedSender<sdk::ChatInputEvent>>,
    pub processing_handle: Option<crate::tui::effect::session::processing::ProcessingHandle>,
    pub is_processing: bool,
    pub is_cancelling: bool,
}

impl ChatState {
    pub(crate) fn clear_tool_activity(&mut self) {
        self.tool_call_active = false;
        self.active_tool_call_ids.clear();
    }

    pub(crate) fn start_tool_activity(&mut self) {
        self.tool_call_active = true;
    }

    pub(crate) fn register_tool_call(&mut self, id: sdk::ids::ToolCallId) {
        self.tool_call_active = true;
        self.active_tool_call_ids.insert(id);
    }

    pub(crate) fn has_active_tool_call(&self, id: &sdk::ids::ToolCallId) -> bool {
        self.active_tool_call_ids.contains(id)
    }

    pub(crate) fn finish_tool_call(&mut self, id: &sdk::ids::ToolCallId) -> usize {
        self.active_tool_call_ids.remove(id);
        let remaining = self.active_tool_call_ids.len();
        self.tool_call_active = remaining > 0;
        remaining
    }

    pub(crate) fn start_input_event_buffer(
        &mut self,
    ) -> crate::tui::effect::session::processing::TuiInputEventPort {
        let (tx, port) = crate::tui::effect::session::processing::TuiInputEventPort::channel();
        self.input_event_tx = Some(tx);
        port
    }

    pub(crate) fn clear_input_event_buffer(&mut self) {
        self.input_event_tx = None;
    }

    pub(crate) fn push_input_event(&mut self, event: sdk::ChatInputEvent) -> usize {
        let Some(tx) = &self.input_event_tx else {
            return 0;
        };
        // send() only fails when receiver dropped; treat as unavailable.
        if tx.send(event).is_err() {
            self.input_event_tx = None;
            return 0;
        }
        1
    }

    pub(crate) fn reset_runtime_state(&mut self) {
        self.clear_tool_activity();
        self.is_processing = false;
        self.is_cancelling = false;
        self.pending_reflection = None;
        self.applying_reflection = None;
        self.clear_processing_handle();
        self.turn_count = 0;
    }

    pub(crate) fn start_processing(&mut self) {
        self.is_processing = true;
        self.is_cancelling = false;
    }

    pub(crate) fn stop_processing(&mut self) {
        self.clear_tool_activity();
        self.is_processing = false;
        self.is_cancelling = false;
    }

    pub(crate) fn start_cancelling(&mut self) {
        self.is_cancelling = true;
    }

    pub(crate) fn set_processing_handle(
        &mut self,
        handle: crate::tui::effect::session::processing::ProcessingHandle,
    ) {
        if let Some(old_handle) = self.processing_handle.take() {
            old_handle.abort();
        }
        self.processing_handle = Some(handle);
    }

    pub(crate) fn abort_processing_handle(&mut self) {
        if let Some(handle) = self.processing_handle.take() {
            handle.abort();
        }
        self.clear_tool_activity();
        self.is_processing = false;
        self.is_cancelling = false;
    }

    pub(crate) fn clear_processing_handle(&mut self) {
        self.processing_handle = None;
        self.is_cancelling = false;
    }
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            system_prompt_text: String::new(),
            context_size: 200_000,
            tool_call_active: false,
            active_tool_call_ids: std::collections::HashSet::new(),
            turn_count: 0,
            pending_reflection: None,
            applying_reflection: None,
            input_event_tx: None,
            processing_handle: None,
            is_processing: false,
            is_cancelling: false,
        }
    }
}
