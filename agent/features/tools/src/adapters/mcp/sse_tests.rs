use super::*;
use crate::adapters::mcp::sse_stream::try_parse_incomplete_event;

#[test]
fn test_parse_sse_events_single() {
    let raw = "event: endpoint\ndata: /message?sessionId=abc\n\n";
    let events = parse_sse_events(raw);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "endpoint");
    assert_eq!(events[0].data, "/message?sessionId=abc");
}

#[test]
fn test_parse_sse_events_multiple() {
    let raw = "event: endpoint\ndata: /msg\n\nevent: message\ndata: hello\n\n";
    let events = parse_sse_events(raw);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, "endpoint");
    assert_eq!(events[1].event_type, "message");
}

#[test]
fn test_parse_sse_events_default_type() {
    let raw = "data: just_data\n\n";
    let events = parse_sse_events(raw);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "message");
    assert_eq!(events[0].data, "just_data");
}

#[test]
fn test_parse_sse_events_multiline_data() {
    let raw = "data: line1\ndata: line2\n\n";
    let events = parse_sse_events(raw);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "line1\nline2");
}

#[test]
fn test_parse_sse_events_empty() {
    assert!(parse_sse_events("").is_empty());
    assert!(parse_sse_events("\n\n").is_empty());
}

#[test]
fn test_resolve_endpoint_url_relative() {
    let base = "https://api.example.com/sse?token=abc";
    let result = resolve_endpoint_url(base, "/message?sessionId=123").unwrap();
    assert_eq!(result, "https://api.example.com/message?sessionId=123");
}

#[test]
fn test_resolve_endpoint_url_absolute() {
    let base = "https://api.example.com/sse";
    let result = resolve_endpoint_url(base, "https://other.example.com/msg").unwrap();
    assert_eq!(result, "https://other.example.com/msg");
}

#[test]
fn test_resolve_endpoint_url_invalid_base() {
    assert!(resolve_endpoint_url("not a url", "/path").is_err());
}

#[test]
fn test_try_extract_response_matching() {
    let data = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#;
    let result = try_extract_response(data, 1).unwrap();
    assert!(result.is_some());
    assert!(result.unwrap().get("tools").is_some());
}

#[test]
fn test_try_extract_response_wrong_id() {
    // id > expected_id should be rejected
    let data = r#"{"jsonrpc":"2.0","id":5,"result":{}}"#;
    let result = try_extract_response(data, 2).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_try_extract_response_stale_id_accepted() {
    // id < expected_id (stale from previous attempt) should be accepted
    let data = r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[]}}"#;
    let result = try_extract_response(data, 3).unwrap();
    assert!(result.is_some());
}

#[test]
fn test_try_extract_response_error() {
    let data = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"bad"}}"#;
    let result = try_extract_response(data, 1);
    assert!(result.is_err());
}

#[test]
fn test_try_extract_response_not_json() {
    let result = try_extract_response("not json", 1).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_try_parse_incomplete_event_valid_json() {
    // SSE event without trailing \n\n but with valid JSON
    let buffer = "event:message\ndata:{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}";
    let event = try_parse_incomplete_event(buffer).unwrap();
    assert_eq!(event.event_type, "message");
    assert!(event.data.contains("\"id\":1"));
}

#[test]
fn test_try_parse_incomplete_event_invalid_json() {
    // SSE event with truncated JSON
    let buffer = "event:message\ndata:{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[{\"na";
    assert!(try_parse_incomplete_event(buffer).is_none());
}

#[test]
fn test_try_parse_incomplete_event_no_data() {
    assert!(try_parse_incomplete_event("event:message\n").is_none());
}
