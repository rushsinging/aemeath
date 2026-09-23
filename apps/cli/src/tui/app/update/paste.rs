use super::UpdateResult;
use crate::tui::app::App;
use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::intent::{ConversationIntent, InsertAskUserChatText};
use crate::tui::model::input::intent::InputIntent;
use crate::tui::update::intent::AgentIntent;

/// 粘贴投递目标——由当前焦点唯一决定，`route_paste` 与 AskUser 按键路径共用
/// 这一决策，避免新增交互态时粘贴落入错误的输入目标。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PasteTarget {
    /// AskUser Type something 子态：插入自由输入框。
    AskUserFreeInput,
    /// AskUser 交互活动中但非自由输入子态：忽略粘贴，不污染任何输入区。
    AskUserActive,
    /// 无 AskUser 交互：走主输入区既有粘贴管线。
    MainInput,
}

impl App {
    /// 当前焦点对应的粘贴投递目标（唯一决策入口）。
    pub(crate) fn paste_target(&self) -> PasteTarget {
        match self.model.conversation.ask_user_snapshot() {
            Some(snapshot) if snapshot.chat_input_active => PasteTarget::AskUserFreeInput,
            Some(_) => PasteTarget::AskUserActive,
            None => PasteTarget::MainInput,
        }
    }

    /// 统一粘贴分派，空闲与处理中共用同一条判定。
    ///
    /// 目标决策见 [`Self::paste_target`]：
    /// - AskUser 自由输入子态 → 文本插入自由输入框（子态不处理图片）；
    /// - AskUser 活动但非子态 → 忽略；
    /// - 主输入区 → 空白粘贴读图、本地图片加载、其余文本插入输入区。
    pub(crate) fn route_paste(&mut self, text: String) -> UpdateResult {
        self.input.just_pasted = true;
        match self.paste_target() {
            PasteTarget::AskUserFreeInput => {
                crate::tui::log_debug!(
                    "paste target=ask_user_free_input chars={}",
                    text.chars().count()
                );
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::InsertAskUserChatText(InsertAskUserChatText { text }),
                ));
                UpdateResult::none()
            }
            PasteTarget::AskUserActive => {
                crate::tui::log_debug!(
                    "paste target=ask_user_active ignored chars={}",
                    text.chars().count()
                );
                UpdateResult::none()
            }
            PasteTarget::MainInput => match sdk::classify_paste(&text) {
                sdk::PasteKind::Empty => UpdateResult::one(Effect::ReadClipboardImage),
                sdk::PasteKind::LocalImageFile(path) => {
                    UpdateResult::one(Effect::ProcessImageFile {
                        path: path.to_string_lossy().into_owned(),
                        fallback_text: text,
                    })
                }
                sdk::PasteKind::Text => {
                    self.apply_pasted_text(text);
                    UpdateResult::none()
                }
            },
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
