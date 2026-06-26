#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputSubmission {
    pub text: String,
    pub display_text: String,
    /// (placeholder_id, ChatInputImage) 配对：placeholder_id 是 TUI 端
    /// `ImageSpan::placeholder()` 生成的字符串（如 `"[Image #1]"`），
    /// 落入 text 的占位符位置；runtime 端按 text 中 `[Image #N]` 出现顺序
    /// 穿插组装 image block（#fix-tui-image-input-output）。
    pub images: Vec<(String, sdk::ChatInputImage)>,
}