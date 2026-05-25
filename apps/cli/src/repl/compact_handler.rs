use ::runtime::api::provider::stream::StreamHandler;

/// Silent handler for LLM-based compaction (no terminal output).
pub(crate) struct SilentCompactHandler;
impl StreamHandler for SilentCompactHandler {
    fn on_text(&mut self, _text: &str) {}
    fn on_tool_use_start(&mut self, _name: &str, _index: usize) {}
    fn on_error(&mut self, _error: &str) {}
}
