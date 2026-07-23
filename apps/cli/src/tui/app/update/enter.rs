use super::UpdateResult;
use crate::tui::app::App;
use crate::tui::effect::effect::Effect;
use crate::tui::model::input::submission::InputSubmission;

impl App {
    /// Handle Enter when not processing.
    ///
    /// 在常驻 chat() 模型下（#390 A1），首条提交不再 spawn 新 chat，而是与「忙时」
    /// 提交统一经 input_events 通道发往常驻 loop（`submit_user_input_event`）。
    /// 仅 slash 命令仍走 `pending_slash` 单独处理（slash 永不作为 user message）。
    pub(super) fn update_enter(&mut self) -> UpdateResult {
        let Some(submission) = self.submit_input_intent() else {
            return UpdateResult::none();
        };
        if submission.text.is_empty() && submission.images.is_empty() {
            return UpdateResult::none();
        }
        if submission.text.starts_with('/') {
            return UpdateResult {
                effects: Vec::new(),
                spawn_effect: None,
                pending_slash: Some(submission.text),
            };
        }

        // 首条（非忙）提交：进入 Thinking 态、给出即时反馈，再统一经事件通道提交。
        self.chat.clear_tool_activity();
        self.spinner_phase(crate::tui::model::conversation::spinner::SpinnerPhase::Thinking);
        self.chat.expect_run_start();
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
mod tests {
    use super::*;
    use crate::tui::model::input::intent::InputIntent;
    use std::path::PathBuf;

    fn test_app() -> App {
        App::new(
            "test-session".to_string(),
            PathBuf::from("/tmp"),
            "test-model".to_string(),
        )
    }

    /// 提取本次提交产生的 `SendChatInputEvent` 的 `UserMessage` 文本（断言辅助）。
    fn sent_user_message_text(result: &UpdateResult) -> Option<&str> {
        result.effects.iter().find_map(|effect| match effect {
            Effect::SendChatInputEvent {
                event: sdk::ChatInputEvent::UserMessage { text, .. },
            } => Some(text.as_str()),
            _ => None,
        })
    }

    /// 提取本次提交产生的 `SendChatInputEvent` 的 `UserMessage` 图片（断言辅助）。
    fn sent_user_message_images(result: &UpdateResult) -> Option<&Vec<sdk::ChatInputImage>> {
        result.effects.iter().find_map(|effect| match effect {
            Effect::SendChatInputEvent {
                event: sdk::ChatInputEvent::UserMessage { images, .. },
            } => Some(images),
            _ => None,
        })
    }

    #[test]
    fn test_update_enter_empty_submission_is_noop() {
        let mut app = test_app();

        let result = app.update_enter();

        assert!(result.effects.is_empty());
        assert!(result.spawn_effect.is_none());
        assert!(result.pending_slash.is_none());
        assert!(!app.chat.is_processing);
    }

    #[test]
    fn test_update_enter_slash_submission_returns_pending_slash() {
        let mut app = test_app();
        app.model
            .input
            .apply(InputIntent::InsertText("/help".to_string()));

        let result = app.update_enter();

        assert_eq!(result.pending_slash.as_deref(), Some("/help"));
        assert!(result.effects.is_empty());
        assert!(result.spawn_effect.is_none());
    }

    /// 非忙（首条）提交不再 spawn 新 chat，而是经事件通道发 `UserMessage`。
    #[test]
    fn test_update_enter_non_busy_routes_user_message_to_input_channel() {
        let mut app = test_app();
        app.model
            .input
            .apply(InputIntent::InsertText("first message".to_string()));

        let result = app.update_enter();

        assert!(
            result.spawn_effect.is_none(),
            "首条提交不应再 spawn 新 chat"
        );
        assert!(result.pending_slash.is_none());
        assert_eq!(
            sent_user_message_text(&result),
            Some("first message"),
            "首条提交应经事件通道发 UserMessage"
        );
        // 首条提交进入 Thinking 处理态（即时反馈）。
        assert!(app.chat.is_processing, "首条提交后应进入 processing 态");
    }

    /// Step 1（A1 Task 6）：非忙与忙时提交走同一路径——都把 `UserMessage`
    /// 发往 input_event_tx 通道（用真实 sender 捕获断言）。
    #[test]
    fn test_submit_routes_user_message_to_input_channel() {
        // 用真实 unbounded sender 充当「fake input_tx」，可断言通道收到的事件。
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<sdk::ChatInputEvent>();

        // --- 非忙（首条）提交 ---
        let mut app_idle = test_app();
        app_idle.chat.input_event_tx = Some(tx.clone());
        app_idle
            .model
            .input
            .apply(InputIntent::InsertText("hello idle".to_string()));
        let idle_result = app_idle.update_enter();
        // update_enter 返回 SendChatInputEvent 描述；执行该 effect 才真正 send。
        assert_eq!(sent_user_message_text(&idle_result), Some("hello idle"));
        send_chat_input_event_for_test(&mut app_idle, &idle_result);
        let idle_event = rx.try_recv().expect("非忙提交应发往 input 通道");
        assert!(matches!(
            idle_event,
            sdk::ChatInputEvent::UserMessage { ref text, .. } if text == "hello idle"
        ));

        // --- 忙时（插话）提交 ---
        let mut app_busy = test_app();
        app_busy.chat.input_event_tx = Some(tx.clone());
        app_busy.chat.start_processing();
        let busy_submission = InputSubmission {
            text: "hello busy".to_string(),
            display_text: "hello busy".to_string(),
            images: Vec::new(),
        };
        let busy_result = app_busy.submit_user_input_event(busy_submission);
        assert_eq!(sent_user_message_text(&busy_result), Some("hello busy"));
        send_chat_input_event_for_test(&mut app_busy, &busy_result);
        let busy_event = rx.try_recv().expect("忙时提交应发往 input 通道");
        assert!(matches!(
            busy_event,
            sdk::ChatInputEvent::UserMessage { ref text, .. } if text == "hello busy"
        ));
    }

    /// #402 回归：内联/粘贴图片（`display_path: None` 但有 base64）必须经事件通道
    /// 携带 base64，而非被 `display_path` filter 丢弃。
    /// #fix-tui-image-input-output：placeholder 由 `ImageSpan::placeholder()` 提供。
    #[test]
    fn test_submit_carries_inline_image_base64() {
        let mut app = test_app();
        let submission = InputSubmission {
            text: "看图".to_string(),
            display_text: "看图".to_string(),
            images: vec![(
                "[Image #1]".to_string(),
                sdk::ChatInputImage {
                    id: String::new(),
                    base64: "aW1nZGF0YQ==".to_string(),
                    media_type: "image/png".to_string(),
                },
            )],
        };

        let result = app.submit_user_input_event(submission);

        let images = sent_user_message_images(&result).expect("应发出带 images 的 UserMessage");
        assert_eq!(images.len(), 1, "内联图片不应被丢弃");
        assert_eq!(images[0].id, "[Image #1]", "应保留 ImageSpan 占位符");
        assert_eq!(images[0].base64, "aW1nZGF0YQ==");
        assert_eq!(images[0].media_type, "image/png");
    }

    #[test]
    fn test_update_enter_copied_text_routes_original_via_event() {
        let mut app = test_app();
        app.model
            .input
            .apply(InputIntent::InsertPastedText("a\nb\nc\nd".to_string()));

        let result = app.update_enter();

        assert!(result.spawn_effect.is_none());
        // 提交原文（非折叠占位符）经事件通道发送。
        assert_eq!(sent_user_message_text(&result), Some("a\nb\nc\nd"));
        // 排队区即时显示折叠占位符（display_text），正式回显由 MessagesSync 驱动。
        let has_queued = app.model.conversation.timeline.items().iter().any(|item| {
            matches!(
                item,
                crate::tui::model::output_timeline::OutputTimelineItem::QueuedUserMessage { text, .. }
                    if text == "[Copied 4 lines]"
            )
        });
        assert!(has_queued, "首条复制文本应以折叠占位符入排队显示");
    }

    /// Step 1（A3 Task 1）：submit 后文本统一走事件通道，effects 含 SendChatInputEvent::UserMessage。
    #[test]
    fn submit_routes_via_event_channel() {
        let mut app = test_app();
        app.model
            .input
            .apply(InputIntent::InsertText("hello queue".to_string()));

        let result = app.update_enter();

        // effects 必须含一个 SendChatInputEvent::UserMessage
        assert_eq!(
            sent_user_message_text(&result),
            Some("hello queue"),
            "effects 应含 SendChatInputEvent::UserMessage"
        );
    }

    /// Step 1（A3 Task 1）：submit 后，占位块携带的 input_id 与事件 id 相同。
    ///
    /// 同一次提交生成一个 InputId，分别写入：
    /// 1. `ChatInputEvent::UserMessage { id }` 事件；
    /// 2. `ConversationBlock::QueuedUserMessage { input_id }` 占位块。
    ///
    /// 两者必须相等——这是 Task 2 按 id 清占位的前提。
    #[test]
    fn submit_placeholder_carries_input_id() {
        let mut app = test_app();
        app.model
            .input
            .apply(InputIntent::InsertText("hello id".to_string()));

        let result = app.update_enter();

        // 取事件里的 id
        let event_id = result.effects.iter().find_map(|effect| match effect {
            Effect::SendChatInputEvent {
                event: sdk::ChatInputEvent::UserMessage { id, .. },
            } => Some(id.clone()),
            _ => None,
        });
        let event_id = event_id.expect("应产生 SendChatInputEvent UserMessage");

        // 取占位块里的 input_id
        let block_input_id = app
            .model
            .conversation
            .timeline
            .items()
            .iter()
            .find_map(|item| match item {
                crate::tui::model::output_timeline::OutputTimelineItem::QueuedUserMessage {
                    input_id,
                    ..
                } => Some(input_id.clone()),
                _ => None,
            });
        let block_input_id = block_input_id.expect("应产生 QueuedUserMessage 占位块");

        assert_eq!(
            event_id, block_input_id,
            "占位块 input_id 必须与事件 id 相同"
        );
    }

    /// 测试辅助：执行 update 返回的 `SendChatInputEvent` effect（同 executor 行为）。
    fn send_chat_input_event_for_test(app: &mut App, result: &UpdateResult) {
        for effect in &result.effects {
            if let Effect::SendChatInputEvent { event } = effect {
                app.chat.push_input_event(event.clone());
            }
        }
    }
}
