use super::UpdateResult;
use crate::tui::app::App;
use crate::tui::effect::effect::Effect;
use crate::tui::model::input::intent::InputIntent;

impl App {
    /// 统一粘贴分派，空闲与处理中共用同一条判定。
    ///
    /// - 空白粘贴 → 读取剪贴板图片；
    /// - 本地图片引用（含 `file://` URL 与终端转义路径）→ 加载图片；
    /// - 其余文本（含 `http(s)`、`data:` 远端图片链接）→ 插入输入区。
    pub(crate) fn route_paste(&mut self, text: String) -> UpdateResult {
        self.input.just_pasted = true;
        match sdk::classify_paste(&text) {
            sdk::PasteKind::Empty => UpdateResult::one(Effect::ReadClipboardImage),
            sdk::PasteKind::LocalImageFile(path) => UpdateResult::one(Effect::ProcessImageFile {
                path: path.to_string_lossy().into_owned(),
                fallback_text: text,
            }),
            sdk::PasteKind::Text => {
                self.apply_pasted_text(text);
                UpdateResult::none()
            }
        }
    }

    /// 把粘贴文本插入输入区：空闲时按粘贴折叠，处理中保留原文以便排队阅读。
    pub(crate) fn apply_pasted_text(&mut self, text: String) {
        let intent = if self.chat.is_processing {
            InputIntent::InsertText(text)
        } else {
            InputIntent::InsertPastedText(text)
        };
        self.handle_input_intent(intent);
        self.update_suggestions();
    }
}
