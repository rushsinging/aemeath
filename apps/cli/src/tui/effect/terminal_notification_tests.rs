use super::{format_osc777_notification, write_terminal_notification};

#[test]
fn osc777_notification_formats_title_body_with_bel_terminator() {
    let sequence = format_osc777_notification("aemeath", "Turn complete");
    assert_eq!(sequence, "\x1b]777;notify;aemeath;Turn complete\x07");
}

#[test]
fn osc777_notification_strips_control_chars_from_title_and_body() {
    let sequence = format_osc777_notification("a\x07emeath", "done\x1b[31m\nnow");
    assert_eq!(sequence, "\x1b]777;notify;aemeath;done[31mnow\x07");
}

#[test]
fn osc777_notification_replaces_field_separator_in_title() {
    // 标题字段以 `;` 分隔，标题内的 `;` 必须替换，否则正文被吞进标题。
    let sequence = format_osc777_notification("a;b", "body");
    assert_eq!(sequence, "\x1b]777;notify;a:b;body\x07");
}

#[test]
fn write_terminal_notification_writes_exact_bytes_to_writer() {
    let mut written = Vec::new();
    write_terminal_notification(&mut written, "aemeath", "Turn complete")
        .expect("write to in-memory writer");
    assert_eq!(written, b"\x1b]777;notify;aemeath;Turn complete\x07");
}
