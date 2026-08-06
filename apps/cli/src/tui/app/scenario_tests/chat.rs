use crate::tui::adapter::runtime_view::{TuiChatMessage, TuiResumedSessionStep};
use crate::tui::adapter::tui_runtime_event::{
    TuiActivityAudience, TuiActivityChangeKind, TuiActivityDetail, TuiActivityKind,
    TuiActivityObservation, TuiActivitySnapshot, TuiActivitySource, TuiActivityState,
    TuiActivityTiming, TuiCompactStage, TuiCompactWork, TuiHookPoint, TuiInteractionKind,
    TuiModelStreamState, TuiRunContext, TuiRunPhaseKind, TuiRunPurpose, TuiRuntimeEvent,
    UiActivityId,
};
use crate::tui::model::conversation::interaction::UiRunId;

use super::super::testing::TuiScenarioHarness;

fn ctx() -> TuiRunContext {
    TuiRunContext {
        chat_id: "chat-p0".to_string(),
        run_id: "turn-p0".to_string(),
    }
}

fn normalize_completed_terminal_verb(screen: &str) -> String {
    const DONE_VERBS: [&str; 20] = [
        "Sautéed",
        "Baked",
        "Grilled",
        "Simmered",
        "Roasted",
        "Brewed",
        "Toasted",
        "Stewed",
        "Marinated",
        "Charred",
        "Poached",
        "Steamed",
        "Smoked",
        "Brûléed",
        "Flambéed",
        "Fermented",
        "Pickled",
        "Cured",
        "Seared",
        "Blanched",
    ];
    DONE_VERBS
        .iter()
        .fold(screen.to_string(), |normalized, verb| {
            normalized.replace(verb, "CompletedVerb")
        })
}

struct ActivityFixture<'a> {
    id: &'a str,
    revision: u64,
    parent_activity_id: Option<&'a str>,
    source: TuiActivitySource,
    kind: TuiActivityKind,
    state: TuiActivityState,
    detail: TuiActivityDetail,
    audience: TuiActivityAudience,
    total_elapsed_ms: u64,
    state_elapsed_ms: u64,
}

fn activity(fixture: ActivityFixture<'_>) -> TuiActivityObservation {
    TuiActivityObservation {
        id: UiActivityId::from(fixture.id),
        run_id: UiRunId::from("main-activity-run"),
        run_step_id: None,
        parent_activity_id: fixture.parent_activity_id.map(UiActivityId::from),
        source: fixture.source,
        kind: fixture.kind,
        state: fixture.state,
        detail: fixture.detail,
        audience: fixture.audience,
        revision: fixture.revision,
        timing: TuiActivityTiming {
            total_elapsed_ms: fixture.total_elapsed_ms,
            active_elapsed_ms: fixture.total_elapsed_ms,
            state_elapsed_ms: fixture.state_elapsed_ms,
            started_at_unix_ms: Some(1_000),
            finished_at_unix_ms: None,
        },
    }
}

fn root_activity(revision: u64, state: TuiActivityState) -> TuiActivityObservation {
    activity(ActivityFixture {
        id: "root",
        revision,
        parent_activity_id: None,
        source: TuiActivitySource::Run,
        kind: TuiActivityKind::Run,
        state,
        detail: TuiActivityDetail::Run {
            purpose: TuiRunPurpose::Main,
        },
        audience: TuiActivityAudience::User,
        total_elapsed_ms: revision * 1_000,
        state_elapsed_ms: revision * 1_000,
    })
}

fn activity_screen(harness: &mut TuiScenarioHarness) -> String {
    harness.render();
    harness.screen()
}

fn assert_single_activity_summary(screen: &str, expected: &str) {
    assert!(
        screen.contains(expected),
        "活动摘要应显示 {expected}，实际屏幕：\n{screen}"
    );
    assert_eq!(
        screen.matches(expected).count(),
        1,
        "活动摘要必须唯一，实际屏幕：\n{screen}"
    );
}

#[test]
fn activity_pipeline_renders_one_low_noise_main_summary_until_terminal() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let observations = [
        root_activity(1, TuiActivityState::Running),
        activity(ActivityFixture {
            id: "context",
            revision: 2,
            parent_activity_id: Some("root"),
            source: TuiActivitySource::Run,
            kind: TuiActivityKind::RunPhase(TuiRunPhaseKind::PreparingContext),
            state: TuiActivityState::Running,
            detail: TuiActivityDetail::Phase {
                phase: TuiRunPhaseKind::PreparingContext,
            },
            audience: TuiActivityAudience::User,
            total_elapsed_ms: 1_000,
            state_elapsed_ms: 1_000,
        }),
        activity(ActivityFixture {
            id: "model",
            revision: 3,
            parent_activity_id: Some("root"),
            source: TuiActivitySource::ModelInvocation("model-1".into()),
            kind: TuiActivityKind::ModelInvocation,
            state: TuiActivityState::Running,
            detail: TuiActivityDetail::Model {
                model: "model-private".into(),
                attempt: 4,
                stream: TuiModelStreamState::WaitingForFirstToken,
            },
            audience: TuiActivityAudience::User,
            total_elapsed_ms: 2_000,
            state_elapsed_ms: 2_000,
        }),
        activity(ActivityFixture {
            id: "tool",
            revision: 4,
            parent_activity_id: Some("root"),
            source: TuiActivitySource::ToolCall("tool-1".into()),
            kind: TuiActivityKind::ToolCall,
            state: TuiActivityState::Running,
            detail: TuiActivityDetail::Tool {
                name: "Read".into(),
                summary: Some("Cargo.toml".into()),
                parallel_count: 3,
            },
            audience: TuiActivityAudience::User,
            total_elapsed_ms: 3_000,
            state_elapsed_ms: 3_000,
        }),
        activity(ActivityFixture {
            id: "hook",
            revision: 5,
            parent_activity_id: Some("root"),
            source: TuiActivitySource::HookDispatch(UiActivityId::from("hook-source")),
            kind: TuiActivityKind::HookDispatch,
            state: TuiActivityState::Running,
            detail: TuiActivityDetail::Hook {
                point: TuiHookPoint::Stop,
                script: "check-agent-stop.sh".to_string(),
                attempt: 9,
            },
            audience: TuiActivityAudience::Operational,
            total_elapsed_ms: 4_000,
            state_elapsed_ms: 4_000,
        }),
        activity(ActivityFixture {
            id: "compact",
            revision: 6,
            parent_activity_id: Some("root"),
            source: TuiActivitySource::Compaction(UiActivityId::from("compact-source")),
            kind: TuiActivityKind::Compaction,
            state: TuiActivityState::Running,
            detail: TuiActivityDetail::Compact {
                stage: TuiCompactStage::Mapping,
                work: TuiCompactWork::Determinate {
                    completed: 37,
                    total: 100,
                },
            },
            audience: TuiActivityAudience::Operational,
            total_elapsed_ms: 5_000,
            state_elapsed_ms: 5_000,
        }),
        activity(ActivityFixture {
            id: "interaction",
            revision: 7,
            parent_activity_id: Some("root"),
            source: TuiActivitySource::Interaction("interaction-1".into()),
            kind: TuiActivityKind::Interaction,
            state: TuiActivityState::Waiting,
            detail: TuiActivityDetail::Interaction {
                kind: TuiInteractionKind::UserQuestion,
            },
            audience: TuiActivityAudience::User,
            total_elapsed_ms: 6_000,
            state_elapsed_ms: 6_000,
        }),
    ];

    for observation in observations.iter().take(4).cloned() {
        harness.runtime_event(TuiRuntimeEvent::ActivityChanged {
            kind: TuiActivityChangeKind::Updated,
            activity: observation,
        });
    }
    let screen = activity_screen(&mut harness);
    assert_single_activity_summary(&screen, "Calling tools…");
    assert!(screen.contains("Running 3 tools"));

    for observation in observations.iter().skip(4).cloned() {
        harness.runtime_event(TuiRuntimeEvent::ActivityChanged {
            kind: TuiActivityChangeKind::Updated,
            activity: observation,
        });
    }
    let interaction_screen = activity_screen(&mut harness);
    assert_single_activity_summary(&interaction_screen, "Waiting for input…");
    for hidden in ["attempt", "retry", "37", "100", "model-private"] {
        assert!(
            !interaction_screen.contains(hidden),
            "低噪声摘要不得暴露 {hidden}，实际屏幕：\n{interaction_screen}"
        );
    }

    harness.runtime_event(TuiRuntimeEvent::ActivityChanged {
        kind: TuiActivityChangeKind::Started,
        activity: activity(ActivityFixture {
            id: "child",
            revision: 8,
            parent_activity_id: Some("root"),
            source: TuiActivitySource::ChildRun(UiRunId::from("sub-run")),
            kind: TuiActivityKind::ChildRun,
            state: TuiActivityState::Running,
            detail: TuiActivityDetail::ChildRun {
                role: "explore".into(),
                model: "sub-model".into(),
            },
            audience: TuiActivityAudience::User,
            total_elapsed_ms: 7_000,
            state_elapsed_ms: 7_000,
        }),
    });
    let child_screen = activity_screen(&mut harness);
    assert_single_activity_summary(&child_screen, "Running agent…");
    assert!(child_screen.contains("Running explore"));

    harness.runtime_event(TuiRuntimeEvent::ActivitySnapshot(TuiActivitySnapshot {
        run_id: UiRunId::from("main-activity-run"),
        revision: 9,
        activities: vec![root_activity(9, TuiActivityState::Succeeded)],
    }));
    let terminal_screen = activity_screen(&mut harness);
    assert!(!terminal_screen.contains("Running agent…"));
    assert!(!terminal_screen.contains("Running explore"));
    harness.assert_idle();
}

#[test]
fn activity_revision_gap_hides_summary_until_snapshot_repairs_mirror() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.runtime_event(TuiRuntimeEvent::ActivityChanged {
        kind: TuiActivityChangeKind::Started,
        activity: root_activity(1, TuiActivityState::Running),
    });
    harness.runtime_event(TuiRuntimeEvent::ActivityChanged {
        kind: TuiActivityChangeKind::Updated,
        activity: activity(ActivityFixture {
            id: "context",
            revision: 2,
            parent_activity_id: Some("root"),
            source: TuiActivitySource::Run,
            kind: TuiActivityKind::RunPhase(TuiRunPhaseKind::PreparingContext),
            state: TuiActivityState::Running,
            detail: TuiActivityDetail::Phase {
                phase: TuiRunPhaseKind::PreparingContext,
            },
            audience: TuiActivityAudience::User,
            total_elapsed_ms: 2_000,
            state_elapsed_ms: 2_000,
        }),
    });
    assert_single_activity_summary(&activity_screen(&mut harness), "Preparing context…");

    harness.runtime_event(TuiRuntimeEvent::ActivityChanged {
        kind: TuiActivityChangeKind::Updated,
        activity: activity(ActivityFixture {
            id: "model",
            revision: 4,
            parent_activity_id: Some("root"),
            source: TuiActivitySource::ModelInvocation("model-gap".into()),
            kind: TuiActivityKind::ModelInvocation,
            state: TuiActivityState::Running,
            detail: TuiActivityDetail::Model {
                model: "model-gap".into(),
                attempt: 1,
                stream: TuiModelStreamState::Streaming,
            },
            audience: TuiActivityAudience::User,
            total_elapsed_ms: 4_000,
            state_elapsed_ms: 4_000,
        }),
    });
    let stale_screen = activity_screen(&mut harness);
    assert!(!stale_screen.contains("Preparing context…"));
    assert!(!stale_screen.contains("Thinking…"));

    harness.runtime_event(TuiRuntimeEvent::ActivitySnapshot(TuiActivitySnapshot {
        run_id: UiRunId::from("main-activity-run"),
        revision: 4,
        activities: vec![
            root_activity(1, TuiActivityState::Running),
            activity(ActivityFixture {
                id: "model",
                revision: 4,
                parent_activity_id: Some("root"),
                source: TuiActivitySource::ModelInvocation("model-gap".into()),
                kind: TuiActivityKind::ModelInvocation,
                state: TuiActivityState::Running,
                detail: TuiActivityDetail::Model {
                    model: "model-gap".into(),
                    attempt: 1,
                    stream: TuiModelStreamState::Streaming,
                },
                audience: TuiActivityAudience::User,
                total_elapsed_ms: 4_000,
                state_elapsed_ms: 4_000,
            }),
        ],
    }));
    assert_single_activity_summary(&activity_screen(&mut harness), "Thinking…");
    harness.assert_idle();
}

#[test]
fn live_and_resumed_compact_render_one_persistent_notice_without_chat_message() {
    let notice = "✓ 上下文压缩完成";

    let mut live = TuiScenarioHarness::new(100, 30);
    live.runtime_event(TuiRuntimeEvent::CompactFinished {
        messages: vec![],
        notice: notice.into(),
    });
    live.render();

    let mut resumed = TuiScenarioHarness::new(100, 30);
    resumed.runtime_event(TuiRuntimeEvent::SessionResumed {
        steps: vec![],
        display_history: None,
        session_id: "session-compact".into(),
        created_at: 0,
        compacted: true,
    });
    resumed.render();

    let rendered_notice = "✓ 上 下 文 压 缩 完 成";
    assert_eq!(live.screen().matches(rendered_notice).count(), 1);
    assert_eq!(resumed.screen().matches(rendered_notice).count(), 1);
}

#[test]
fn terminal_text_after_thinking_matches_resume_projection() {
    let terminal_text = "Final answer survives the terminal event.";

    let mut live = TuiScenarioHarness::new(100, 30);
    live.runtime_event(TuiRuntimeEvent::TurnStarted { messages: vec![] });
    live.runtime_event(TuiRuntimeEvent::Thinking {
        context: ctx(),
        text: "Inspecting the repository".into(),
    });
    live.runtime_event(TuiRuntimeEvent::BlockComplete {
        context: ctx(),
        text: String::new(),
    });
    live.runtime_event(TuiRuntimeEvent::Text {
        context: ctx(),
        text: terminal_text.into(),
    });
    live.runtime_event(TuiRuntimeEvent::BlockComplete {
        context: ctx(),
        text: String::new(),
    });
    live.runtime_event(TuiRuntimeEvent::Done {
        context: ctx(),
        duration_ms: None,
    });
    live.render();

    let mut resumed = TuiScenarioHarness::new(100, 30);
    resumed.runtime_event(TuiRuntimeEvent::SessionResumed {
        display_history: None,
        steps: vec![TuiResumedSessionStep {
            run_id: "chat-p0".into(),
            step_id: "turn-p0".into(),
            messages: vec![TuiChatMessage::assistant_text(terminal_text)],
            finalize_cause: None,
            duration_ms: None,
        }],
        session_id: "session-p0".into(),
        created_at: 0,
        compacted: false,
    });
    resumed.render();

    assert!(live.screen().contains(terminal_text));
    assert!(resumed.screen().contains(terminal_text));
    assert_eq!(live.screen().matches(terminal_text).count(), 1);
    assert_eq!(resumed.screen().matches(terminal_text).count(), 1);
}

#[test]
fn live_completed_turn_renders_terminal_notice() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.runtime_event(TuiRuntimeEvent::TurnStarted { messages: vec![] });
    harness.runtime_event(TuiRuntimeEvent::Text {
        context: ctx(),
        text: "The result is ready.".into(),
    });
    harness.runtime_event(TuiRuntimeEvent::BlockComplete {
        context: ctx(),
        text: "The result is ready.".into(),
    });
    harness.runtime_event(TuiRuntimeEvent::Done {
        context: ctx(),
        duration_ms: Some(125_000),
    });
    harness.render();

    let screen = harness.screen();
    assert!(screen.contains("The result is ready."));
    assert!(
        screen.contains("for 2m 5s"),
        "实时正常完成必须显示终态耗时提示，实际屏幕：\n{screen}"
    );
    harness.assert_idle();
}

#[test]
fn live_cancelled_turn_renders_terminal_notice() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.runtime_event(TuiRuntimeEvent::TurnStarted { messages: vec![] });
    harness.runtime_event(TuiRuntimeEvent::Cancelled {
        context: ctx(),
        duration_ms: 125_000,
    });
    harness.render();

    let screen = harness.screen();
    assert!(
        screen.contains("✻ Cancelled, ran 2m 5s"),
        "实时取消必须显示统一终态提示，实际屏幕：\n{screen}"
    );
    harness.assert_idle();
}

#[test]
fn authoritative_cancelled_terminal_never_renders_completed_verb() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.runtime_event(TuiRuntimeEvent::TurnStarted { messages: vec![] });
    harness.runtime_event(TuiRuntimeEvent::Cancelled {
        context: ctx(),
        duration_ms: 6_000,
    });
    harness.render();

    let screen = harness.screen();
    assert!(screen.contains("✻ Cancelled, ran 6s"));
    for verb in [
        "Sautéed",
        "Baked",
        "Grilled",
        "Simmered",
        "Roasted",
        "Brewed",
        "Toasted",
        "Stewed",
        "Marinated",
        "Charred",
        "Poached",
        "Steamed",
        "Smoked",
        "Brûléed",
        "Flambéed",
        "Fermented",
        "Pickled",
        "Cured",
        "Seared",
        "Blanched",
    ] {
        assert!(
            !screen.contains(verb),
            "权威取消终态不得渲染正常完成动词 {verb}，实际屏幕：\n{screen}"
        );
    }
    harness.assert_idle();
}

#[test]
fn streaming_has_representative_thinking_and_completed_snapshots() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.runtime_event(TuiRuntimeEvent::TurnStarted { messages: vec![] });
    harness.runtime_event(TuiRuntimeEvent::Thinking {
        context: ctx(),
        text: "Inspecting the repository".into(),
    });
    harness.render();
    let thinking_screen = harness.screen();
    assert!(thinking_screen.contains("Inspecting the repository"));
    assert!(!thinking_screen.contains("Thinking…"));

    harness.runtime_event(TuiRuntimeEvent::Text {
        context: ctx(),
        text: "The result is ready.".into(),
    });
    harness.runtime_event(TuiRuntimeEvent::BlockComplete {
        context: ctx(),
        text: "The result is ready.".into(),
    });
    harness.runtime_event(TuiRuntimeEvent::Done {
        context: ctx(),
        duration_ms: Some(5_000),
    });
    harness.render();
    let completed_screen = harness.screen();
    assert!(completed_screen.contains("The result is ready."));
    assert!(completed_screen.contains(" for 5s"));
    assert!(!completed_screen.contains("Completed"));
    insta::assert_snapshot!(
        "chat_streaming__completed__100x30",
        normalize_completed_terminal_verb(&completed_screen)
    );
    harness.assert_idle();
}

#[test]
fn tool_lifecycle_binds_result_to_call_and_renders_stable_states() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let id = "read-1".to_string();
    harness.runtime_event(TuiRuntimeEvent::ToolCallStart {
        context: ctx(),
        id: id.clone(),
        provider_id: Some("provider-read-1".into()),
        name: "Read".into(),
        index: 0,
    });
    harness.runtime_event(TuiRuntimeEvent::ToolCallUpdate {
        context: ctx(),
        id: id.clone(),
        provider_id: Some("provider-read-1".into()),
        name: "Read".into(),
        index: 0,
        arguments_delta: None,
        arguments: Some(serde_json::json!({"file_path":"Cargo.toml"})),
        status: crate::tui::adapter::tui_runtime_event::TuiToolCallStatus::Ready,
    });
    harness.render();
    assert!(harness.screen().contains("Read"));
    insta::assert_snapshot!("tool_read__running__100x30", harness.screen());

    harness.runtime_event(TuiRuntimeEvent::ToolResult {
        context: ctx(),
        id,
        provider_id: "provider-read-1".into(),
        tool_name: "Read".into(),
        output: "[workspace]\nmembers = []".into(),
        content: serde_json::json!({"text":"[workspace]\nmembers = []"}),
        is_error: false,
        images: vec![],
    });
    harness.render();
    assert!(harness.screen().contains("Read"));
    insta::assert_snapshot!("tool_read__completed__100x30", harness.screen());
    harness.assert_idle();
}

#[test]
fn oversized_unknown_tool_result_renders_truncation_notice() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let id = "unknown-large-1".to_string();
    harness.runtime_event(TuiRuntimeEvent::ToolCallStart {
        context: ctx(),
        id: id.clone(),
        provider_id: Some("provider-unknown-large-1".into()),
        name: "UnknownTool".into(),
        index: 0,
    });
    harness.runtime_event(TuiRuntimeEvent::ToolResult {
        context: ctx(),
        id,
        provider_id: "provider-unknown-large-1".into(),
        tool_name: "UnknownTool".into(),
        output: "<persisted-output>\nOutput too large. Full output unavailable because persistence failed.\n\n--- head (15 chars) ---\nvisible preview\n\n[... 2499999985 chars omitted ...]\n\n--- tail (0 chars) ---\n\n</persisted-output>".into(),
        content: serde_json::json!({
            "text": "<persisted-output>\nOutput too large. Full output unavailable because persistence failed.\n\n--- head (15 chars) ---\nvisible preview\n\n[... 2499999985 chars omitted ...]\n\n--- tail (0 chars) ---\n\n</persisted-output>",
            "truncated": true,
            "original_chars": 2_500_000_000usize,
            "original_bytes": 2_500_000_000usize,
            "omitted_chars": 2_499_999_985usize,
            "blob": {
                "status": "unavailable",
                "reason": "write_failed"
            }
        }),
        is_error: false,
        images: vec![],
    });

    harness.render();

    let screen = harness.screen();
    assert!(screen.contains("UnknownTool"));
    assert!(screen.contains("Output too large"));
    assert!(screen.contains("persistence failed"));
    assert!(!screen.contains("FULL_PAYLOAD_SENTINEL"));
    harness.assert_idle();
}

/// #1106 回归：runtime 允许发空 SystemMessage（hook 的 additional_context /
/// system_message 只判 Option 不判空串），TUI 必须不渲染——否则每条空消息
/// 各吃掉 2 行（空内容 + depth0 前置空行），在输出区堆出大片空行。
///
/// 端到端：runtime 事件 → ACL → model → view_assembler → render → 屏幕字符。
#[test]
fn empty_system_messages_from_runtime_do_not_accumulate_blank_lines() {
    fn blanks_between_anchors(empty_count: usize) -> usize {
        let mut harness = TuiScenarioHarness::new(60, 30);
        for anchor in ["ANCHORUP", "ANCHORDOWN"] {
            if anchor == "ANCHORDOWN" {
                for payload in ["", "<system-reminder></system-reminder>"]
                    .iter()
                    .cycle()
                    .take(empty_count)
                {
                    harness.runtime_event(TuiRuntimeEvent::SystemMessage((*payload).to_string()));
                }
            }
            harness.runtime_event(TuiRuntimeEvent::Text {
                context: ctx(),
                text: anchor.to_string(),
            });
            harness.runtime_event(TuiRuntimeEvent::BlockComplete {
                context: ctx(),
                text: anchor.to_string(),
            });
        }
        harness.runtime_event(TuiRuntimeEvent::Done {
            context: ctx(),
            duration_ms: None,
        });
        harness.render();

        let screen = harness.screen();
        let lines: Vec<&str> = screen.lines().collect();
        let up = lines
            .iter()
            .position(|l| l.contains("ANCHORUP"))
            .expect("上锚点应在屏幕上");
        let down = lines
            .iter()
            .position(|l| l.contains("ANCHORDOWN"))
            .expect("下锚点应在屏幕上");
        lines[up + 1..down]
            .iter()
            .filter(|l| l.trim().is_empty())
            .count()
    }

    let baseline = blanks_between_anchors(0);
    for empty_count in [1usize, 4, 8] {
        assert_eq!(
            blanks_between_anchors(empty_count),
            baseline,
            "{empty_count} 条空 SystemMessage 不应新增空行（基线 {baseline}）"
        );
    }
}

#[test]
fn chat_retry_after_partial_preserves_output_append_only() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.runtime_event(TuiRuntimeEvent::TurnStarted { messages: vec![] });
    harness.runtime_event(TuiRuntimeEvent::Text {
        context: ctx(),
        text: "partial before interruption".into(),
    });
    harness.runtime_event(TuiRuntimeEvent::BlockComplete {
        context: ctx(),
        text: "partial before interruption".into(),
    });
    harness.runtime_event(TuiRuntimeEvent::ModelInvocationRetrying {
        context: ctx(),
        attempt: 2,
        delay_ms: 10_000,
    });
    harness.runtime_event(TuiRuntimeEvent::Text {
        context: ctx(),
        text: "replacement after retry".into(),
    });
    harness.runtime_event(TuiRuntimeEvent::BlockComplete {
        context: ctx(),
        text: "replacement after retry".into(),
    });
    harness.runtime_event(TuiRuntimeEvent::Done {
        context: ctx(),
        duration_ms: Some(5_000),
    });
    harness.render();

    let screen = harness.screen();
    let partial = screen
        .find("partial before interruption")
        .expect("partial output should remain visible");
    let retry = screen
        .find("Retrying model invocation (attempt 2) in 10.0s.")
        .expect("retry notice should be visible");
    let replacement = screen
        .find("replacement after retry")
        .expect("replacement output should be visible");
    assert!(partial < retry && retry < replacement);
    assert!(!screen.contains("rollback"));
    assert!(screen.contains(" for 5s"));
    assert!(!screen.contains("Completed"));
    insta::assert_snapshot!(
        "chat_retry_after_partial__100x30",
        normalize_completed_terminal_verb(&screen)
    );
    harness.assert_idle();
}
