use kernel::config::hooks::HookEvent;
use kernel::hook::{HookData, HookRunner, SessionHookData, StopHookData};
use kernel::message::Message;
use kernel::session::{self, Session};
use std::path::Path;

pub(super) async fn run_session_start_hooks(hook_runner: &HookRunner, user_context: &mut String) {
    let hook_results = hook_runner
        .run_hooks_with_json(
            HookEvent::SessionStart,
            None,
            HookData::Session(SessionHookData {}),
        )
        .await;

    for (_, result, json_output) in &hook_results {
        if let Some(json) = json_output {
            if let Some(ref ctx) = json.additional_context {
                *user_context = if user_context.is_empty() {
                    ctx.clone()
                } else {
                    format!("{}\n\n{}", ctx, user_context)
                };
            }
        }
        if result.blocked {
            eprintln!("[SessionStart hook blocked session start]");
        }
    }
}

pub(super) async fn save_session_on_exit(
    messages: &[Message],
    resumed_session: Option<Session>,
    session_id: &str,
    cwd: &Path,
) {
    if messages.is_empty() {
        return;
    }

    let session = if let Some(mut existing) = resumed_session {
        existing.messages = messages.to_vec();
        existing.updated_at = session::now_iso();
        existing
    } else {
        let mut new_session =
            Session::new(session_id.to_string(), cwd.to_string_lossy().to_string());
        new_session.messages = messages.to_vec();
        new_session
    };

    if let Err(e) = session::save_session(&session).await {
        eprintln!("warning: failed to save session: {e}");
    } else {
        crate::render::TerminalRenderer::print_session_saved(session_id);
    }
}

pub(super) async fn run_stop_hooks(hook_runner: &HookRunner, turn_count: usize) {
    let hook_results = hook_runner
        .run_hooks(
            HookEvent::Stop,
            None,
            HookData::Stop(StopHookData { turns: turn_count }),
        )
        .await;

    for result in &hook_results {
        if result.blocked {
            eprintln!("[Stop hook blocked]");
        }
        if let Some(error) = &result.error {
            log::warn!("Stop hook error: {error}");
            eprintln!("warning: Stop hook error: {error}");
        }
    }
}

pub(super) async fn run_session_end_hooks(hook_runner: &HookRunner) {
    let hook_results = hook_runner.on_session_end().await;
    for (_, result, json_output) in &hook_results {
        if let Some(json) = json_output {
            if let Some(ref msg) = json.system_message {
                eprintln!("{}", msg);
            }
        }
        if result.error.is_some() {
            log::warn!("SessionEnd hook error: {:?}", result.error);
        }
    }
}
