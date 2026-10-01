use super::UpdateResult;
use crate::tui::app::App;
use crate::tui::effect::effect::Effect;
use crate::tui::model::input::submission::InputSubmission;

impl App {
    /// Handle Enter when not processing.
    ///
    /// 在常驻 chat() 模型下（#390 A1），首条提交不再 spawn 新 chat，而是与「忙时」
    /// 提交统一经 input_events 通道发往常驻 loop（`submit_user_input_event`）。
    /// slash 命令同步分发（`handle_slash_command`），永不作为 user message。
    pub(super) fn update_enter(&mut self) -> UpdateResult {
        let Some(submission) = self.submit_input_intent() else {
            return UpdateResult::none();
        };
        if submission.text.is_empty() && submission.images.is_empty() {
            return UpdateResult::none();
        }
        if submission.text.starts_with('/') {
            // slash 命令同步分发：纯 update 产出 Effect，由 run_loop 统一执行。
            return self.handle_slash_command(&submission.text);
        }

        // 首条（非忙）提交：进入 Thinking 态、给出即时反馈，再统一经事件通道提交。
        self.chat.clear_tool_activity();
        self.chat.start_processing();
        self.submit_user_input_event(submission)
    }

    /// 统一提交入口：把一次用户提交转成 `ChatInputEvent::UserMessage` 发往常驻 loop。
    ///
    /// 非忙（首条）与忙时（插话）提交共用本路径——回显交由 runtime 的 MessagesSync
    /// 单一真相驱动（A1 不动回显机制），此处只入队「排队中」占位并发送事件。
    pub(super) fn submit_user_input_event(&mut self, submission: InputSubmission) -> UpdateResult {
        // 图片携带 base64 数据（含内联/粘贴图，display_path 可能为 None）经事件通道送达
        // runtime；submission.images 保留 placeholder id（#fix-tui-image-input-output）
        // — runtime 按 text 中 `[Image #N]` 出现顺序穿插拆 image block，image 仍按
        // TUI 端出现顺序（drain_images 已按 span.start 排好）。
        let images: Vec<sdk::ChatInputImage> = submission
            .images
            .into_iter()
            .map(|(placeholder, image)| sdk::ChatInputImage {
                id: placeholder,
                base64: image.base64,
                media_type: image.media_type,
            })
            .collect();
        // 生成一次 InputId，同时用于事件 id 与占位块 input_id——两者必须相同（#390 A3）。
        let input_id = sdk::InputId::new_v7();
        let text_len = submission.text.chars().count();
        let image_count = images.len();
        crate::tui::log_debug!(
            "submit_user_input_event input_id={} text_len={} image_count={} is_processing={}",
            input_id,
            text_len,
            image_count,
            self.chat.is_processing
        );
        let event = sdk::ChatInputEvent::UserMessage {
            id: input_id.clone(),
            text: submission.text.clone(),
            images,
        };
        // 入队即时显示「排队中」块（QueuedUserMessage，携 input_id），由归宿事件
        // UserMessagesAdopted 按 id 清除（#390 A3）。submission.text 仅经事件通道送达 runtime。
        self.enqueue_submission_echo(input_id, submission.display_text);
        UpdateResult::one(Effect::SendChatInputEvent { event })
    }
}

#[cfg(test)]
#[path = "enter_tests.rs"]
mod tests;
