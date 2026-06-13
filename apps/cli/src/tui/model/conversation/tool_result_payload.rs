use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct ToolResultPayload {
    pub output: String,
    pub content: Value,
    pub is_error: bool,
    pub image_count: usize,
}

impl ToolResultPayload {
    pub fn new(output: String, content: Value, is_error: bool, image_count: usize) -> Self {
        Self {
            output,
            content,
            is_error,
            image_count,
        }
    }
}
