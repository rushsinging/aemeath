use std::time::Duration;

use super::*;

fn timing(total_ms: u64) -> FrameTiming {
    FrameTiming {
        prepare: Duration::from_millis(total_ms / 4),
        flush: Duration::from_millis(total_ms / 2),
        draw: Duration::from_millis(total_ms / 4),
        total: Duration::from_millis(total_ms),
    }
}

fn context() -> FrameDiagnosticContext {
    FrameDiagnosticContext {
        output_dirty: true,
        revision: 7,
        timeline_items: 11,
        output_roots: 5,
        document_lines: 23,
        assemble_calls: 1,
    }
}

#[test]
fn frame_diagnostics_reports_first_frame_once_even_when_fast() {
    let mut diagnostics = FrameDiagnostics::new(Duration::from_millis(50), Duration::from_secs(5));

    let first = diagnostics
        .classify(Duration::ZERO, timing(10), context(), None)
        .expect("first frame");
    assert_eq!(first.kind, FrameDiagnosticKind::FirstFrame);
    assert_eq!(first.context, context());

    assert_eq!(
        diagnostics.classify(Duration::from_secs(1), timing(10), context(), None),
        None
    );
}

#[test]
fn frame_diagnostics_reports_slow_frame_at_threshold() {
    let mut diagnostics = FrameDiagnostics::new(Duration::from_millis(50), Duration::from_secs(5));
    diagnostics.classify(Duration::ZERO, timing(10), context(), None);

    let slow = diagnostics
        .classify(Duration::from_secs(1), timing(50), context(), None)
        .expect("slow frame");
    assert_eq!(slow.kind, FrameDiagnosticKind::SlowFrame);
    assert_eq!(slow.timing.total, Duration::from_millis(50));
}

#[test]
fn frame_diagnostics_cools_down_repeated_slow_frames() {
    let mut diagnostics = FrameDiagnostics::new(Duration::from_millis(50), Duration::from_secs(5));
    diagnostics.classify(Duration::ZERO, timing(10), context(), None);
    assert!(diagnostics
        .classify(Duration::from_secs(1), timing(60), context(), None)
        .is_some());
    assert_eq!(
        diagnostics.classify(Duration::from_secs(2), timing(70), context(), None),
        None
    );
    assert!(diagnostics
        .classify(Duration::from_secs(6), timing(80), context(), None)
        .is_some());
}

#[test]
fn frame_diagnostics_keeps_phase_context_and_memory_sample() {
    let mut diagnostics = FrameDiagnostics::new(Duration::from_millis(50), Duration::from_secs(5));
    let memory = ProcessMemorySnapshot {
        current_rss_bytes: 1_024,
        peak_rss_bytes: 2_048,
        first_rss_bytes: 512,
        growth_from_first_bytes: 512,
        growth_from_previous_bytes: 128,
    };

    let event = diagnostics
        .classify(Duration::ZERO, timing(75), context(), Some(memory))
        .expect("first slow frame");
    assert_eq!(event.kind, FrameDiagnosticKind::FirstFrame);
    assert_eq!(event.timing.flush, Duration::from_millis(37));
    assert_eq!(event.memory, Some(memory));
    assert_eq!(event.context.document_lines, 23);
}
