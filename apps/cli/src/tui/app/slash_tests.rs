use super::App;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

#[allow(dead_code)]
pub(super) struct BlockingReflectionClient {
    pub(super) started_tx: Mutex<Option<oneshot::Sender<()>>>,
    pub(super) finish_rx: Mutex<Option<oneshot::Receiver<()>>>,
}

#[async_trait]
impl sdk::AgentClient for BlockingReflectionClient {
    fn cancel_run(&self, _run_id: &sdk::RunId) -> sdk::CancelRunOutcome {
        sdk::CancelRunOutcome::NotFound
    }

    async fn chat(&self, _input: sdk::ChatRequest) -> Result<sdk::ChatStream, sdk::SdkError> {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Ok(sdk::ChatStream::new(rx))
    }
}

pub(super) fn app_with_blocking_reflection_client(
) -> (App, oneshot::Receiver<()>, oneshot::Sender<()>) {
    let (app, started_rx, finish_tx, _) = app_with_blocking_reflection_client_handle();
    (app, started_rx, finish_tx)
}

pub(super) fn app_with_blocking_reflection_client_handle() -> (
    App,
    oneshot::Receiver<()>,
    oneshot::Sender<()>,
    Arc<BlockingReflectionClient>,
) {
    let (started_tx, started_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = oneshot::channel();
    let client = Arc::new(BlockingReflectionClient {
        started_tx: Mutex::new(Some(started_tx)),
        finish_rx: Mutex::new(Some(finish_rx)),
    });
    let mut app = App::new(
        "test-session".to_string(),
        std::env::temp_dir(),
        "test-model".to_string(),
    );
    app.agent_client = Some(client.clone());
    (app, started_rx, finish_tx, client)
}

fn system_texts(app: &App) -> Vec<&str> {
    app.model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            crate::tui::model::output_timeline::OutputTimelineItem::System { text, .. } => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect()
}

fn apply_ui_event(app: &mut App, event: super::event::UiEvent) {
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let spawn_refs =
        crate::tui::effect::session::processing::SpawnContextRefs { agent_client: None };
    app.update(crate::tui::update::msg::TuiMsg::Ui(event), &tx, &spawn_refs);
}

#[test]
fn reflection_history_displays_safe_metadata_without_body() {
    let mut app = App::new(
        "test-session".to_string(),
        std::env::temp_dir(),
        "test-model".to_string(),
    );
    apply_ui_event(
        &mut app,
        super::event::UiEvent::ReflectionHistory {
            records: vec![sdk::ReflectionHistoryView {
                id: "reflection-secret-body-must-not-appear".to_string(),
                timestamp: 1_700_000_000,
                trigger: sdk::ReflectionTriggerView::Manual,
                status: sdk::ReflectionStatusView::Failed,
                deviations: 2,
                suggestions: 3,
                outdated: 1,
                apply_status: sdk::ReflectionApplyStatusView::PartiallyApplied,
                error_category: Some(sdk::ReflectionErrorCategoryView::Parse),
                token_usage: Some(sdk::ReflectionTokenUsageView {
                    input_tokens: 11,
                    output_tokens: 7,
                }),
                duration_ms: 432,
            }],
        },
    );

    let rendered = system_texts(&app).join("\n");
    assert!(rendered.contains("timestamp=1700000000"));
    assert!(rendered.contains("trigger=Manual"));
    assert!(rendered.contains("status=Failed"));
    assert!(rendered.contains("2/3/1"));
    assert!(rendered.contains("apply=PartiallyApplied"));
    assert!(rendered.contains("error=Parse"));
    assert!(rendered.contains("tokens(in/out)=11/7"));
    assert!(rendered.contains("duration=432ms"));
    assert!(!rendered.contains("reflection-secret-body-must-not-appear"));
}

#[tokio::test]
async fn test_clear_command_clears_task_store_and_task_window() {
    let (mut app, _started_rx, finish_tx, _client) = app_with_blocking_reflection_client_handle();
    app.model
        .conversation
        .apply(crate::tui::model::conversation::intent::UpdateTaskLines(
            vec!["━━ Tasks: 1/1 ━━".to_string(), "□ #1 existing".to_string()],
        ));
    app.refresh_live_status_from_model();

    app.handle_slash_command_with_events("/clear", None).await;
    app.refresh_live_status_from_model();

    assert!(app.model.conversation.runtime.task_status.lines.is_empty());
    assert!(app.live_status_view_model().task_lines.is_empty());
    let _ = finish_tx.send(());
}
