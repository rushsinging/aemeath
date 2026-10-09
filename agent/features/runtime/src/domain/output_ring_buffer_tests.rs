use super::*;

#[test]
fn append_and_read_tail_returns_recent_bytes_with_cursor() {
    let mut buffer = OutputRingBuffer::new(1024);
    buffer.append(b"line-1\nline-2\n");

    let (text, cursor) = buffer.read_tail_text(1024);
    assert_eq!(text, "line-1\nline-2\n");
    assert_eq!(cursor, buffer.total_written());
}

#[test]
fn append_beyond_capacity_drops_oldest_bytes() {
    let mut buffer = OutputRingBuffer::new(10);
    buffer.append(b"0123456789");
    buffer.append(b"ABCDEF");

    assert_eq!(buffer.total_written(), 16);
    let (text, _) = buffer.read_tail_text(1024);
    assert_eq!(text, "6789ABCDEF", "超出容量应保留最新字节（上限为容量）");
}

#[test]
fn read_tail_is_non_consumptive_and_idempotent() {
    let mut buffer = OutputRingBuffer::new(64);
    buffer.append(b"hello background processes");

    let first = buffer.read_tail_text(1024);
    let second = buffer.read_tail_text(1024);
    assert_eq!(first, second, "多次读取幂等，不消耗数据");

    buffer.append(b" more");
    let (text, _) = buffer.read_tail_text(1024);
    assert_eq!(
        text, "hello background processes more",
        "读取不影响后续写入"
    );
}

#[test]
fn read_from_cursor_returns_only_new_bytes() {
    let mut buffer = OutputRingBuffer::new(1024);
    buffer.append(b"first-chunk;");
    let (_, cursor_after_first) = buffer.read_tail_text(1024);
    buffer.append(b"second-chunk");

    let (text, cursor_after_second) = buffer.read_from_text(cursor_after_first, 1024);
    assert_eq!(text, "second-chunk", "增量游标只返回新增");
    assert_eq!(cursor_after_second, buffer.total_written());
}

#[test]
fn stale_cursor_clamps_to_available_window() {
    let mut buffer = OutputRingBuffer::new(8);
    buffer.append(b"0123456789"); // 头部 "01" 已被覆盖
    let stale_cursor = 0;

    let (text, _) = buffer.read_from_text(stale_cursor, 1024);
    assert_eq!(text, "23456789", "过期游标应 clamp 到当前可用最老数据");
}

#[test]
fn zero_max_bytes_returns_empty_and_advances_cursor() {
    let mut buffer = OutputRingBuffer::new(64);
    buffer.append(b"data");
    let (text, cursor) = buffer.read_from_text(0, 0);
    assert_eq!(text, "");
    assert_eq!(cursor, 0);
}

#[test]
fn utf8_multibyte_cut_renders_lossy_without_panicking() {
    let mut buffer = OutputRingBuffer::new(9);
    buffer.append("中文输出测试".as_bytes()); // 每字 3 字节，容量 9 = 3 字

    let (text, _) = buffer.read_tail_text(8); // 从多字节字符中间切割
    assert!(
        text.contains('测') && text.contains('试'),
        "切割后仍应保留完整汉字：{text:?}"
    );
    assert!(
        text.chars().count() <= 4,
        "lossy 渲染字符数不得超过可用字节数：{text:?}"
    );
}
