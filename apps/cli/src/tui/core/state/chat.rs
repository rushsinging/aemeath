//! 聊天相关纯数据状态

/// 聊天会话的所有可变数据（不含视图组件 output_area）
#[derive(Debug)]
pub(crate) struct ChatState {
    pub messages: Vec<sdk::ChatMessage>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_api_calls: u64,
    pub last_input_tokens: u64,
    pub pending_images: Vec<::runtime::api::image::ProcessedImage>,
    pub system_prompt_text: String,
    pub context_size: usize,
    pub tool_call_active: bool,
    pub active_tool_call_ids: std::collections::HashSet<String>,
    pub turn_count: usize,
    pub pending_reflection: Option<::runtime::api::reflection::ReflectionOutput>,
    pub is_processing: bool,
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
            is_processing: false,
        }
    }
}
