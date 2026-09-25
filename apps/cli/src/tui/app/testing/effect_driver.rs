use std::collections::VecDeque;

use crate::tui::app::frame_driver::FrameOutcome;
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};
use crate::tui::update::msg::TuiMsg;

pub(crate) enum ExpectedEffect {
    SendUserMessage {
        text: String,
        replies: Vec<TuiMsg>,
    },
    CancelRunStep {
        run_id: sdk::RunId,
        step_id: sdk::RunStepId,
        replies: Vec<TuiMsg>,
    },
    ReadClipboardImage,
    ProcessImageFile {
        path: String,
        fallback_text: String,
    },
    QuitApplication,
    ReplyInteraction {
        request_id: Option<String>,
        reply: Option<crate::tui::model::conversation::interaction::UiInteractionReply>,
        replies: Vec<TuiMsg>,
    },
    CancelInteraction {
        replies: Vec<TuiMsg>,
    },
}

#[derive(Default)]
pub(crate) struct ScriptedEffectDriver {
    expected: VecDeque<ExpectedEffect>,
    pub effects: Vec<Effect>,
    pub spawn_effects: Vec<SpawnAgentChatEffect>,
}

impl ScriptedEffectDriver {
    pub fn expect(&mut self, expected: ExpectedEffect) {
        self.expected.push_back(expected);
    }

    pub fn record(&mut self, outcome: FrameOutcome) -> Vec<TuiMsg> {
        let mut replies = Vec::new();
        for effect in outcome.effects {
            if matches!(
                effect,
                Effect::RequestRender
                    | Effect::RunHook { .. }
                    | Effect::LoadDisplayHistoryWindow { .. }
            ) {
                self.effects.push(effect);
                continue;
            }
            if let Effect::ResolveWorkspaceMetadata { ref root, revision } = effect {
                replies.push(TuiMsg::Ui(
                    crate::tui::app::event::UiEvent::WorkspaceMetadataResolved(
                        crate::tui::app::event::WorkspaceMetadataResolved {
                            root: root.clone(),
                            revision,
                            branch: None,
                            kind: crate::tui::model::conversation::workspace::WorktreeKind::Unknown,
                        },
                    ),
                ));
                self.effects.push(effect);
                continue;
            }
            let expected = self
                .expected
                .pop_front()
                .unwrap_or_else(|| panic!("unexpected effect: {effect:?}"));
            match (&effect, expected) {
                (
                    Effect::SendChatInputEvent {
                        event: sdk::ChatInputEvent::UserMessage { text, .. },
                    },
                    ExpectedEffect::SendUserMessage {
                        text: expected,
                        replies: scripted,
                    },
                ) => {
                    assert_eq!(text, &expected, "user message payload mismatch");
                    replies.extend(scripted);
                }
                (
                    Effect::CancelRunStep { run_id, step_id },
                    ExpectedEffect::CancelRunStep {
                        run_id: expected_run_id,
                        step_id: expected_step_id,
                        replies: scripted,
                    },
                ) => {
                    assert_eq!(run_id, &expected_run_id, "cancel run id mismatch");
                    assert_eq!(step_id, &expected_step_id, "cancel step id mismatch");
                    replies.extend(scripted);
                }
                (Effect::ReadClipboardImage, ExpectedEffect::ReadClipboardImage) => {}
                (
                    Effect::ProcessImageFile {
                        path,
                        fallback_text,
                    },
                    ExpectedEffect::ProcessImageFile {
                        path: expected_path,
                        fallback_text: expected_fallback,
                    },
                ) => {
                    assert_eq!(path, &expected_path, "image path mismatch");
                    assert_eq!(fallback_text, &expected_fallback, "原始粘贴文本 mismatch");
                }
                (Effect::QuitApplication, ExpectedEffect::QuitApplication) => {}
                (
                    Effect::ReplyInteraction { request_id, reply },
                    ExpectedEffect::ReplyInteraction {
                        request_id: expected_request_id,
                        reply: expected_reply,
                        replies: scripted,
                    },
                ) => {
                    if let Some(expected_request_id) = expected_request_id {
                        assert_eq!(
                            request_id.as_str(),
                            expected_request_id,
                            "interaction request id mismatch"
                        );
                    }
                    if let Some(expected_reply) = expected_reply {
                        assert_eq!(reply, &expected_reply, "interaction reply payload mismatch");
                    }
                    replies.extend(scripted);
                }
                (
                    Effect::CancelInteraction { .. },
                    ExpectedEffect::CancelInteraction { replies: scripted },
                ) => {
                    replies.extend(scripted);
                }
                (_, _) => panic!("effect did not match script: {effect:?}"),
            }
            self.effects.push(effect);
        }
        if let Some(effect) = outcome.spawn_effect {
            self.spawn_effects.push(effect);
        }
        replies
    }

    pub fn is_idle(&self) -> bool {
        self.expected.is_empty() && self.spawn_effects.is_empty()
    }
}
