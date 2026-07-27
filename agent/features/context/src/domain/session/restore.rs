//! Canonical Session 恢复投影。

use super::message_integrity::{check_message_integrity, deep_clean_messages, sanitize_messages};
use crate::domain::session::CanonicalSession;
use share::message::Message;

#[derive(Debug, Clone)]
pub struct SessionRestoreStep {
    pub run_id: String,
    pub step_id: String,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone)]
pub struct SessionRestore {
    pub active_messages: Vec<Message>,
    pub display_steps: Vec<SessionRestoreStep>,
    pub created_at: String,
    pub trimmed: usize,
    pub repaired: usize,
}

impl SessionRestore {
    pub fn from_canonical(session: &CanonicalSession) -> Self {
        let (active_steps, active_trimmed, active_repaired) =
            clean_steps(session.flattened_steps_from_marker());
        let (display_steps, _, _) = clean_steps(session.all_persisted_steps());
        let active_messages = active_steps
            .iter()
            .flat_map(|step| step.messages.iter().cloned())
            .collect();
        Self {
            active_messages,
            display_steps,
            created_at: session.created_at.clone(),
            trimmed: active_trimmed,
            repaired: active_repaired,
        }
    }
}

fn clean_steps(
    raw_steps: Vec<(super::RunStepCursor, Vec<Message>)>,
) -> (Vec<SessionRestoreStep>, usize, usize) {
    let mut steps = Vec::with_capacity(raw_steps.len());
    let mut trimmed = 0;
    let mut repaired = 0;
    for (cursor, mut step_messages) in raw_steps {
        let before = step_messages.len();
        sanitize_messages(&mut step_messages);
        trimmed += before.saturating_sub(step_messages.len());
        if check_message_integrity(&step_messages).has_issues() {
            repaired += deep_clean_messages(&mut step_messages);
        }
        if !step_messages.is_empty() {
            steps.push(SessionRestoreStep {
                run_id: cursor.run_id,
                step_id: cursor.step_id,
                messages: step_messages,
            });
        }
    }
    (steps, trimmed, repaired)
}

#[cfg(test)]
#[path = "restore_tests.rs"]
mod tests;
