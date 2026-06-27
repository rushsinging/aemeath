use super::*;

#[test]
fn test_insert_text_advances_cursor() {
    let mut doc = InputDocument::default();
    doc.insert_text("abc");
    assert_eq!(doc.buffer, "abc");
    assert_eq!(doc.cursor, 3);
}

#[test]
fn test_move_cursor_clamps_to_buffer() {
    let mut doc = InputDocument::default();
    doc.insert_text("abc");
    doc.move_cursor(99);
    assert_eq!(doc.cursor, 3);
    doc.move_cursor(0);
    assert_eq!(doc.cursor, 0);
}

#[test]
fn test_delete_backward_at_start_is_noop() {
    let mut doc = InputDocument::default();
    doc.delete_backward();
    assert_eq!(doc.buffer, "");
    assert_eq!(doc.cursor, 0);
}

// 回归测试：Bug #77 — @ 补全后按空格回退删除字符
//
// 场景：用户输入 @src/m 触发补全，Tab 确认后 textarea 变为 @src/main.rs，
// 但模型未同步（仍是 @src/m）。此时按空格，模型 insert(' ') 生成 @src/m ，
// TextChanged → set_text 用旧文本覆盖 textarea 的正确内容。
//
// 修复：补全确认后执行 clear + insert_text 将模型同步到 textarea 的文本。

/// 模拟补全后模型未同步 → 按空格 → 旧文本覆盖 textarea 的 bug 场景
#[test]
fn test_stale_model_overwrites_textarea_after_completion() {
    // Step 1: 用户输入 @src/m → 模型 = @src/m
    let mut doc = InputDocument::default();
    doc.insert_text("@src/m");
    assert_eq!(doc.buffer, "@src/m");
    assert_eq!(doc.cursor, 6);

    // Step 2: 用户 Tab 补全 → textarea 变为 @src/main.rs
    //         但模型未同步（保持 @src/m），这是 bug 条件
    //         模拟：textarea = @src/main.rs，模型仍是 @src/m

    // Step 3: 用户按空格 → 模型 insert(' ')，产生 @src/m ，
    //         TextChanged + set_text 用 @src/m  覆盖 textarea 的 @src/main.rs
    doc.insert_text(" ");
    // BUG：模型生成 @src/m ，set_text 会覆盖 textarea 上的 @src/main.rs
    assert_eq!(doc.buffer, "@src/m ");
    // 修复前的错误行为验证：buffer 长度 7 而非 @src/main.rs  的 13
    assert_ne!(doc.buffer, "@src/main.rs ");
}

/// 修复后：补全确认后模型同步 → 按空格 → 正确追加
#[test]
fn test_synced_model_after_completion_preserves_text() {
    let mut doc = InputDocument::default();
    // 用户输入 @src/m
    doc.insert_text("@src/m");
    assert_eq!(doc.buffer, "@src/m");

    // 补全确认后 textarea 变为 @src/main.rs，模型同步：
    doc.clear();
    doc.insert_text("@src/main.rs");
    assert_eq!(doc.buffer, "@src/main.rs");
    assert_eq!(doc.cursor, 12);

    // 按空格：模型在正确位置插入空格
    doc.insert_text(" ");
    assert_eq!(doc.buffer, "@src/main.rs ");
    assert_eq!(doc.cursor, 13);
}

/// CJK 路径补全后模型同步 + 按空格
#[test]
fn test_synced_model_after_cjk_completion_preserves_text() {
    let mut doc = InputDocument::default();
    // 用户输入 @中文/路径
    doc.insert_text("@中文/路径");
    assert_eq!(doc.buffer, "@中文/路径");

    // 补全确认后 textarea 变为 @中文/路径/目标.rs，模型同步：
    doc.clear();
    doc.insert_text("@中文/路径/目标.rs");
    assert_eq!(doc.buffer, "@中文/路径/目标.rs");
    assert_eq!(doc.cursor, "@中文/路径/目标.rs".len());

    // 按空格
    doc.insert_text(" ");
    assert_eq!(doc.buffer, "@中文/路径/目标.rs ");
    assert_eq!(doc.cursor, "@中文/路径/目标.rs ".len());
}

// 回归测试：Bug #78 — 粘贴后按空格清空粘贴内容
//
// 场景：用户粘贴文本，input_area.input(ch) 直接修改 textarea 但未同步模型。
// 按空格时模型 insert(' ') 产生旧文本+空格 → set_text 覆盖 textarea 中的粘贴内容。
//
// 修复：粘贴循环后执行 clear + insert_text 将模型同步到 textarea 的文本。

/// 粘贴后模型同步 → 按空格 → 正确追加
#[test]
fn test_synced_model_after_paste_preserves_text() {
    let mut doc = InputDocument::default();
    // 粘贴前状态（模型）
    doc.insert_text("before ");
    assert_eq!(doc.buffer, "before ");

    // 用户粘贴 "hello world" → textarea = "before hello world"
    // 修复：粘贴后同步模型
    doc.clear();
    doc.insert_text("before hello world");
    assert_eq!(doc.buffer, "before hello world");

    // 按空格
    doc.insert_text(" ");
    assert_eq!(doc.buffer, "before hello world ");
    assert_eq!(doc.cursor, "before hello world ".len());
}

/// 粘贴 Cjk 文本后模型同步 + 按空格
#[test]
fn test_synced_model_after_cjk_paste_preserves_text() {
    let mut doc = InputDocument::default();
    // 粘贴前
    doc.insert_text("前置");
    assert_eq!(doc.buffer, "前置");

    // 用户粘贴 "你好世界" → textarea = "前置你好世界"
    doc.clear();
    doc.insert_text("前置你好世界");
    assert_eq!(doc.buffer, "前置你好世界");

    // 按空格
    doc.insert_text(" ");
    assert_eq!(doc.buffer, "前置你好世界 ");
}

// 回归测试：Bug #79 — Option+Enter 换行后继续输入回到上一行
//
// 场景：用户 Option+Enter 在 textarea 中插入换行，但 textarea.enter(true)
// 直接修改了 textarea 未同步模型。模型仍是单行旧文本 — 继续输入时
// model.insert → TextChanged → set_text 用旧文本覆盖 textarea，换行丢失。
//
// 修复：enter(true) 后执行 clear + insert_text 同步模型。

/// 换行后模型同步 → 继续输入 → 保持在第二行
#[test]
fn test_synced_model_after_newline_preserves_text() {
    let mut doc = InputDocument::default();
    // 用户在第一行输入
    doc.insert_text("第一行文字");
    assert_eq!(doc.buffer, "第一行文字");

    // 用户 Option+Enter 换行 → textarea = "第一行文字\n"
    // 修复：换行后同步模型
    doc.clear();
    doc.insert_text("第一行文字\n");
    assert_eq!(doc.buffer, "第一行文字\n");

    // 用户在第二行继续输入
    doc.insert_text("第二行");
    assert_eq!(doc.buffer, "第一行文字\n第二行");
}

/// Cjk 换行后模型同步 → 继续输入 → 正确
#[test]
fn test_synced_model_after_newline_cjk_preserves_text() {
    let mut doc = InputDocument::default();
    doc.insert_text("中文内容");
    assert_eq!(doc.buffer, "中文内容");

    // Option+Enter 换行
    doc.clear();
    doc.insert_text("中文内容\n");
    assert_eq!(doc.buffer, "中文内容\n");

    // 继续输入
    doc.insert_text("继续输入");
    assert_eq!(doc.buffer, "中文内容\n继续输入");
}

// 回归测试：光标移动未同步模型 cursor — InsertChar 以旧位置插入
//
// 场景：Ctrl+A/E/Left/Right/End 移动 textarea 光标，但模型 cursor 未更新。
// 后续 InsertChar 以模型旧光标位置插入，造成错位。
// 这在 CJK 文本中尤为明显（字符索引 ≠ 字节位置）。
//
// 修复：set_cursor_col 将 textarea 的字符索引光标转换为模型字节位置

#[test]
fn test_set_cursor_col_sets_correct_byte_position() {
    let mut doc = InputDocument::default();
    doc.insert_text("abc你好def");
    assert_eq!(doc.buffer, "abc你好def");
    assert_eq!(doc.cursor, "abc你好def".len());

    // textarea 光标移到字符索引 3（'你' 的位置）
    doc.set_cursor_col(3);
    // 字节位置：'a'(1) + 'b'(1) + 'c'(1) = 3
    assert_eq!(doc.cursor, 3);

    // textarea 光标移到字符索引 4（'好' 的位置）
    doc.set_cursor_col(4);
    // 字节位置：3 + '你'(3) = 6
    assert_eq!(doc.cursor, 6);
}

#[test]
fn test_insert_at_synced_cursor_after_move() {
    let mut doc = InputDocument::default();
    doc.insert_text("abc你好def");
    assert_eq!(doc.cursor, "abc你好def".len());

    // 模拟光标移动到 start (Ctrl+A)
    doc.set_cursor_col(0);
    assert_eq!(doc.cursor, 0);

    // 在行首插入
    doc.insert_text(">>");
    assert_eq!(doc.buffer, ">>abc你好def");
    assert_eq!(doc.cursor, ">>".len());
}

#[test]
fn test_insert_at_cjk_boundary_after_cursor_sync() {
    let mut doc = InputDocument::default();
    doc.insert_text("你好世界");

    // 光标移到字符索引 2 (在 '你好' 之后，'世界' 之前)
    doc.set_cursor_col(2);
    assert_eq!(doc.cursor, "你好".len());

    // 插入
    doc.insert_text("的");
    assert_eq!(doc.buffer, "你好的世界");
    assert_eq!(doc.cursor, "你好的".len());
}

// 回归测试：Bug #99 — 多行输入时光标上下移动
//
// 场景：用户在多行文本中按 ↑/↓，期望光标在行间移动，
// 但旧逻辑始终触发历史翻看。修复后 InputDocument 支持
// move_up/move_down，在边界行时由 InputModel 切换到历史导航。

#[test]
fn test_move_up_goes_to_previous_line() {
    let mut doc = InputDocument::default();
    doc.insert_text("第一行\n第二行\n第三行");
    // 光标在第三行末尾
    assert_eq!(doc.cursor_row(), 2);
    doc.move_up();
    assert_eq!(doc.cursor_row(), 1, "应移到第二行");
    doc.move_up();
    assert_eq!(doc.cursor_row(), 0, "应移到第一行");
    // 已在第一行，再次 move_up 不动
    doc.move_up();
    assert_eq!(doc.cursor_row(), 0, "已在第一行，不应移动");
}

#[test]
fn test_move_down_goes_to_next_line() {
    let mut doc = InputDocument::default();
    doc.insert_text("第一行\n第二行\n第三行");
    doc.move_cursor(0); // 光标移到第一行开头
    assert_eq!(doc.cursor_row(), 0);
    doc.move_down();
    assert_eq!(doc.cursor_row(), 1, "应移到第二行");
    doc.move_down();
    assert_eq!(doc.cursor_row(), 2, "应移到第三行");
    // 已在最后一行，再次 move_down 不动
    doc.move_down();
    assert_eq!(doc.cursor_row(), 2, "已在最后一行，不应移动");
}

#[test]
fn test_move_up_down_preserves_column_position() {
    let mut doc = InputDocument::default();
    doc.insert_text("abcde\nxyz");
    // 光标在第二行末尾（xyz 之后）
    // 移到第二行的 'y' 位置（字符列 1）
    doc.set_cursor_col("abcde\n".len() + "x".len());
    assert_eq!(doc.cursor_col(), 1);
    doc.move_up();
    assert_eq!(doc.cursor_row(), 0, "应移到第一行");
    assert_eq!(doc.cursor_col(), 1, "列应保持为 1（即 'b' 的位置）");
    // 验证光标在 'b' 位置
    assert_eq!(doc.cursor, 1);
}

#[test]
fn test_move_up_down_clamps_shorter_line() {
    let mut doc = InputDocument::default();
    doc.insert_text("ab\nxyzw");
    // 光标在第二行的 'w' 之后（列 4）
    doc.move_up();
    assert_eq!(doc.cursor_row(), 0, "应移到第一行");
    // 第一行只有 2 个字符，列 4 应 clamp 到 2
    assert_eq!(doc.cursor_col(), 2, "列应 clamp 到第一行长 2");
    assert_eq!(doc.cursor, 2, "光标应在第一行末尾");
}

#[test]
fn test_move_up_down_single_line_is_noop() {
    let mut doc = InputDocument::default();
    doc.insert_text("单行文本");
    let cursor_before = doc.cursor;
    doc.move_up();
    assert_eq!(doc.cursor, cursor_before, "单行时 move_up 不动");
    doc.move_down();
    assert_eq!(doc.cursor, cursor_before, "单行时 move_down 不动");
}

#[test]
fn test_cursor_row_col_with_cjk() {
    let mut doc = InputDocument::default();
    doc.insert_text("你好\n世界");
    // 光标在末尾（第二行'世界'之后）
    assert_eq!(doc.cursor_row(), 1);
    assert_eq!(doc.cursor_col(), 2, "第二行有 2 个字符");
    doc.move_cursor(0);
    assert_eq!(doc.cursor_row(), 0);
    assert_eq!(doc.cursor_col(), 0);
}

#[test]
fn test_move_up_down_with_cjk() {
    let mut doc = InputDocument::default();
    doc.insert_text("你好世界\nabc\n测试");
    // 光标在第三行末尾
    assert_eq!(doc.cursor_row(), 2);
    doc.move_up();
    assert_eq!(doc.cursor_row(), 1);
    doc.move_up();
    assert_eq!(doc.cursor_row(), 0);
    doc.move_down();
    assert_eq!(doc.cursor_row(), 1);
}

#[test]
fn test_is_cursor_at_first_last_line() {
    let mut doc = InputDocument::default();
    doc.insert_text("第一行\n第二行\n第三行");
    assert!(doc.is_cursor_at_last_line());
    assert!(!doc.is_cursor_at_first_line());
    doc.move_cursor(0);
    assert!(doc.is_cursor_at_first_line());
    assert!(!doc.is_cursor_at_last_line());
    // 移到第二行开头（第一行末尾的 \n 之后）
    let second_line_start = "第一行\n".len();
    doc.move_cursor(second_line_start);
    assert!(!doc.is_cursor_at_first_line());
    assert!(!doc.is_cursor_at_last_line());
}

// === ImageSpan 测试 ===

fn make_test_image(size: usize) -> sdk::ClipboardImageView {
    sdk::ClipboardImageView {
        base64: "img".to_string(),
        media_type: "image/png".to_string(),
        final_size: size,
        display_path: None,
        width: None,
        height: None,
    }
}

#[test]
fn test_insert_image_adds_placeholder_and_span() {
    let mut doc = InputDocument::default();
    doc.insert_text("hello ");
    doc.insert_image(make_test_image(100));
    assert_eq!(doc.buffer, "hello [Image #1]");
    assert_eq!(doc.cursor, "hello [Image #1]".len());
    assert_eq!(doc.image_spans.len(), 1);
    assert_eq!(doc.image_spans[0].index, 1);
}

#[test]
fn test_insert_multiple_images_assigns_sequential_index() {
    let mut doc = InputDocument::default();
    doc.insert_image(make_test_image(10));
    doc.insert_text(" ");
    doc.insert_image(make_test_image(20));
    assert_eq!(doc.buffer, "[Image #1] [Image #2]");
    assert_eq!(doc.image_spans.len(), 2);
}

#[test]
fn test_delete_backward_atomically_removes_image() {
    let mut doc = InputDocument::default();
    doc.insert_image(make_test_image(10));
    // cursor 在占位符末尾
    assert_eq!(doc.cursor, "[Image #1]".len());
    doc.delete_backward();
    assert_eq!(doc.buffer, "");
    assert!(doc.image_spans.is_empty());
}

#[test]
fn test_delete_backward_removes_image_in_middle() {
    let mut doc = InputDocument::default();
    doc.insert_text("a");
    doc.insert_image(make_test_image(10));
    doc.insert_text("b");
    // a[Image #1]b, cursor 在末尾 'b' 之后
    // 移到占位符末尾（'b' 之前）
    doc.move_cursor("a[Image #1]".len());
    doc.delete_backward();
    assert_eq!(doc.buffer, "ab");
    assert!(doc.image_spans.is_empty());
}

#[test]
fn test_delete_image_preserves_index_hole() {
    let mut doc = InputDocument::default();
    doc.insert_image(make_test_image(10)); // #1
    doc.insert_image(make_test_image(20)); // #2
    doc.insert_image(make_test_image(30)); // #3
                                           // buffer = "[Image #1][Image #2][Image #3]"
                                           // 删除 #2（中间）
    doc.move_cursor("[Image #1]".len());
    doc.delete_backward(); // 删除 #1... 不对，这里删除的是光标前的 span
                           // 重新设计：移动光标到 #2 末尾后 delete_backward
    let mut doc = InputDocument::default();
    doc.insert_image(make_test_image(10));
    doc.insert_image(make_test_image(20));
    doc.insert_image(make_test_image(30));
    // 光标在 #2 末尾（#3 之前）
    let pos = "[Image #1][Image #2]".len();
    doc.move_cursor(pos);
    doc.delete_backward(); // 原子删除 #2
    assert_eq!(doc.buffer, "[Image #1][Image #3]");
    assert_eq!(doc.image_spans.len(), 2);
    // 编号保留原始 index
    assert_eq!(doc.image_spans[0].index, 1);
    assert_eq!(doc.image_spans[1].index, 3);
}

/// #507 修复：`submit_text()` 现**保留** image placeholder（而非剔除为空），
/// 以便 runtime 端 `Message::user_with_images` 能按占位符穿插拆块。
#[test]
fn test_submit_text_preserves_image_placeholders() {
    let mut doc = InputDocument::default();
    doc.insert_text("look at ");
    doc.insert_image(make_test_image(10));
    doc.insert_text(" this");
    assert_eq!(doc.submit_text(), "look at [Image #1] this");
}

/// #507 修复：copied text 还原 + image placeholder 保留的组合行为。
#[test]
fn test_submit_text_combined_copied_text_and_image_placeholder() {
    let mut doc = InputDocument::default();
    doc.insert_text("see ");
    doc.insert_pasted_text("a\nb\nc\nd"); // 变成 [Copied 4 lines]
    doc.insert_text(" and ");
    doc.insert_image(make_test_image(10));
    let text = doc.submit_text();
    assert!(
        text.contains("see a\nb\nc\nd and"),
        "copied text 应展开为原文：got={text:?}"
    );
    assert!(
        text.contains("[Image #1]"),
        "image placeholder 应保留：got={text:?}"
    );
    assert!(
        !text.contains("[Copied"),
        "copied text 不应在 submit 输出：got={text:?}"
    );
}

#[test]
fn test_drain_images_returns_in_order() {
    let mut doc = InputDocument::default();
    doc.insert_image(make_test_image(10));
    doc.insert_text(" mid ");
    doc.insert_image(make_test_image(20));
    let images = doc.drain_images();
    assert_eq!(images.len(), 2);
    // 按文档中插入顺序返回 (placeholder, image) 配对
    assert_eq!(images[0].0, "[Image #1]");
    assert_eq!(images[0].1.media_type, "image/png"); // make_test_image 固定 png
    assert_eq!(images[1].0, "[Image #2]");
    assert!(doc.image_spans.is_empty());
}

/// 原 `test_submit_text_expands_copied_text_and_strips_images` 已删除，
/// 由 line 459 `test_submit_text_combined_copied_text_and_image_placeholder` 覆盖。

#[test]
fn test_clear_removes_image_spans() {
    let mut doc = InputDocument::default();
    doc.insert_image(make_test_image(10));
    doc.clear();
    assert!(doc.image_spans.is_empty());
    assert!(doc.buffer.is_empty());
}
