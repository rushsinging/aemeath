use crate::tui::render::output_area::OutputArea;
use sdk::{AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView};

#[test]
fn test_push_agent_progress_replaces_tool_calls_for_same_agent() {
    let mut output = OutputArea::new();

    output.push_agent_progress(
        "agent-1",
        tool_calls_event(1, vec![call("1", "Read", "old.rs")]),
    );
    output.push_agent_progress(
        "agent-1",
        tool_calls_event(
            2,
            vec![
                call("2", "Read", "new.rs"),
                call("3", "Grep", "\"needle\" in src"),
            ],
        ),
    );

    let matching = output
        .lines
        .iter()
        .filter(|line| line.tool_id.as_deref() == Some("agent-1"))
        .map(|line| line.content.as_str())
        .collect::<Vec<_>>();

    assert_eq!(matching, vec!["  ↳ Read: new.rs | Grep: \"needle\" in src"]);
}

#[test]
fn test_push_agent_progress_keeps_different_agent_tool_calls_separate() {
    let mut output = OutputArea::new();

    output.push_agent_progress(
        "agent-1",
        tool_calls_event(1, vec![call("1", "Read", "a.rs")]),
    );
    output.push_agent_progress(
        "agent-2",
        tool_calls_event(1, vec![call("2", "Bash", "cargo check")]),
    );

    let matching = output
        .lines
        .iter()
        .filter(|line| line.tool_id.as_deref().is_some())
        .map(|line| line.content.as_str())
        .collect::<Vec<_>>();

    assert_eq!(matching, vec!["  ↳ Read: a.rs", "  ↳ Bash: cargo check"]);
}

#[test]
fn test_push_agent_progress_groups_duplicate_tools_without_showing_turn() {
    let mut output = OutputArea::new();

    output.push_agent_progress(
        "agent-1",
        tool_calls_event(
            7,
            vec![
                call("1", "Read", "a.rs"),
                call("2", "Read", "b.rs"),
                call("3", "Read", "c.rs"),
                call("4", "Read", "d.rs"),
            ],
        ),
    );

    let matching = output
        .lines
        .iter()
        .filter(|line| line.tool_id.as_deref() == Some("agent-1"))
        .map(|line| line.content.as_str())
        .collect::<Vec<_>>();

    assert_eq!(matching, vec!["  ↳ Read ×4: a.rs, b.rs, c.rs +1 more"]);
}

#[test]
fn test_push_agent_progress_appends_message_events() {
    let mut output = OutputArea::new();

    output.push_agent_progress("agent-1", message_event(1, "plain progress"));
    output.push_agent_progress("agent-1", message_event(2, "another progress"));

    let matching = output
        .lines
        .iter()
        .filter(|line| line.tool_id.as_deref() == Some("agent-1"))
        .map(|line| line.content.as_str())
        .collect::<Vec<_>>();

    assert_eq!(matching, vec!["  ↳ plain progress", "  ↳ another progress"]);
}

fn tool_calls_event(
    sequence: usize,
    calls: Vec<AgentToolCallProgressView>,
) -> AgentProgressEventView {
    AgentProgressEventView {
        sequence,
        kind: AgentProgressKindView::ToolCalls { calls },
    }
}

fn message_event(sequence: usize, text: &str) -> AgentProgressEventView {
    AgentProgressEventView {
        sequence,
        kind: AgentProgressKindView::Message {
            text: text.to_string(),
        },
    }
}

fn call(id: &str, name: &str, summary: &str) -> AgentToolCallProgressView {
    AgentToolCallProgressView {
        id: id.to_string(),
        name: name.to_string(),
        input: serde_json::json!({}),
        summary: summary.to_string(),
    }
}
