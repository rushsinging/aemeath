use std::collections::VecDeque;

use crate::tui::app::frame_driver::FrameOutcome;
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};
use crate::tui::update::msg::TuiMsg;

pub(crate) enum ExpectedEffect {
    SendUserMessage { text: String, replies: Vec<TuiMsg> },
    CancelCurrentRun { replies: Vec<TuiMsg> },
    ReadClipboardImage,
    ProcessImageFile { path: String },
    QuitApplication,
    ReplyInteraction { replies: Vec<TuiMsg> },
    CancelInteraction { replies: Vec<TuiMsg> },
}

#[derive(Default)]
pub(crate) struct ScriptedEffectDriver {
    expected: VecDeque<ExpectedEffect>,
    pub effects: Vec<Effect>,
    pub spawn_effects: Vec<SpawnAgentChatEffect>,
    pub pending_slash: Vec<String>,
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
                Effect::None | Effect::RequestRender | Effect::RunHook { .. }
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
                    Effect::CancelCurrentRun,
                    ExpectedEffect::CancelCurrentRun { replies: scripted },
                ) => replies.extend(scripted),
                (Effect::ReadClipboardImage, ExpectedEffect::ReadClipboardImage) => {}
                (
                    Effect::ProcessImageFile { path },
                    ExpectedEffect::ProcessImageFile { path: expected },
                ) => assert_eq!(path, &expected, "image path mismatch"),
                (Effect::QuitApplication, ExpectedEffect::QuitApplication) => {}
                (
                    Effect::ReplyInteraction { .. },
                    ExpectedEffect::ReplyInteraction { replies: scripted },
                ) => {
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
        if let Some(input) = outcome.pending_slash {
            self.pending_slash.push(input);
        }
        replies
    }

    pub fn is_idle(&self) -> bool {
        self.expected.is_empty() && self.spawn_effects.is_empty() && self.pending_slash.is_empty()
    }
}
