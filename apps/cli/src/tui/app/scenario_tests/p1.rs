use crate::tui::app::event::UiEvent;
use crate::tui::effect::effect::Effect;

use super::super::testing::{ExpectedEffect, TuiScenarioHarness};

#[test]
fn resize_and_tiny_terminal_reflow_without_panicking() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.ui(UiEvent::SystemMessage(
        "a deliberately long status message that must wrap on a narrow terminal".into(),
    ));
    harness.resize(40, 12);
    harness.render();
    assert_eq!(harness.app.layout.last_terminal_size.unwrap().width, 40);
    assert!(harness.screen().contains("Aemeath"));

    harness.resize(8, 4);
    harness.render();
    assert_eq!(harness.app.layout.last_terminal_size.unwrap().height, 4);
    assert!(!harness.screen().is_empty());
    harness.assert_idle();
}

#[test]
fn busy_paste_classifies_text_empty_and_image_without_real_clipboard() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();

    harness.paste("queued text");
    assert_eq!(harness.input_text(), "queued text");
    assert!(!harness.app.input.just_pasted);

    harness.expect_effect(ExpectedEffect::ReadClipboardImage);
    harness.paste("   ");
    assert!(harness
        .effects()
        .iter()
        .any(|effect| matches!(effect, Effect::ReadClipboardImage)));

    harness.expect_effect(ExpectedEffect::ProcessImageFile {
        path: "/tmp/p1-fixture.png".into(),
    });
    harness.paste(" /tmp/p1-fixture.png ");
    assert!(harness.effects().iter().any(
        |effect| matches!(effect, Effect::ProcessImageFile { path } if path == "/tmp/p1-fixture.png")
    ));
    harness.assert_idle();
}

#[test]
fn error_and_compact_progress_converge_through_frame_driver() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.ui(UiEvent::CompactProgress {
        stage: "Summarizing".into(),
        current: Some(2),
        total: Some(4),
    });
    assert!(harness
        .app
        .model
        .conversation
        .runtime
        .compact_progress
        .is_some());
    assert!(harness.app.view_state.dirty.output);
    harness.render();
    assert_eq!(
        harness
            .app
            .model
            .conversation
            .runtime
            .compact_progress
            .as_ref()
            .map(|progress| progress.stage.as_str()),
        Some("Summarizing")
    );

    harness.ui(UiEvent::Error("provider unavailable".into()));
    assert!(!harness.app.model.conversation.runtime.spinner.chat_active);
    assert!(harness.effects().iter().any(
        |effect| matches!(effect, Effect::RunHook { name, message } if name == "error" && message == "provider unavailable")
    ));
    harness.assert_idle();
}

#[test]
fn working_directory_change_updates_status_projection() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let root = std::path::Path::new("/workspace");
    let worktree = root.join("feature-one");
    harness.ui(crate::tui::app::status_context_for_paths(&worktree, root));
    harness.render();

    let expected_path_base = worktree.display().to_string();
    let expected_workspace_root = root.display().to_string();
    assert_eq!(
        harness.app.model.workspace_provider.path_base(),
        Some(expected_path_base.as_str())
    );
    assert_eq!(
        harness.app.model.workspace_provider.workspace_root(),
        Some(expected_workspace_root.as_str())
    );
    assert!(harness.screen().contains("feature-one"));
    harness.assert_idle();
}
