use super::{BufferDrain, RunInputBuffer};
use crate::application::loop_engine::DrainEpoch;
use sdk::ChatInputEvent;

fn um(text: &str) -> ChatInputEvent {
    ChatInputEvent::UserMessage {
        id: sdk::InputId::new(uuid::Uuid::now_v7().to_string()),
        text: text.to_string(),
        images: Vec::new(),
    }
}

fn cmd(raw: &str) -> ChatInputEvent {
    ChatInputEvent::ControlCommand {
        raw: raw.to_string(),
    }
}

/// Helper: assert a BufferDrain::Ready result matches expected texts and epoch,
/// verifying that input_ids are present (from drain_user_inputs preserving
/// ChatInputEvent::UserMessage::id).
fn assert_drain_ready(result: &BufferDrain, expected_epoch: DrainEpoch, expected_texts: &[&str]) {
    match result {
        BufferDrain::Ready { batch, epoch } => {
            assert_eq!(*epoch, expected_epoch, "epoch mismatch");
            assert_eq!(
                batch.len(),
                expected_texts.len(),
                "batch length mismatch: {:?}",
                batch.iter().map(|i| &i.text).collect::<Vec<_>>()
            );
            for (input, &expected_text) in batch.iter().zip(expected_texts.iter()) {
                assert_eq!(input.text, expected_text, "text mismatch");
                assert!(
                    input.input_id.is_some(),
                    "input_id should be Some for user messages from drain: text={:?}",
                    input.text
                );
            }
        }
        other => panic!("expected Ready, got {:?}", other),
    }
}

#[test]
fn new_buffer_is_empty() {
    let buf = RunInputBuffer::new();
    assert!(buf.is_empty());
    assert_eq!(buf.pending_user_count(), 0);
    assert!(!buf.is_sealed());
    assert_eq!(buf.current_epoch(), DrainEpoch(0));
}

#[test]
fn push_user_message_increases_count() {
    let mut buf = RunInputBuffer::new();
    buf.push(um("hello"));
    assert!(!buf.is_empty());
    assert_eq!(buf.pending_user_count(), 1);
}

#[test]
fn drain_user_inputs_returns_loop_inputs_and_keeps_non_user() {
    let mut buf = RunInputBuffer::new();
    buf.push(um("first"));
    buf.push(cmd("/clear"));
    buf.push(um("second"));

    let inputs = buf.drain_user_inputs();
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0].text, "first");
    assert_eq!(inputs[1].text, "second");
    // #1272: input_id from ChatInputEvent::UserMessage::id must be preserved
    assert!(
        inputs[0].input_id.is_some(),
        "input_id must be Some for user messages"
    );
    assert!(
        inputs[1].input_id.is_some(),
        "input_id must be Some for user messages"
    );
    // IDs must be distinct (each um() generates a unique UUIDv7)
    assert_ne!(
        inputs[0].input_id, inputs[1].input_id,
        "input_ids must be distinct"
    );
    assert!(!buf.is_empty());
    assert_eq!(buf.pending_user_count(), 0);
}

#[test]
fn withdraw_all_removes_user_texts_and_keeps_controls() {
    let mut buf = RunInputBuffer::new();
    buf.push(um("a"));
    buf.push(cmd("/model"));
    buf.push(um("b"));

    assert_eq!(buf.withdraw_all_user_texts(), vec!["a", "b"]);
    assert!(!buf.is_empty());
    let remaining = buf.drain_all();
    assert_eq!(remaining.len(), 1);
    assert!(matches!(
        remaining[0],
        ChatInputEvent::ControlCommand { .. }
    ));
}

#[test]
fn drain_all_clears_everything() {
    let mut buf = RunInputBuffer::new();
    buf.push(um("x"));
    buf.push(cmd("/reset"));

    assert_eq!(buf.drain_all().len(), 2);
    assert!(buf.is_empty());
}

#[test]
fn empty_drain_returns_empty_vec() {
    let mut buf = RunInputBuffer::new();
    assert!(buf.drain_user_inputs().is_empty());
    assert!(buf.withdraw_all_user_texts().is_empty());
    assert!(buf.drain_all().is_empty());
}

#[test]
fn snapshot_includes_user_messages_only() {
    let mut buf = RunInputBuffer::new();
    buf.push(um("snapshot me"));
    buf.push(cmd("/compact"));

    let snap = buf.user_message_snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].1.text_content(), "snapshot me");
}

// ── #1272 drain_or_seal tests ──────────────────────────────────────────

#[test]
fn drain_or_seal_returns_ready_when_user_inputs_present() {
    let mut buf = RunInputBuffer::new();
    buf.push(um("input"));

    let result = buf.drain_or_seal(DrainEpoch(0));
    assert_drain_ready(&result, DrainEpoch(0), &["input"]);
    assert!(!buf.is_sealed());
    assert!(buf.is_empty());
    assert_eq!(buf.current_epoch(), DrainEpoch(1));
}

#[test]
fn drain_or_seal_seals_when_empty() {
    let mut buf = RunInputBuffer::new();

    let result = buf.drain_or_seal(DrainEpoch(0));
    assert_eq!(
        result,
        BufferDrain::EmptyAndSealed {
            epoch: DrainEpoch(0)
        }
    );
    assert!(buf.is_sealed());
    assert_eq!(buf.current_epoch(), DrainEpoch(1));
}

#[test]
fn drain_or_seal_returns_already_sealed_after_seal() {
    let mut buf = RunInputBuffer::new();
    assert_eq!(
        buf.drain_or_seal(DrainEpoch(0)),
        BufferDrain::EmptyAndSealed {
            epoch: DrainEpoch(0)
        }
    );
    assert!(buf.is_sealed());

    // Second call on sealed buffer — epoch doesn't matter, AlreadySealed
    // is returned before epoch check.
    assert_eq!(
        buf.drain_or_seal(DrainEpoch(1)),
        BufferDrain::AlreadySealed {
            epoch: DrainEpoch(0)
        }
    );
}

#[test]
fn drain_or_seal_keeps_non_user_events() {
    let mut buf = RunInputBuffer::new();
    buf.push(cmd("/compact"));

    let result = buf.drain_or_seal(DrainEpoch(0));
    // No user inputs → seal
    assert_eq!(
        result,
        BufferDrain::EmptyAndSealed {
            epoch: DrainEpoch(0)
        }
    );
    assert!(buf.is_sealed());

    // Non-user event still in buffer (for drain_all later)
    let remaining = buf.drain_all();
    assert_eq!(remaining.len(), 1);
    assert!(matches!(
        remaining[0],
        ChatInputEvent::ControlCommand { .. }
    ));
}

#[test]
fn drain_or_seal_preserves_fifo_order_of_user_messages() {
    let mut buf = RunInputBuffer::new();
    buf.push(um("first"));
    buf.push(cmd("/clear"));
    buf.push(um("second"));
    buf.push(um("third"));

    let result = buf.drain_or_seal(DrainEpoch(0));
    assert_drain_ready(&result, DrainEpoch(0), &["first", "second", "third"]);
}

// ── #1272 test inject hook ─────────────────────────────────────────────

#[test]
fn inject_after_drain_turns_empty_seal_into_ready() {
    let mut buf = RunInputBuffer::new();
    // Buffer is empty; drain_or_seal would normally seal.
    // But the test hook injects a UserMessage after the drain pass.
    buf.test_inject_after_drain = vec![um("late arrival")];

    let result = buf.drain_or_seal(DrainEpoch(0));
    assert_drain_ready(&result, DrainEpoch(0), &["late arrival"]);
    assert!(!buf.is_sealed());
}

#[test]
fn inject_after_drain_merges_with_existing_batch_preserving_fifo_order() {
    let mut buf = RunInputBuffer::new();
    // Pre-seed with a user message — first drain produces a non-empty batch.
    buf.push(um("existing"));
    // Inject a second message via the test hook.
    buf.test_inject_after_drain = vec![um("injected")];

    let result = buf.drain_or_seal(DrainEpoch(0));
    assert_drain_ready(&result, DrainEpoch(0), &["existing", "injected"]);
    assert!(!buf.is_sealed());
}

#[test]
fn inject_after_drain_with_user_and_control_still_seals_if_no_user() {
    let mut buf = RunInputBuffer::new();
    // Inject only a control command — no UserMessage → still seal.
    buf.test_inject_after_drain = vec![cmd("/compact")];

    let result = buf.drain_or_seal(DrainEpoch(0));
    assert_eq!(
        result,
        BufferDrain::EmptyAndSealed {
            epoch: DrainEpoch(0)
        }
    );
    assert!(buf.is_sealed());
}

#[test]
fn inject_does_not_affect_already_sealed_buffer() {
    let mut buf = RunInputBuffer::new();
    assert_eq!(
        buf.drain_or_seal(DrainEpoch(0)),
        BufferDrain::EmptyAndSealed {
            epoch: DrainEpoch(0)
        }
    );
    assert!(buf.is_sealed());

    // Hook is ignored when already sealed
    buf.test_inject_after_drain = vec![um("too late")];
    assert_eq!(
        buf.drain_or_seal(DrainEpoch(1)),
        BufferDrain::AlreadySealed {
            epoch: DrainEpoch(0)
        }
    );
}

// ── #1272 sealed push rejection ────────────────────────────────────────

#[test]
fn push_or_reject_rejects_user_message_when_sealed() {
    let mut buf = RunInputBuffer::new();
    assert_eq!(
        buf.drain_or_seal(DrainEpoch(0)),
        BufferDrain::EmptyAndSealed {
            epoch: DrainEpoch(0)
        }
    );
    assert!(buf.is_sealed());

    let rejected = buf.push_or_reject(um("after seal"));
    assert!(
        rejected.is_some(),
        "UserMessage must be rejected when sealed"
    );
    assert!(buf.is_empty());
}

#[test]
fn push_or_reject_accepts_control_when_sealed() {
    let mut buf = RunInputBuffer::new();
    assert_eq!(
        buf.drain_or_seal(DrainEpoch(0)),
        BufferDrain::EmptyAndSealed {
            epoch: DrainEpoch(0)
        }
    );
    assert!(buf.is_sealed());

    let rejected = buf.push_or_reject(cmd("/reset"));
    assert!(
        rejected.is_none(),
        "Control commands accepted on sealed buffer"
    );
    assert!(!buf.is_empty());
}

#[test]
fn push_or_reject_accepts_user_message_when_not_sealed() {
    let mut buf = RunInputBuffer::new();
    let rejected = buf.push_or_reject(um("normal"));
    assert!(rejected.is_none());
    assert_eq!(buf.pending_user_count(), 1);
}

// ── #1272 drain_input sealed-admission contract ─────────────────────────

/// Simulates drain_input processing external source events (input_events +
/// queue) after a prior drain_or_seal sealed the buffer. UserMessages must
/// be rejected and routed to pending_input, not silently buffered (#1272).
#[test]
fn sealed_buffer_rejects_external_source_user_messages() {
    let mut buf = RunInputBuffer::new();
    // Prior drain_or_seal returns EmptyAndSealed → buffer is now sealed.
    assert_eq!(
        buf.drain_or_seal(DrainEpoch(0)),
        BufferDrain::EmptyAndSealed {
            epoch: DrainEpoch(0)
        }
    );
    assert!(buf.is_sealed());

    // External sources deliver a mix of events. UserMessages go through
    // push_or_reject (as used by admit_user_message) and must be rejected.
    let external_events = vec![um("after seal 1"), cmd("/list"), um("after seal 2")];

    let mut rejected_count = 0;
    let mut accepted_count = 0;
    for event in external_events {
        match buf.push_or_reject(event) {
            Some(rejected) => {
                assert!(
                    matches!(rejected, ChatInputEvent::UserMessage { .. }),
                    "only UserMessages are rejected; controls always accepted"
                );
                rejected_count += 1;
            }
            None => accepted_count += 1,
        }
    }

    assert_eq!(rejected_count, 2, "both UserMessages must be rejected");
    assert_eq!(accepted_count, 1, "control command must be accepted");
    assert_eq!(
        buf.pending_user_count(),
        0,
        "no UserMessages left in buffer"
    );
    assert!(
        !buf.is_empty(),
        "control command still in buffer for drain_all"
    );
}

#[test]
fn plain_push_exists_for_internal_use_only() {
    let mut buf = RunInputBuffer::new();
    assert_eq!(
        buf.drain_or_seal(DrainEpoch(0)),
        BufferDrain::EmptyAndSealed {
            epoch: DrainEpoch(0)
        }
    );
    assert!(buf.is_sealed());

    // plain push() is a low-level operation. All UserMessage admission
    // paths (drain_input, queue_busy_event) now use push_or_reject to
    // ensure sealed buffers reject UserMessages to pending_input (#1272).
    // This test documents that push() still exists — but it must never
    // be called directly for UserMessage by admission callers.
    buf.push(um("plain push after seal"));
    assert_eq!(buf.pending_user_count(), 1);
}

// ── #1272 epoch tests ──────────────────────────────────────────────────

#[test]
fn initial_epoch_is_zero_ready_then_empty_at_epoch_one() {
    let mut buf = RunInputBuffer::new();
    assert_eq!(buf.current_epoch(), DrainEpoch(0));

    // First drain: Ready at epoch 0
    buf.push(um("hello"));
    let result = buf.drain_or_seal(DrainEpoch(0));
    assert_drain_ready(&result, DrainEpoch(0), &["hello"]);
    assert!(!buf.is_sealed());
    assert_eq!(buf.current_epoch(), DrainEpoch(1));

    // Second drain: Empty at epoch 1
    let result = buf.drain_or_seal(DrainEpoch(1));
    assert_eq!(
        result,
        BufferDrain::EmptyAndSealed {
            epoch: DrainEpoch(1)
        }
    );
    assert!(buf.is_sealed());
    assert_eq!(buf.current_epoch(), DrainEpoch(2));
}

#[test]
fn epoch_mismatch_rejects_without_consuming_input() {
    let mut buf = RunInputBuffer::new();
    buf.push(um("should not be consumed"));

    // Pass wrong epoch — mismatch expected
    let result = buf.drain_or_seal(DrainEpoch(5));
    assert_eq!(
        result,
        BufferDrain::EpochMismatch {
            expected: DrainEpoch(5),
            actual: DrainEpoch(0),
        }
    );

    // Buffer state unchanged: not sealed, input still present, epoch unchanged
    assert!(!buf.is_sealed());
    assert_eq!(buf.current_epoch(), DrainEpoch(0));
    assert_eq!(buf.pending_user_count(), 1);

    // Correct epoch now succeeds
    let result = buf.drain_or_seal(DrainEpoch(0));
    assert_drain_ready(&result, DrainEpoch(0), &["should not be consumed"]);
    assert_eq!(buf.current_epoch(), DrainEpoch(1));
}

#[test]
fn test_injection_preserves_fifo_with_correct_epochs() {
    let mut buf = RunInputBuffer::new();

    // First drain at epoch 0: Ready with one input
    buf.push(um("first"));
    let result = buf.drain_or_seal(DrainEpoch(0));
    assert_drain_ready(&result, DrainEpoch(0), &["first"]);
    assert_eq!(buf.current_epoch(), DrainEpoch(1));

    // Epoch 1: inject after drain — should be Ready with correct epoch
    buf.test_inject_after_drain = vec![um("injected")];
    let result = buf.drain_or_seal(DrainEpoch(1));
    assert_drain_ready(&result, DrainEpoch(1), &["injected"]);
    assert!(!buf.is_sealed());
    assert_eq!(buf.current_epoch(), DrainEpoch(2));

    // Epoch 2: with pre-seeded input + injection, FIFO preserved
    buf.push(um("existing"));
    buf.test_inject_after_drain = vec![um("late"), um("also late")];
    let result = buf.drain_or_seal(DrainEpoch(2));
    assert_drain_ready(&result, DrainEpoch(2), &["existing", "late", "also late"]);
    assert!(!buf.is_sealed());
    assert_eq!(buf.current_epoch(), DrainEpoch(3));
}

// ── #1272 internal continuation epoch tests ───────────────────────────

/// L2: `take_internal_continuation` advances the epoch (unlike the old
/// `drain_user_inputs` + `current_epoch()` read pattern). Subsequent
/// `drain_or_seal` sees the advanced epoch and can empty-seal correctly.
#[test]
fn take_internal_continuation_advances_epoch_then_drain_or_seal_increments() {
    let mut buf = RunInputBuffer::new();

    // Step 1: take_internal_continuation at epoch 0 with one user message
    buf.push(um("continuation input"));
    let result = buf.take_internal_continuation(DrainEpoch(0));
    assert_drain_ready(&result, DrainEpoch(0), &["continuation input"]);
    assert!(!buf.is_sealed(), "take_internal_continuation never seals");
    assert_eq!(buf.current_epoch(), DrainEpoch(1));

    // Step 2: drain_or_seal at epoch 1 with empty buffer → EmptyAndSealed
    let result = buf.drain_or_seal(DrainEpoch(1));
    assert_eq!(
        result,
        BufferDrain::EmptyAndSealed {
            epoch: DrainEpoch(1)
        }
    );
    assert!(buf.is_sealed());
    assert_eq!(buf.current_epoch(), DrainEpoch(2));
}

/// `take_internal_continuation` rejects epoch mismatch just like
/// `drain_or_seal`.
#[test]
fn take_internal_continuation_rejects_epoch_mismatch() {
    let mut buf = RunInputBuffer::new();
    buf.push(um("should not be consumed"));

    let result = buf.take_internal_continuation(DrainEpoch(5));
    assert_eq!(
        result,
        BufferDrain::EpochMismatch {
            expected: DrainEpoch(5),
            actual: DrainEpoch(0),
        }
    );
    // Buffer state unchanged
    assert!(!buf.is_sealed());
    assert_eq!(buf.current_epoch(), DrainEpoch(0));
    assert_eq!(buf.pending_user_count(), 1);
}

/// `take_internal_continuation` on an already-sealed buffer returns
/// `AlreadySealed`.
#[test]
fn take_internal_continuation_on_sealed_buffer_returns_already_sealed() {
    let mut buf = RunInputBuffer::new();
    assert_eq!(
        buf.drain_or_seal(DrainEpoch(0)),
        BufferDrain::EmptyAndSealed {
            epoch: DrainEpoch(0)
        }
    );
    assert!(buf.is_sealed());

    let result = buf.take_internal_continuation(DrainEpoch(1));
    assert_eq!(
        result,
        BufferDrain::AlreadySealed {
            epoch: DrainEpoch(0)
        }
    );
}

// ── #1272 try_drain_unsealed tests ───────────────────────────────────

/// `try_drain_unsealed` returns Ready when user inputs are present.
#[test]
fn try_drain_unsealed_returns_ready_when_user_inputs_present() {
    let mut buf = RunInputBuffer::new();
    buf.push(um("hello"));
    let result = buf.try_drain_unsealed(DrainEpoch(0));
    assert_drain_ready(&result, DrainEpoch(0), &["hello"]);
    assert!(!buf.is_sealed());
    assert_eq!(buf.current_epoch(), DrainEpoch(1));
}

/// `try_drain_unsealed` returns Empty (NOT EmptyAndSealed) when buffer
/// is empty. Buffer is NOT sealed and epoch is NOT advanced.
#[test]
fn try_drain_unsealed_returns_empty_not_sealed_when_empty() {
    let mut buf = RunInputBuffer::new();
    let result = buf.try_drain_unsealed(DrainEpoch(0));
    assert_eq!(
        result,
        BufferDrain::Empty {
            epoch: DrainEpoch(0)
        }
    );
    assert!(!buf.is_sealed(), "buffer must NOT be sealed");
    assert_eq!(buf.current_epoch(), DrainEpoch(0), "epoch must NOT advance");
}

/// `try_drain_unsealed` on empty buffer: epoch stays the same, so
/// a subsequent call with the same expected epoch works.
#[test]
fn try_drain_unsealed_empty_then_retry_same_epoch() {
    let mut buf = RunInputBuffer::new();
    let result = buf.try_drain_unsealed(DrainEpoch(0));
    assert_eq!(
        result,
        BufferDrain::Empty {
            epoch: DrainEpoch(0)
        }
    );
    assert_eq!(buf.current_epoch(), DrainEpoch(0));

    // Push input after the empty drain
    buf.push(um("late input"));
    let result = buf.try_drain_unsealed(DrainEpoch(0));
    assert_drain_ready(&result, DrainEpoch(0), &["late input"]);
    assert!(!buf.is_sealed());
    assert_eq!(buf.current_epoch(), DrainEpoch(1));
}

/// `try_drain_unsealed` after `drain_or_seal` sealed the buffer
/// returns AlreadySealed.
#[test]
fn try_drain_unsealed_on_sealed_buffer_returns_already_sealed() {
    let mut buf = RunInputBuffer::new();
    assert_eq!(
        buf.drain_or_seal(DrainEpoch(0)),
        BufferDrain::EmptyAndSealed {
            epoch: DrainEpoch(0)
        }
    );
    assert!(buf.is_sealed());

    let result = buf.try_drain_unsealed(DrainEpoch(1));
    assert_eq!(
        result,
        BufferDrain::AlreadySealed {
            epoch: DrainEpoch(0)
        }
    );
}

/// `try_drain_unsealed` rejects epoch mismatch without consuming input.
#[test]
fn try_drain_unsealed_rejects_epoch_mismatch() {
    let mut buf = RunInputBuffer::new();
    buf.push(um("test"));
    let result = buf.try_drain_unsealed(DrainEpoch(5));
    assert_eq!(
        result,
        BufferDrain::EpochMismatch {
            expected: DrainEpoch(5),
            actual: DrainEpoch(0),
        }
    );
    // Input was not consumed
    assert_eq!(buf.pending_user_count(), 1);
    assert_eq!(buf.current_epoch(), DrainEpoch(0));
}
