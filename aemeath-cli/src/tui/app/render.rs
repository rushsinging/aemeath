use crate::tui::output_area::{LineStyle, OutputLine};
use aemeath_core::message::{Message, Role};

impl super::App {
    /// Render a saved history message into the output area (used during session resume)
    pub fn render_history_message(&mut self, msg: &Message) {
        match msg.role {
            Role::User => {
                let user_text = msg.content.iter()
                    .filter_map(|block| match block {
                        aemeath_core::message::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                self.output_area.push_line(OutputLine {
                    content: format!("> {}", user_text),
                    style: LineStyle::User,
                    ..Default::default()
                });
            }
            Role::Assistant => {
                for block in &msg.content {
                    match block {
                        aemeath_core::message::ContentBlock::Text { text } => {
                            self.output_area.push_line(OutputLine {
                                content: text.clone(),
                                style: LineStyle::Assistant,
                                ..Default::default()
                            });
                        }
                        aemeath_core::message::ContentBlock::ToolUse { name, input, .. } => {
                            let input_json = input.to_string();
                            self.output_area.push_completed_tool_call(name, &input_json);
                        }
                        aemeath_core::message::ContentBlock::ToolResult { tool_use_id: _, content, is_error, .. } => {
                            let content_str = match content {
                                serde_json::Value::String(s) => s.clone(),
                                serde_json::Value::Array(arr) => {
                                    arr.iter()
                                        .filter_map(|v| v.get("text").and_then(|t| t.as_str()))
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                }
                                _ => content.to_string(),
                            };
                            let style = if *is_error { LineStyle::ToolCallError } else { LineStyle::ToolResult };
                            for line in content_str.lines() {
                                self.output_area.push_line(OutputLine {
                                    content: format!("  {}", line),
                                    style,
                                    ..Default::default()
                                });
                            }
                        }
                        _ => {}
                    }
                }
                self.output_area.push_line(OutputLine {
                    content: String::new(),
                    style: LineStyle::System,
                    ..Default::default()
                });
            }
        }
    }
}
