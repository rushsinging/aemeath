//! Run-owned input buffer for Main adapter. Stores `ChatInputEvent`s received during a
//! Run's lifetime and provides `drain_or_seal` for step-level consumption within the
//! same Run, unlike the session-owned `PendingInputBuffer` which creates a fresh Run.
//!
//! #1272 Per-turn drain-or-seal linearization:
//! `drain_or_seal` atomically drains user inputs AND seals if empty — a single
//! synchronous decision point, rather than MainRunPort draining sources then
//! checking emptiness separately. Once sealed, late-arriving `UserMessage`s are
//! rejected (returned to caller) instead of silently buffered for the next Run.

use std::collections::VecDeque;

use sdk::ChatInputEvent;

use crate::application::loop_engine::DrainEpoch;
use crate::application::loop_engine::LoopInput;

/// Result of a single drain-or-seal operation on the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BufferDrain {
    /// User inputs were drained; the batch is guaranteed non-empty.
    Ready {
        batch: Vec<LoopInput>,
        epoch: DrainEpoch,
    },
    /// Buffer was empty at drain time; it is now sealed. Future UserMessage
    /// pushes will be rejected.
    EmptyAndSealed { epoch: DrainEpoch },
    /// Buffer was already sealed by a prior `drain_or_seal` call.
    AlreadySealed { epoch: DrainEpoch },
    /// Buffer was empty at drain time but was NOT sealed and epoch was NOT
    /// advanced. Used by `try_drain_unsealed` during AwaitingUser so the
    /// buffer stays receptive to future user input in the same Run (#1272).
    Empty { epoch: DrainEpoch },
    /// Caller's expected epoch does not match the buffer's current epoch.
    /// No input was consumed; no state changed.
    EpochMismatch {
        expected: DrainEpoch,
        actual: DrainEpoch,
    },
}

/// Owned by `MainRunPort`, scoped to one `Run`. User messages accumulate here from
/// `input_events`/`recv_next_input` and the legacy `Queue` during Run execution.
/// Control commands are still forwarded to session `pending_input`.
///
/// # Lifecycle
/// 1. Created fresh when `MainRunPort` is constructed.
/// 2. `drain_or_seal` returns user messages as `LoopInput`s for the next step,
///    or seals the buffer when empty.
/// 3. `withdraw_all_user_texts` clears unbound user messages (WithdrawAll).
/// 4. When the Run ends, remaining non-user events (controls) are drained via
///    `drain_all` and returned to the session.
///
/// # Seal semantics
/// Once `drain_or_seal` returns `EmptyAndSealed`, the buffer is sealed.
/// Subsequent `drain_or_seal` returns `AlreadySealed`. `push()` silently
/// accepts non-UserMessage events (controls) but `push_or_reject()` returns
/// `UserMessage` events to the caller for explicit handling.
///
/// # Epoch semantics (#1272)
/// Each successful drain-or-seal call increments the internal `current_epoch`.
/// Callers pass their expected epoch to `drain_or_seal`; mismatch returns
/// `EpochMismatch` without consuming input.
#[derive(Debug)]
pub(crate) struct RunInputBuffer {
    events: VecDeque<ChatInputEvent>,
    sealed: bool,
    /// Current expected epoch for the next `drain_or_seal` call.
    current_epoch: DrainEpoch,
    /// The epoch at which sealing occurred (set when `EmptyAndSealed` is returned).
    sealed_epoch: Option<DrainEpoch>,
    /// Test-only: events to inject after the drain pass but before the seal
    /// decision. Cleared after consumption. When non-empty, `drain_or_seal`
    /// will inject these events and re-drain, ensuring deterministic ordering
    /// without sleeps.
    #[cfg(test)]
    pub(crate) test_inject_after_drain: Vec<ChatInputEvent>,
}

impl Default for RunInputBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl RunInputBuffer {
    pub(crate) fn new() -> Self {
        Self {
            events: VecDeque::new(),
            sealed: false,
            current_epoch: DrainEpoch(0),
            sealed_epoch: None,
            #[cfg(test)]
            test_inject_after_drain: Vec::new(),
        }
    }

    /// Push an event unconditionally. Callers that may operate on a sealed
    /// buffer should prefer `push_or_reject` for `UserMessage` events so
    /// that late arrivals are not silently buffered.
    pub(crate) fn push(&mut self, event: ChatInputEvent) {
        self.events.push_back(event);
    }

    /// Push an event. If the buffer is sealed and the event is a `UserMessage`,
    /// the event is returned to the caller for explicit handling (e.g. logged
    /// rejection or routing to `pending_input`). Non-`UserMessage` events are
    /// always accepted even on a sealed buffer (control commands must still be
    /// drained back to the session).
    pub(crate) fn push_or_reject(&mut self, event: ChatInputEvent) -> Option<ChatInputEvent> {
        if self.sealed && matches!(event, ChatInputEvent::UserMessage { .. }) {
            return Some(event);
        }
        self.events.push_back(event);
        None
    }

    #[allow(dead_code)]
    pub(crate) fn extend(&mut self, events: impl IntoIterator<Item = ChatInputEvent>) {
        self.events.extend(events);
    }

    pub(crate) fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// The current expected epoch for the next `drain_or_seal` call.
    #[allow(dead_code)]
    pub(crate) fn current_epoch(&self) -> DrainEpoch {
        self.current_epoch
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Count of `UserMessage` events currently buffered.
    #[cfg(test)]
    pub(crate) fn pending_user_count(&self) -> usize {
        self.events
            .iter()
            .filter(|e| matches!(e, ChatInputEvent::UserMessage { .. }))
            .count()
    }

    // ── drain helpers (internal) ──────────────────────────────────────────

    /// Drain all `UserMessage` events as `LoopInput`s for the next Run step.
    /// Non-user events remain in the buffer for later return to session idle.
    ///
    /// Still used by `MainRunPort::drain_input` for the StopHookFeedback and
    /// ToolResults branches, which drain the batch alongside the continuation
    /// BEFORE the seal decision.
    pub(crate) fn drain_user_inputs(&mut self) -> Vec<LoopInput> {
        let mut inputs = Vec::new();
        let mut retained = VecDeque::new();
        while let Some(event) = self.events.pop_front() {
            match event {
                ChatInputEvent::UserMessage {
                    id, text, images, ..
                } => {
                    inputs.push(LoopInput {
                        text,
                        input_id: Some(id),
                        images,
                    });
                }
                other => retained.push_back(other),
            }
        }
        self.events = retained;
        inputs
    }

    // ── internal continuation ──────────────────────────────────────────

    /// Drain user inputs and advance epoch for engine-driven continuations
    /// (StopHookFeedback / ToolResults). Unlike `drain_or_seal`, this
    /// never seals the buffer — only the normal drain-or-seal path seals
    /// (#1272 per-turn drain linearization).
    ///
    /// Returns `Ready` with the drained batch and the consumed epoch.
    /// Returns `EpochMismatch` / `AlreadySealed` on the same guards as
    /// `drain_or_seal`.
    pub(crate) fn take_internal_continuation(&mut self, expected: DrainEpoch) -> BufferDrain {
        if self.sealed {
            return BufferDrain::AlreadySealed {
                epoch: self.sealed_epoch.expect("sealed_epoch set when sealed"),
            };
        }
        if expected != self.current_epoch {
            return BufferDrain::EpochMismatch {
                expected,
                actual: self.current_epoch,
            };
        }
        let batch = self.drain_user_inputs();
        let epoch = self.current_epoch;
        self.current_epoch = DrainEpoch(epoch.0 + 1);
        BufferDrain::Ready { batch, epoch }
    }

    // ── await-user-input drain (never seals) ────────────────────────────

    /// Drain user inputs WITHOUT sealing. Used during AwaitingUser so the
    /// buffer stays receptive to future user input in the same Run (#1272).
    ///
    /// Unlike `drain_or_seal`, this NEVER seals the buffer. When the buffer
    /// is empty, returns `Empty { epoch }` WITHOUT advancing the internal
    /// epoch — the next call can retry with the same expected epoch.
    ///
    /// When user inputs are present, behaves like `drain_or_seal` (returns
    /// `Ready`, advances epoch).
    pub(crate) fn try_drain_unsealed(&mut self, expected: DrainEpoch) -> BufferDrain {
        if self.sealed {
            return BufferDrain::AlreadySealed {
                epoch: self.sealed_epoch.expect("sealed_epoch set when sealed"),
            };
        }
        if expected != self.current_epoch {
            return BufferDrain::EpochMismatch {
                expected,
                actual: self.current_epoch,
            };
        }
        let batch = self.drain_user_inputs();
        if batch.is_empty() {
            // #1272: empty during AwaitingUser — don't seal, don't advance epoch
            BufferDrain::Empty {
                epoch: self.current_epoch,
            }
        } else {
            let epoch = self.current_epoch;
            self.current_epoch = DrainEpoch(epoch.0 + 1);
            BufferDrain::Ready { batch, epoch }
        }
    }

    // ── atomic drain-or-seal ─────────────────────────────────────────────

    /// Atomically drain user inputs OR seal the buffer if empty.
    ///
    /// This is the single linearization point for the per-turn drain-or-seal
    /// contract (#1272). After this returns `EmptyAndSealed`, the buffer is
    /// sealed — future `UserMessage` pushes via `push_or_reject` are rejected.
    ///
    /// # Epoch guard
    /// `expected` must equal the buffer's current epoch. On mismatch,
    /// `EpochMismatch` is returned without consuming input. On success
    /// (Ready or EmptyAndSealed), the current epoch is incremented.
    ///
    /// Test hook: if `test_inject_after_drain` is non-empty (test-only), the
    /// events are injected after the first drain pass but before the seal
    /// decision. If the injected events include a `UserMessage`, the result
    /// is `Ready` (not `EmptyAndSealed`), proving that the seal decision is
    /// atomic with respect to the buffer state.
    pub(crate) fn drain_or_seal(&mut self, expected: DrainEpoch) -> BufferDrain {
        if self.sealed {
            return BufferDrain::AlreadySealed {
                epoch: self.sealed_epoch.expect("sealed_epoch set when sealed"),
            };
        }

        if expected != self.current_epoch {
            return BufferDrain::EpochMismatch {
                expected,
                actual: self.current_epoch,
            };
        }

        let batch = self.drain_user_inputs();

        // Test-only: inject late-arriving events between drain and seal.
        // This allows deterministic testing that a UserMessage arriving in
        // the critical window produces Ready, not EmptyAndSealed (#1272).
        // When the first drain also produced a batch, the injected batch
        // is merged in FIFO order (existing first, injected second).
        #[cfg(test)]
        let batch = {
            if !self.test_inject_after_drain.is_empty() {
                let mut batch = batch;
                let injected: Vec<_> = self.test_inject_after_drain.drain(..).collect();
                for event in injected {
                    self.events.push_back(event);
                }
                let batch2 = self.drain_user_inputs();
                batch.extend(batch2);
                batch
            } else {
                batch
            }
        };

        let epoch = self.current_epoch;
        self.current_epoch = DrainEpoch(epoch.0 + 1);

        if batch.is_empty() {
            self.sealed = true;
            self.sealed_epoch = Some(epoch);
            BufferDrain::EmptyAndSealed { epoch }
        } else {
            BufferDrain::Ready { batch, epoch }
        }
    }

    /// Withdraw all `UserMessage` events and return their texts.
    /// Non-user events (control commands) remain in the buffer.
    pub(crate) fn withdraw_all_user_texts(&mut self) -> Vec<String> {
        let mut texts = Vec::new();
        let mut retained = VecDeque::new();
        while let Some(event) = self.events.pop_front() {
            match event {
                ChatInputEvent::UserMessage { text, .. } => texts.push(text),
                other => retained.push_back(other),
            }
        }
        self.events = retained;
        texts
    }

    /// Drain all remaining events. Called when the Run ends to return
    /// unconsumed control events back to session `pending_input`.
    ///
    /// When the buffer is sealed, `UserMessage` events should not be present
    /// (they were either drained by `drain_or_seal` or rejected by
    /// `push_or_reject`). If any are found, they are drained but the caller
    /// should treat them as anomalous (log + route explicitly, not silently
    /// forward to the next Run).
    pub(crate) fn drain_all(&mut self) -> Vec<ChatInputEvent> {
        self.events.drain(..).collect()
    }

    /// Snapshot current `UserMessage` events as (InputId, Message) pairs,
    /// used for `UserMessagesQueued` emission during busy `select!`.
    pub(crate) fn user_message_snapshot(&self) -> Vec<(sdk::InputId, share::message::Message)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                ChatInputEvent::UserMessage { id, text, .. } => {
                    Some((id.clone(), share::message::Message::user(text.clone())))
                }
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "run_input_buffer_tests.rs"]
mod tests;
