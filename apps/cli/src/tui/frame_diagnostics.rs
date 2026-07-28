use std::time::Duration;

use crate::tui::process_memory::ProcessMemorySnapshot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FrameTiming {
    pub(crate) prepare: Duration,
    pub(crate) flush: Duration,
    pub(crate) draw: Duration,
    pub(crate) total: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FrameDiagnosticContext {
    pub(crate) output_dirty: bool,
    pub(crate) revision: u64,
    pub(crate) timeline_items: usize,
    pub(crate) output_roots: usize,
    pub(crate) document_lines: usize,
    pub(crate) assemble_calls: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FrameDiagnosticKind {
    FirstFrame,
    SlowFrame,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FrameDiagnosticEvent {
    pub(crate) kind: FrameDiagnosticKind,
    pub(crate) timing: FrameTiming,
    pub(crate) context: FrameDiagnosticContext,
    pub(crate) memory: Option<ProcessMemorySnapshot>,
}

pub(crate) struct FrameDiagnostics {
    slow_threshold: Duration,
    slow_cooldown: Duration,
    first_frame_reported: bool,
    last_slow_report_at: Option<Duration>,
}

impl FrameDiagnostics {
    pub(crate) fn new(slow_threshold: Duration, slow_cooldown: Duration) -> Self {
        Self {
            slow_threshold,
            slow_cooldown,
            first_frame_reported: false,
            last_slow_report_at: None,
        }
    }

    pub(crate) fn classify(
        &mut self,
        now: Duration,
        timing: FrameTiming,
        context: FrameDiagnosticContext,
        memory: Option<ProcessMemorySnapshot>,
    ) -> Option<FrameDiagnosticEvent> {
        let kind = if !self.first_frame_reported {
            self.first_frame_reported = true;
            FrameDiagnosticKind::FirstFrame
        } else if timing.total < self.slow_threshold
            || self
                .last_slow_report_at
                .is_some_and(|last| now.saturating_sub(last) < self.slow_cooldown)
        {
            return None;
        } else {
            self.last_slow_report_at = Some(now);
            FrameDiagnosticKind::SlowFrame
        };
        Some(FrameDiagnosticEvent {
            kind,
            timing,
            context,
            memory,
        })
    }
}

#[cfg(test)]
#[path = "frame_diagnostics_tests.rs"]
mod tests;
