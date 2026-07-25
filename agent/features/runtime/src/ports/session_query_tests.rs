use async_trait::async_trait;
use sdk::{ModelSummary, ReflectionHistoryView, ReminderView, SdkError, SessionSummary};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::SessionQueryPort;

/// Test fixtures — flat canned data builders.
fn canned_model() -> ModelSummary {
    ModelSummary {
        provider: "test-provider".into(),
        id: "test-model".into(),
        name: "Test Model".into(),
        context_window: 8192,
        max_tokens: 4096,
    }
}

fn canned_session() -> SessionSummary {
    SessionSummary {
        id: "session-1".into(),
        title: Some("Test Session".into()),
        project: Some("test-project".into()),
        model: Some("test-model".into()),
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
        message_count: 10,
        preview: None,
        summary: "test summary".into(),
    }
}

fn canned_reminder() -> ReminderView {
    ReminderView {
        id: "reminder-1".into(),
        content: "test reminder".into(),
        done: false,
        created_at: 1700000000,
    }
}

fn canned_reflection() -> ReflectionHistoryView {
    ReflectionHistoryView {
        id: "ref-1".into(),
        timestamp: 42,
        trigger: sdk::ReflectionTriggerView::Manual,
        status: sdk::ReflectionStatusView::Succeeded,
        deviations: 1,
        suggestions: 2,
        outdated: 0,
        apply_status: sdk::ReflectionApplyStatusView::NotApplied,
        error_category: None,
        token_usage: None,
        duration_ms: 100,
    }
}

/// A recording fake that captures every call argument and returns canned responses.
struct RecordingFake {
    should_error: bool,
    model_calls: AtomicUsize,
    session_calls: AtomicUsize,
    reminder_calls: AtomicUsize,
    reflection_calls: AtomicUsize,
    last_reflection_limit: std::sync::Mutex<Option<usize>>,
}

impl RecordingFake {
    fn new() -> Self {
        Self {
            should_error: false,
            model_calls: AtomicUsize::new(0),
            session_calls: AtomicUsize::new(0),
            reminder_calls: AtomicUsize::new(0),
            reflection_calls: AtomicUsize::new(0),
            last_reflection_limit: std::sync::Mutex::new(None),
        }
    }
}

#[async_trait]
impl SessionQueryPort for RecordingFake {
    async fn list_models(&self) -> Result<Vec<ModelSummary>, SdkError> {
        self.model_calls.fetch_add(1, Ordering::SeqCst);
        if self.should_error {
            Err(SdkError::Internal("models unavailable".into()))
        } else {
            Ok(vec![canned_model()])
        }
    }

    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SdkError> {
        self.session_calls.fetch_add(1, Ordering::SeqCst);
        if self.should_error {
            Err(SdkError::Session("sessions unavailable".into()))
        } else {
            Ok(vec![canned_session()])
        }
    }

    async fn list_reminders(&self) -> Result<Vec<ReminderView>, SdkError> {
        self.reminder_calls.fetch_add(1, Ordering::SeqCst);
        if self.should_error {
            Err(SdkError::Internal("reminders unavailable".into()))
        } else {
            Ok(vec![canned_reminder()])
        }
    }

    async fn list_reflection_history(
        &self,
        limit: usize,
    ) -> Result<Vec<ReflectionHistoryView>, SdkError> {
        self.reflection_calls.fetch_add(1, Ordering::SeqCst);
        *self.last_reflection_limit.lock().unwrap() = Some(limit);
        if self.should_error {
            Err(SdkError::Internal("reflection history unavailable".into()))
        } else {
            Ok(vec![canned_reflection()])
        }
    }
}

#[tokio::test]
async fn session_query_port_object_carries_four_query_methods() {
    let fake = RecordingFake::new();

    // Exercise list_models
    let models = fake.list_models().await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "test-model");
    assert_eq!(fake.model_calls.load(Ordering::SeqCst), 1);

    // Exercise list_sessions
    let sessions = fake.list_sessions().await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "session-1");
    assert_eq!(sessions[0].title.as_deref(), Some("Test Session"));
    assert_eq!(fake.session_calls.load(Ordering::SeqCst), 1);

    // Exercise list_reminders
    let reminders = fake.list_reminders().await.unwrap();
    assert_eq!(reminders.len(), 1);
    assert_eq!(reminders[0].id, "reminder-1");
    assert_eq!(fake.reminder_calls.load(Ordering::SeqCst), 1);

    // Exercise list_reflection_history with a specific limit
    let reflections = fake.list_reflection_history(7).await.unwrap();
    assert_eq!(reflections.len(), 1);
    assert_eq!(reflections[0].id, "ref-1");
    assert_eq!(fake.reflection_calls.load(Ordering::SeqCst), 1);
    assert_eq!(*fake.last_reflection_limit.lock().unwrap(), Some(7));
}

#[tokio::test]
async fn session_query_port_error_propagation() {
    let error_fake = RecordingFake {
        should_error: true,
        ..RecordingFake::new()
    };

    let models_err = error_fake.list_models().await.unwrap_err();
    assert!(matches!(models_err, SdkError::Internal(_)));

    let sessions_err = error_fake.list_sessions().await.unwrap_err();
    assert!(matches!(sessions_err, SdkError::Session(_)));

    let reminders_err = error_fake.list_reminders().await.unwrap_err();
    assert!(matches!(reminders_err, SdkError::Internal(_)));

    let reflection_err = error_fake.list_reflection_history(3).await.unwrap_err();
    assert!(matches!(reflection_err, SdkError::Internal(_)));
}

#[test]
fn session_query_port_is_object_safe_and_send_sync() {
    // Compile-time assertion: the trait must be usable as `Arc<dyn SessionQueryPort>`.
    fn _assert_object_safe(_: &dyn SessionQueryPort) {}
    fn _assert_send_sync<T: Send + Sync>(_: &T) {}

    // These would fail to compile if the trait were not object-safe or Send+Sync.
    let fake = RecordingFake::new();
    _assert_object_safe(&fake);
    _assert_send_sync(&fake);
}
