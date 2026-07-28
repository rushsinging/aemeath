use std::time::Instant;

use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

use super::super::testing::fixture;
use super::super::testing::instrumented_backend::InstrumentedBackend;
use crate::tui::model::conversation::intent::AppendUserMessage;
use crate::tui::render::performance::{capture, percentiles_ns, RenderPerformanceSnapshot};

const WIDTH: u16 = 100;
const HEIGHT: u16 = 30;
const SAMPLES: usize = 20;

fn frame_app(blocks: usize) -> crate::tui::app::App {
    let mut app = fixture::app();
    for index in 0..blocks {
        app.model.conversation.apply(AppendUserMessage {
            text: format!("message-{index:04} markdown **bold** 中文 🚀"),
        });
    }
    app.view_state.dirty.mark_output();
    app.handle_resize(WIDTH, HEIGHT);
    app.view_state.output.last_visible_height = HEIGHT.saturating_sub(7) as usize;
    app
}

fn draw_frame(
    app: &mut crate::tui::app::App,
    terminal: &mut Terminal<InstrumentedBackend<TestBackend>>,
) -> RenderPerformanceSnapshot {
    if app.layout.output_area_rect.height == 0 {
        app.draw(terminal).expect("initial layout draw");
    }
    let (_, metrics) = capture(|| {
        app.prepare_frame();
        app.draw(terminal).expect("instrumented frame draw");
    });
    metrics
}

#[test]
fn frame_pipeline_reports_cold_spinner_and_resize_phase_work() {
    let mut app = frame_app(100);
    let mut terminal = Terminal::new(InstrumentedBackend::new(TestBackend::new(WIDTH, HEIGHT)))
        .expect("test terminal");

    let cold = draw_frame(&mut app, &mut terminal);
    let expected_items = u64::try_from(app.model.conversation.timeline.items().len()).unwrap();
    assert_eq!(cold.assemble_calls, 1);
    assert_eq!(cold.assemble_source_items, expected_items);
    assert_eq!(cold.viewport_render_calls, 1);
    assert_eq!(
        cold.viewport_source_lines,
        u64::try_from(app.output_area.document().total_lines()).unwrap()
    );
    assert!(cold.viewport_visible_lines > 0);
    assert_eq!(cold.terminal_draw_calls, 1);
    assert_eq!(cold.terminal_diff_calls, 1);
    assert!(cold.terminal_diff_cells > 0);
    assert_eq!(cold.backend_flush_calls, 1);

    let warm = draw_frame(&mut app, &mut terminal);
    assert_eq!(warm.assemble_calls, 0);
    assert_eq!(warm.viewport_render_calls, 1);
    assert_eq!(warm.terminal_diff_calls, 1);
    assert!(warm.terminal_diff_cells <= cold.terminal_diff_cells);
    assert_eq!(warm.backend_flush_calls, 1);

    terminal.backend_mut().inner_mut().resize(120, HEIGHT);
    terminal
        .resize(Rect::new(0, 0, 120, HEIGHT))
        .expect("resize terminal buffers");
    app.handle_resize(120, HEIGHT);
    let resized = draw_frame(&mut app, &mut terminal);
    assert_eq!(resized.assemble_calls, 0, "revision 不变应复用 view model");
    assert_eq!(
        resized.document_render_calls, 1,
        "resize 应按新宽度重建 document"
    );
    assert_eq!(resized.viewport_render_calls, 1);
    assert_eq!(resized.backend_flush_calls, 1);
}

#[test]
#[ignore = "性能基线；手动运行：cargo test -p cli --release tui_frame_phase_release_workload -- --ignored --nocapture"]
#[allow(clippy::print_stdout)]
fn tui_frame_phase_release_workload() {
    println!("\n=== #1418 TUI 帧阶段性能基线（samples={SAMPLES}）===");
    for blocks in [100usize, 500, 1000] {
        let mut assemble_samples = Vec::with_capacity(SAMPLES);
        let mut viewport_samples = Vec::with_capacity(SAMPLES);
        let mut draw_samples = Vec::with_capacity(SAMPLES);
        let mut diff_samples = Vec::with_capacity(SAMPLES);
        let mut flush_samples = Vec::with_capacity(SAMPLES);
        let mut wall_samples = Vec::with_capacity(SAMPLES);
        let mut representative = RenderPerformanceSnapshot::default();

        for sample in 0..SAMPLES {
            let mut app = frame_app(blocks);
            let mut terminal =
                Terminal::new(InstrumentedBackend::new(TestBackend::new(WIDTH, HEIGHT)))
                    .expect("test terminal");
            let started = Instant::now();
            let metrics = draw_frame(&mut app, &mut terminal);
            wall_samples.push(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
            assemble_samples.push(metrics.assemble_ns);
            viewport_samples.push(metrics.viewport_render_ns);
            draw_samples.push(metrics.terminal_draw_ns);
            diff_samples.push(metrics.terminal_diff_ns);
            flush_samples.push(metrics.backend_flush_ns);
            if sample == 0 {
                representative = metrics;
            }
        }

        let (assemble_p50, assemble_p95) = percentiles_ns(&assemble_samples).unwrap();
        let (viewport_p50, viewport_p95) = percentiles_ns(&viewport_samples).unwrap();
        let (draw_p50, draw_p95) = percentiles_ns(&draw_samples).unwrap();
        let (diff_p50, diff_p95) = percentiles_ns(&diff_samples).unwrap();
        let (flush_p50, flush_p95) = percentiles_ns(&flush_samples).unwrap();
        let (wall_p50, wall_p95) = percentiles_ns(&wall_samples).unwrap();

        println!(
            "blocks={blocks:>4} source_items={} roots={} source_lines={} visible_lines={} diff_cells={} | wall_p50/p95={:.3}/{:.3}ms assemble={:.3}/{:.3}ms viewport={:.3}/{:.3}ms draw={:.3}/{:.3}ms diff={:.3}/{:.3}ms flush={:.3}/{:.3}ms",
            representative.assemble_source_items,
            representative.assemble_output_roots,
            representative.viewport_source_lines,
            representative.viewport_visible_lines,
            representative.terminal_diff_cells,
            wall_p50 as f64 / 1_000_000.0,
            wall_p95 as f64 / 1_000_000.0,
            assemble_p50 as f64 / 1_000_000.0,
            assemble_p95 as f64 / 1_000_000.0,
            viewport_p50 as f64 / 1_000_000.0,
            viewport_p95 as f64 / 1_000_000.0,
            draw_p50 as f64 / 1_000_000.0,
            draw_p95 as f64 / 1_000_000.0,
            diff_p50 as f64 / 1_000_000.0,
            diff_p95 as f64 / 1_000_000.0,
            flush_p50 as f64 / 1_000_000.0,
            flush_p95 as f64 / 1_000_000.0,
        );
    }
}
