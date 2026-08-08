use super::*;

#[test]
fn progress_line_buffer_handles_newline_and_marker_across_chunk_boundaries() {
    let mut buffer = ProgressLineBuffer::new(CWD_MARKER);
    let mut events = Vec::new();

    for chunk in [
        "progress_stream_test_marker",
        "\n",
        "\n__AEM",
        "EATH_CWD__=/tmp\n",
    ] {
        events.extend(buffer.push(chunk));
    }
    events.extend(buffer.finish());

    assert_eq!(events, vec!["progress_stream_test_marker\n"]);
}

#[test]
fn progress_line_buffer_flushes_unterminated_tail_once() {
    let mut buffer = ProgressLineBuffer::new(CWD_MARKER);
    let mut events = buffer.push("tail without newline");
    events.extend(buffer.finish());

    assert_eq!(events, vec!["tail without newline"]);
}
