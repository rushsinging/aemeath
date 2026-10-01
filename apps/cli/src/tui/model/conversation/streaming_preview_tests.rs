use super::*;

fn policy() -> ToolStreamingPreviewPolicy {
    ToolStreamingPreviewPolicy::new(3, true, 8)
}

#[test]
fn tail_mode_keeps_last_max_lines() {
    let mut buffer = ToolStreamingPreviewBuffer::new(policy());
    for text in ["a", "b", "c", "d"] {
        buffer.push_activity(AgentActivityLine::message(text.to_string()));
    }
    let lines = buffer.display_lines();
    let texts: Vec<&str> = lines.iter().map(|l| l.text().unwrap_or_default()).collect();
    assert_eq!(texts, vec!["b", "c", "d"]);
}

#[test]
fn truncates_long_lines() {
    let mut buffer = ToolStreamingPreviewBuffer::new(policy());
    buffer.push_activity(AgentActivityLine::message("1234567890".to_string()));
    buffer.push_activity(AgentActivityLine::message("abcdefghi".to_string()));
    let lines = buffer.display_lines();
    let texts: Vec<&str> = lines.iter().map(|l| l.text().unwrap_or_default()).collect();
    assert_eq!(texts, vec!["1234567…", "abcdefg…"]);
}
