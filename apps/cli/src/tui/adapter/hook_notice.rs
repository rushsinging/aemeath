use crate::tui::model::conversation::block::{HookNoticeContent, HookNoticeKind};
use crate::tui::model::conversation::system_reminder::strip_system_reminder_envelope;

pub fn hook_event_notice(event: &sdk::HookEventView) -> Option<HookNoticeContent> {
    let result = event.result.as_ref();
    match event.status {
        sdk::HookEventStatus::Blocked => Some(HookNoticeContent {
            kind: HookNoticeKind::Blocked,
            title: format!("Hook blocked: {}", event.hook_name),
            body: hook_body(result, BodyPreference::ReasonStdoutStderr)
                .unwrap_or_else(|| "Hook returned a block decision.".to_string()),
            details: hook_details(event, result, DetailMode::Blocked),
        }),
        sdk::HookEventStatus::Failed => Some(HookNoticeContent {
            kind: HookNoticeKind::Failed,
            title: format!("Hook failed: {}", event.hook_name),
            body: hook_body(result, BodyPreference::ReasonStderrStdout)
                .unwrap_or_else(|| "Hook execution failed.".to_string()),
            details: hook_details(event, result, DetailMode::Failed),
        }),
        sdk::HookEventStatus::Running | sdk::HookEventStatus::Succeeded => None,
    }
}

pub fn hook_spinner_phase(
    event: &sdk::HookEventView,
) -> crate::tui::model::runtime::spinner::SpinnerPhase {
    use crate::tui::model::runtime::spinner::{HookOutcome, SpinnerPhase};
    let outcome = match event.status {
        sdk::HookEventStatus::Running => HookOutcome::Running,
        sdk::HookEventStatus::Succeeded => HookOutcome::Done,
        sdk::HookEventStatus::Blocked => HookOutcome::Blocked,
        sdk::HookEventStatus::Failed => HookOutcome::Failed,
    };
    SpinnerPhase::Hook {
        event: event.hook_name.clone(),
        detail: hook_spinner_detail(event),
        outcome,
    }
}

#[derive(Clone, Copy)]
enum BodyPreference {
    ReasonStdoutStderr,
    ReasonStderrStdout,
}

#[derive(Clone, Copy)]
enum DetailMode {
    Blocked,
    Failed,
}

fn hook_body(
    result: Option<&sdk::HookExecutionResultView>,
    preference: BodyPreference,
) -> Option<String> {
    let result = result?;
    match preference {
        BodyPreference::ReasonStdoutStderr => first_non_empty([
            result.reason.as_deref(),
            Some(result.stdout.as_str()),
            Some(result.stderr.as_str()),
        ]),
        BodyPreference::ReasonStderrStdout => first_non_empty([
            result.reason.as_deref(),
            Some(result.stderr.as_str()),
            Some(result.stdout.as_str()),
        ]),
    }
}

fn hook_details(
    event: &sdk::HookEventView,
    result: Option<&sdk::HookExecutionResultView>,
    mode: DetailMode,
) -> Option<String> {
    let mut lines = Vec::new();
    push_field(&mut lines, "Matcher", event.matcher.as_deref());
    push_field(&mut lines, "Command", event.command.as_deref());
    if let Some(result) = result {
        if let Some(exit_code) = result.exit_code {
            lines.push(format!("Exit code: {exit_code}"));
        }
        push_field(&mut lines, "Decision", result.decision.as_deref());
        match mode {
            DetailMode::Blocked => {
                if result
                    .reason
                    .as_deref()
                    .is_some_and(|reason| !reason.trim().is_empty())
                {
                    push_field(&mut lines, "Reason", result.reason.as_deref());
                }
                push_field(
                    &mut lines,
                    "Stderr",
                    non_empty(result.stderr.as_str()).as_deref(),
                );
                push_field(
                    &mut lines,
                    "Additional context",
                    result.additional_context.as_deref(),
                );
            }
            DetailMode::Failed => {
                push_field(&mut lines, "Reason", result.reason.as_deref());
                push_field(
                    &mut lines,
                    "Stdout",
                    non_empty(result.stdout.as_str()).as_deref(),
                );
                push_field(
                    &mut lines,
                    "Stderr",
                    non_empty(result.stderr.as_str()).as_deref(),
                );
                push_field(
                    &mut lines,
                    "Additional context",
                    result.additional_context.as_deref(),
                );
            }
        }
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn hook_spinner_detail(event: &sdk::HookEventView) -> String {
    if let Some(command) = event.command.as_deref().and_then(non_empty) {
        return truncate_for_spinner(&command, 48);
    }
    event
        .result
        .as_ref()
        .and_then(|result| {
            result
                .reason
                .as_deref()
                .and_then(non_empty)
                .or_else(|| non_empty(result.stderr.as_str()))
                .map(|text| truncate_for_spinner(&text, 48))
        })
        .unwrap_or_default()
}

fn first_non_empty<const N: usize>(values: [Option<&str>; N]) -> Option<String> {
    values.into_iter().flatten().find_map(non_empty)
}

fn push_field(lines: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(value) = value.and_then(non_empty) {
        lines.push(format!("{label}: {value}"));
    }
}

fn non_empty(text: &str) -> Option<String> {
    let stripped = strip_system_reminder_envelope(text);
    let trimmed = stripped.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn truncate_for_spinner(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(limit.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        status: sdk::HookEventStatus,
        result: sdk::HookExecutionResultView,
    ) -> sdk::HookEventView {
        sdk::HookEventView {
            hook_name: "Stop".to_string(),
            status,
            matcher: Some("*".to_string()),
            command: Some("echo hook".to_string()),
            result: Some(result),
        }
    }

    fn result() -> sdk::HookExecutionResultView {
        sdk::HookExecutionResultView {
            exit_code: Some(2),
            stdout: "out".to_string(),
            stderr: "err".to_string(),
            decision: Some("block".to_string()),
            reason: Some("why".to_string()),
            additional_context: Some("ctx".to_string()),
        }
    }

    #[test]
    fn blocked_event_builds_blocked_notice() {
        let notice = hook_event_notice(&event(sdk::HookEventStatus::Blocked, result())).unwrap();
        assert_eq!(notice.kind, HookNoticeKind::Blocked);
        assert_eq!(notice.title, "Hook blocked: Stop");
        assert_eq!(notice.body, "why");
        assert!(notice.details.unwrap().contains("Decision: block"));
    }

    #[test]
    fn failed_event_prefers_stderr_without_reason() {
        let mut result = result();
        result.reason = None;
        let notice = hook_event_notice(&event(sdk::HookEventStatus::Failed, result)).unwrap();
        assert_eq!(notice.kind, HookNoticeKind::Failed);
        assert_eq!(notice.title, "Hook failed: Stop");
        assert_eq!(notice.body, "err");
    }

    #[test]
    fn succeeded_event_does_not_build_notice() {
        assert!(hook_event_notice(&event(sdk::HookEventStatus::Succeeded, result())).is_none());
    }

    #[test]
    fn body_strips_system_reminder_envelope() {
        let mut result = result();
        result.reason = Some("<system-reminder>\nblocked\n</system-reminder>".to_string());
        let notice = hook_event_notice(&event(sdk::HookEventStatus::Blocked, result)).unwrap();
        assert_eq!(notice.body, "blocked");
    }
}
