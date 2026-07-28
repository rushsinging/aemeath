use super::InstrumentedBackend;
use crate::tui::render::performance::capture;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

#[test]
fn records_terminal_diff_and_backend_flush() {
    let mut terminal = Terminal::new(InstrumentedBackend::new(TestBackend::new(20, 4)))
        .expect("instrumented test terminal");

    let (_, metrics) = capture(|| {
        terminal
            .draw(|frame| frame.render_widget("hello", frame.area()))
            .expect("draw succeeds");
    });

    assert_eq!(metrics.terminal_diff_calls, 1);
    assert!(metrics.terminal_diff_cells > 0);
    assert_eq!(metrics.backend_flush_calls, 1);
    assert_eq!(terminal.backend().inner().buffer()[(0, 0)].symbol(), "h");
}
