use super::*;
use crate::tui::render::output::tool_display::ToolDisplay;
use serde_json::json;

/// issue #646：可指定 role/model 的 agent header helper
fn agent_header_with_meta(description: &str, role: Option<&str>, model: Option<&str>) -> String {
    let display = AgentDisplay;
    let mut input = serde_json::Map::new();
    input.insert("description".to_string(), json!(description));
    if let Some(r) = role {
        input.insert("role".to_string(), json!(r));
    }
    if let Some(m) = model {
        input.insert("model".to_string(), json!(m));
    }
    let input = serde_json::Value::Object(input);
    let line = display.format_header_line_with_result(&input, None, None);
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

// ── issue #646: AgentDisplay header role/model 4 case ──

#[test]
fn agent_header_without_role_or_model_shows_no_meta() {
    let text = agent_header_with_meta("do something", None, None);
    assert_eq!(text, "Agent do something");
}

#[test]
fn agent_header_with_role_only_shows_role() {
    let text = agent_header_with_meta("do something", Some("coder"), None);
    assert_eq!(text, "Agent do something [role: coder]");
}

#[test]
fn agent_header_with_model_only_shows_model() {
    let text = agent_header_with_meta("do something", None, Some("Zhipu/glm-5.2"));
    assert_eq!(text, "Agent do something [model: Zhipu/glm-5.2]");
}

#[test]
fn agent_header_with_role_and_model_shows_both() {
    let text = agent_header_with_meta("do something", Some("coder"), Some("Zhipu/glm-5.2"));
    assert_eq!(
        text,
        "Agent do something [role: coder] [model: Zhipu/glm-5.2]"
    );
}
