use std::cell::RefCell;
use std::future::Future;
use std::sync::{Arc, Weak};

use share::message::{ContentBlock, Message};

use crate::domain::session::CanonicalSession;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionStructureSnapshot {
    pub revision: u64,
    pub run_slices: usize,
    pub steps: usize,
    pub accepted_messages: usize,
    pub outcome_messages: usize,
    pub tool_result_blocks: usize,
    pub tool_result_content_bytes: u64,
    pub committed_steps: usize,
}

#[derive(Clone, Debug)]
pub struct SessionGenerationTransition {
    pub before: SessionStructureSnapshot,
    pub after: SessionStructureSnapshot,
    pub replaced_generation: Weak<CanonicalSession>,
}

#[derive(Clone, Debug, Default)]
pub struct SessionLifecycleSnapshot {
    pub transitions: Vec<SessionGenerationTransition>,
}

thread_local! {
    static ACTIVE_CAPTURE: RefCell<Option<SessionLifecycleSnapshot>> = const { RefCell::new(None) };
}

struct CaptureGuard {
    active: bool,
}

impl CaptureGuard {
    fn start() -> Self {
        ACTIVE_CAPTURE.with(|capture| {
            assert!(
                capture.borrow().is_none(),
                "session lifecycle capture 不支持嵌套"
            );
            *capture.borrow_mut() = Some(SessionLifecycleSnapshot::default());
        });
        Self { active: true }
    }

    fn finish(mut self) -> SessionLifecycleSnapshot {
        self.active = false;
        ACTIVE_CAPTURE.with(|capture| {
            capture
                .borrow_mut()
                .take()
                .expect("session lifecycle capture scope 应保持有效")
        })
    }
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        if self.active {
            ACTIVE_CAPTURE.with(|capture| {
                capture.borrow_mut().take();
            });
        }
    }
}

pub async fn capture<T>(run: impl Future<Output = T>) -> (T, SessionLifecycleSnapshot) {
    let guard = CaptureGuard::start();
    let value = run.await;
    (value, guard.finish())
}

pub(crate) fn record_generation_transition(
    before: &Arc<CanonicalSession>,
    after: &CanonicalSession,
) {
    ACTIVE_CAPTURE.with(|capture| {
        let mut capture = capture.borrow_mut();
        let Some(snapshot) = capture.as_mut() else {
            return;
        };
        snapshot.transitions.push(SessionGenerationTransition {
            before: structure(before),
            after: structure(after),
            replaced_generation: Arc::downgrade(before),
        });
    });
}

fn structure(session: &CanonicalSession) -> SessionStructureSnapshot {
    let mut snapshot = SessionStructureSnapshot {
        revision: session.revision,
        run_slices: session.run_slices.len(),
        committed_steps: session.committed_steps.len(),
        ..SessionStructureSnapshot::default()
    };
    for slice in &session.run_slices {
        snapshot.steps = snapshot.steps.saturating_add(slice.steps.len());
        for step in &slice.steps {
            if let Some(input) = &step.accepted_input {
                snapshot.accepted_messages = snapshot
                    .accepted_messages
                    .saturating_add(input.messages.len());
                add_tool_result_metrics(&mut snapshot, &input.messages);
            }
            if let Some(outcome) = &step.outcome {
                snapshot.outcome_messages = snapshot
                    .outcome_messages
                    .saturating_add(outcome.messages.len());
                add_tool_result_metrics(&mut snapshot, &outcome.messages);
            }
        }
    }
    snapshot
}

fn add_tool_result_metrics(snapshot: &mut SessionStructureSnapshot, messages: &[Message]) {
    for block in messages.iter().flat_map(|message| message.content.iter()) {
        if let ContentBlock::ToolResult { content, .. } = block {
            snapshot.tool_result_blocks = snapshot.tool_result_blocks.saturating_add(1);
            snapshot.tool_result_content_bytes = snapshot
                .tool_result_content_bytes
                .saturating_add(u64::try_from(content.to_string().len()).unwrap_or(u64::MAX));
        }
    }
}
