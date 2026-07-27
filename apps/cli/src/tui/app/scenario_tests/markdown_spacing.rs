use super::super::testing::TuiScenarioHarness;

fn assistant_events(text: &str) -> Vec<sdk::ChatEvent> {
    let context = sdk::ChatEventContext::new(
        sdk::ChatId::new("chat-spacing"),
        sdk::ChatTurnId::new("turn-spacing"),
    );
    vec![
        sdk::ChatEvent::Token {
            context: context.clone(),
            text: text.to_string(),
        },
        sdk::ChatEvent::BlockComplete {
            context: context.clone(),
            text: text.to_string(),
        },
        sdk::ChatEvent::Done { context },
    ]
}

fn blank_lines_between(screen: &str, first: &str, second: &str) -> usize {
    let lines = screen.lines().collect::<Vec<_>>();
    let first = lines
        .iter()
        .position(|line| line.contains(first))
        .expect("first anchor");
    let second = lines
        .iter()
        .position(|line| line.contains(second))
        .expect("second anchor");
    lines[first + 1..second]
        .iter()
        .filter(|line| line.trim().is_empty())
        .count()
}

#[test]
fn sdk_config_reload_switches_existing_markdown_from_normal_to_compact() {
    let mut harness = TuiScenarioHarness::new(80, 30);
    harness.sdk_runtime_batch(assistant_events("FIRST\n\nSECOND"));
    harness.render();
    let normal = harness.screen();
    assert_eq!(blank_lines_between(&normal, "FIRST", "SECOND"), 1);

    harness.sdk_runtime_batch([sdk::ChatEvent::ConfigReloaded {
        event: sdk::ConfigReloadedEvent {
            changed_keys: vec!["config:scope:immediate".to_string()],
            scopes: vec![sdk::ConfigApplicationScopeView::Immediate],
            view: sdk::ConfigView {
                markdown_spacing: sdk::MarkdownSpacingModeView::Compact,
                ..Default::default()
            },
        },
    }]);
    harness.render();
    let compact = harness.screen();

    assert_eq!(blank_lines_between(&compact, "FIRST", "SECOND"), 0);
    assert_eq!(
        harness.app.model.ui_preferences.markdown_spacing().mode(),
        crate::tui::render::output::spacing::MarkdownSpacingMode::Compact
    );
}

#[test]
fn compact_keeps_fence_internal_blank_line() {
    let mut harness = TuiScenarioHarness::new(80, 30);
    harness.sdk_runtime_batch([sdk::ChatEvent::ConfigReloaded {
        event: sdk::ConfigReloadedEvent {
            changed_keys: vec!["config:scope:immediate".to_string()],
            scopes: vec![sdk::ConfigApplicationScopeView::Immediate],
            view: sdk::ConfigView {
                markdown_spacing: sdk::MarkdownSpacingModeView::Compact,
                ..Default::default()
            },
        },
    }]);
    harness.sdk_runtime_batch(assistant_events("```\nCODE_ONE\n\nCODE_TWO\n```"));
    harness.render();

    assert_eq!(
        blank_lines_between(&harness.screen(), "CODE_ONE", "CODE_TWO"),
        1
    );
}
