use super::format_duration;
use crate::tui::render::output_area::OutputArea;
use crate::tui::view_model::SpinnerLineView;

fn line_text(elapsed_secs: u64, phase_elapsed_secs: u64) -> String {
    let output = OutputArea::new();
    let view = SpinnerLineView {
        frame: 0,
        verb: "Thinking".to_string(),
        elapsed_secs,
        phase_elapsed_secs,
        phase_text: Some("Running tool".to_string()),
    };

    output
        .build_spinner_line(&view, None)
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

#[test]
fn readable_duration_formats_second_minute_and_hour_boundaries() {
    let cases = [
        (0, "0s"),
        (59, "59s"),
        (60, "1m"),
        (65, "1m 5s"),
        (3600, "1h"),
        (3723, "1h 2m 3s"),
    ];

    for (seconds, expected) in cases {
        assert_eq!(format_duration(seconds), expected);
    }
}

#[test]
fn spinner_total_and_phase_elapsed_share_readable_duration_format() {
    let text = line_text(3723, 65);

    assert!(text.contains("  1h 2m 3s"), "actual spinner line: {text}");
    assert!(text.contains("  ⏱ 1m 5s"), "actual spinner line: {text}");
    assert!(!text.contains("3723s"), "actual spinner line: {text}");
}
