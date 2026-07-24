use super::UpdateResult;
use crate::tui::app::App;
use crate::tui::model::conversation::intent::{
    CancelInteraction, ConfirmInteraction, UpdateInteractionDraft,
};
use crate::tui::model::conversation::interaction::{InteractionDraftAction, InteractionPhase};
use crate::tui::update::intent::AgentIntent;
use crossterm::event::{KeyCode, KeyEvent};

pub(super) fn handle_dialog_key(app: &mut App, key: KeyEvent) -> Option<UpdateResult> {
    if !app.layout.has_active_dialog() {
        return None;
    }

    match key.code {
        KeyCode::Up => {
            if let Some(d) = app.layout.active_dialog_mut() {
                d.select_prev();
            }
        }
        KeyCode::Down => {
            if let Some(d) = app.layout.active_dialog_mut() {
                d.select_next();
            }
        }
        KeyCode::Enter => {
            if let Some(model_key) = app.layout.selected_model_key() {
                let command = format!("/model {}", model_key);
                app.layout.clear_dialog();
                return Some(UpdateResult {
                    effects: Vec::new(),
                    spawn_effect: None,
                    pending_slash: Some(command),
                });
            }
            app.layout.clear_dialog();
        }
        KeyCode::Esc => app.layout.clear_dialog(),
        _ => return None,
    }

    Some(UpdateResult::none())
}

/// Handle key events for the interaction overlay (AskUserQuestion / ToolApproval / etc.)
pub(super) fn handle_interaction_key(app: &mut App, key: KeyEvent) -> UpdateResult {
    let Some(interaction) = app.model.conversation.active_interaction() else {
        return UpdateResult::none();
    };
    let phase = interaction.phase();
    let request_id = interaction.request_id().clone();
    drop(interaction);

    // Only Collecting / Confirming phases accept input
    if !matches!(phase, InteractionPhase::Collecting | InteractionPhase::Confirming) {
        return UpdateResult::none();
    }

    let body = app
        .model
        .conversation
        .active_interaction()
        .map(|i| i.body().clone());
    let Some(body) = body else {
        return UpdateResult::none();
    };

    match key.code {
        // ── Esc = cancel ──
        KeyCode::Esc => {
            app.apply_agent_intent(AgentIntent::Conversation(
                crate::tui::model::conversation::intent::ConversationIntent::CancelInteraction(
                    CancelInteraction {
                        request_id: request_id.clone(),
                    },
                ),
            ));
            app.layout.interaction_selected = 0;
        }
        // ── Enter = confirm ──
        KeyCode::Enter => {
            // First, confirm the draft → produces InteractionReplyRequested
            app.apply_agent_intent(AgentIntent::Conversation(
                crate::tui::model::conversation::intent::ConversationIntent::ConfirmInteraction(
                    ConfirmInteraction {
                        request_id: request_id.clone(),
                    },
                ),
            ));
            app.layout.interaction_selected = 0;
        }
        // ── Tab / Up / Down = cycle options (for option-list interactions) ──
        KeyCode::Tab | KeyCode::Down | KeyCode::Up => {
            let option_count = match &body {
                crate::tui::model::conversation::interaction::InteractionBody::UserQuestions(qs)
                    if qs.len() == 1 && !qs[0].options.is_empty() =>
                {
                    qs[0].options.len()
                }
                _ => 2, // Approval: Approve / Deny
            };
            if option_count > 0 {
                if matches!(key.code, KeyCode::Up) {
                    app.layout.interaction_selected = app
                        .layout
                        .interaction_selected
                        .saturating_sub(1)
                        .min(option_count.saturating_sub(1));
                } else {
                    app.layout.interaction_selected =
                        (app.layout.interaction_selected + 1) % option_count;
                }
            }
        }
        // ── Left / Right = cycle for approval-style ──
        KeyCode::Left => {
            app.layout.interaction_selected = app
                .layout
                .interaction_selected
                .saturating_sub(1)
                .min(1);
        }
        KeyCode::Right => {
            app.layout.interaction_selected = (app.layout.interaction_selected + 1) % 2;
        }
        // ── Y / N = quick approve/deny for approval interactions ──
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.apply_agent_intent(AgentIntent::Conversation(
                crate::tui::model::conversation::intent::ConversationIntent::UpdateInteractionDraft(
                    UpdateInteractionDraft {
                        request_id: request_id.clone(),
                        action: InteractionDraftAction::Approve,
                    },
                ),
            ));
            // Immediately confirm
            app.apply_agent_intent(AgentIntent::Conversation(
                crate::tui::model::conversation::intent::ConversationIntent::ConfirmInteraction(
                    ConfirmInteraction { request_id },
                ),
            ));
            app.layout.interaction_selected = 0;
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            app.apply_agent_intent(AgentIntent::Conversation(
                crate::tui::model::conversation::intent::ConversationIntent::UpdateInteractionDraft(
                    UpdateInteractionDraft {
                        request_id: request_id.clone(),
                        action: InteractionDraftAction::Deny { reason: None },
                    },
                ),
            ));
            app.apply_agent_intent(AgentIntent::Conversation(
                crate::tui::model::conversation::intent::ConversationIntent::ConfirmInteraction(
                    ConfirmInteraction { request_id },
                ),
            ));
            app.layout.interaction_selected = 0;
        }
        // ── Number keys = quick select for option lists ──
        KeyCode::Char(c) if c.is_ascii_digit() => {
            let idx = c.to_digit(10).unwrap_or(0) as usize;
            match &body {
                crate::tui::model::conversation::interaction::InteractionBody::UserQuestions(qs)
                    if qs.len() == 1 && !qs[0].options.is_empty() =>
                {
                    if idx < qs[0].options.len() {
                        app.layout.interaction_selected = idx;
                        // For single-select with options: directly confirm with this answer
                        app.apply_agent_intent(AgentIntent::Conversation(
                            crate::tui::model::conversation::intent::ConversationIntent::
                                UpdateInteractionDraft(UpdateInteractionDraft {
                                    request_id: request_id.clone(),
                                    action: InteractionDraftAction::SetUserAnswer {
                                        index: 0,
                                        answer: qs[0].options[idx].clone(),
                                    },
                                }),
                        ));
                    }
                }
                _ => {}
            }
        }
        _ => return UpdateResult::none(),
    }

    // After any interaction key, also sync the draft for option-list items
    // so the selected option becomes the draft answer.
    let request_id = app
        .model
        .conversation
        .active_interaction()
        .map(|i| i.request_id().clone());
    let Some(request_id) = request_id else {
        return UpdateResult::none();
    };
    match &body {
        crate::tui::model::conversation::interaction::InteractionBody::UserQuestions(qs)
            if qs.len() == 1 && !qs[0].options.is_empty() =>
        {
            let sel = app.layout.interaction_selected;
            if sel < qs[0].options.len() {
                app.apply_agent_intent(AgentIntent::Conversation(
                    crate::tui::model::conversation::intent::ConversationIntent::UpdateInteractionDraft(
                        UpdateInteractionDraft {
                            request_id: request_id.clone(),
                            action: InteractionDraftAction::SetUserAnswer {
                                index: 0,
                                answer: qs[0].options[sel].clone(),
                            },
                        },
                    ),
                ));
            }
        }
        crate::tui::model::conversation::interaction::InteractionBody::ToolApproval(_)
        | crate::tui::model::conversation::interaction::InteractionBody::PlanApproval(_) => {
            let action = if app.layout.interaction_selected == 0 {
                InteractionDraftAction::Approve
            } else {
                InteractionDraftAction::Deny { reason: None }
            };
            app.apply_agent_intent(AgentIntent::Conversation(
                crate::tui::model::conversation::intent::ConversationIntent::UpdateInteractionDraft(
                    UpdateInteractionDraft {
                        request_id,
                        action,
                    },
                ),
            ));
        }
        _ => {}
    }

    app.mark_output_dirty();
    UpdateResult::none()
}
