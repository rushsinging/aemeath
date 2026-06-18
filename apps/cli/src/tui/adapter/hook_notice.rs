use crate::tui::model::conversation::block::{HookNoticeContent, HookNoticeKind};
use crate::tui::model::conversation::system_reminder::strip_system_reminder_envelope;

pub fn hook_event_notice(event: &sdk::HookEventView) -> Option<HookNoticeContent> {
    match event.status {
        sdk::HookEventStatus::Blocked => Some(HookNoticeContent {
            kind: HookNoticeKind::Blocked,
            title: format!("Hook blocked: {}", event.hook_name),
            body: "Hook returned a block decision.".to_string(),
            details: hook_summary_details(event),
        }),
        sdk::HookEventStatus::Failed => Some(HookNoticeContent {
            kind: HookNoticeKind::Failed,
            title: format!("Hook failed: {}", event.hook_name),
            body: "Hook execution failed.".to_string(),
            details: hook_summary_details(event),
        }),
        sdk::HookEventStatus::Running | sdk::HookEventStatus::Succeeded => None,
    }
}

pub fn hook_spinner_phase(
    event: &sdk::HookEventView,
) -> crate::tui::model::runtime::spinner::SpinnerPhase {
    use crate::tui::model::runtime::spinner::{HookOutcome, SpinnerPhase};

    // PreCompact 事件使用专门的 Compacting phase
    if event.hook_name == "PreCompact" {
        return SpinnerPhase::Compacting;
    }

    // PostCompact 事件停止 spinner（通过返回 None 的方式，但这里需要特殊处理）
    // 由于函数签名要求返回 SpinnerPhase，我们使用一个特殊的方式
    // 实际上，PostCompact 事件会在 hook_spinner_phase 中被调用，
    // 但我们需要在调用方处理停止 spinner 的逻辑

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

fn hook_summary_details(event: &sdk::HookEventView) -> Option<String> {
    let mut lines = Vec::new();
    push_field(&mut lines, "Matcher", event.matcher.as_deref());
    push_field(&mut lines, "Command", event.command.as_deref());
    if let Some(result) = event.result.as_ref() {
        if let Some(exit_code) = result.exit_code {
            lines.push(format!("Exit code: {exit_code}"));
        }
        push_field(&mut lines, "Decision", result.decision.as_deref());
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn hook_spinner_detail(event: &sdk::HookEventView) -> String {
    if let Some(command) = event.command.as_deref().and_then(non_empty) {
        return truncate_for_spinner(&display_command_name(&command), 48);
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

fn display_command_name(command: &str) -> String {
    command
        .rsplit('/')
        .find(|segment| !segment.trim().is_empty())
        .unwrap_or(command)
        .trim()
        .trim_matches('"')
        .to_string()
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
        assert_eq!(notice.body, "Hook returned a block decision.");
        let details = notice.details.unwrap();
        assert!(details.contains("Decision: block"));
        assert!(!details.contains("why"));
        assert!(!details.contains("out"));
        assert!(!details.contains("err"));
    }

    #[test]
    fn failed_event_uses_summary_without_stderr() {
        let mut result = result();
        result.reason = None;
        let notice = hook_event_notice(&event(sdk::HookEventStatus::Failed, result)).unwrap();
        assert_eq!(notice.kind, HookNoticeKind::Failed);
        assert_eq!(notice.title, "Hook failed: Stop");
        assert_eq!(notice.body, "Hook execution failed.");
        assert!(!notice.details.unwrap().contains("err"));
    }

    #[test]
    fn succeeded_event_does_not_build_notice() {
        assert!(hook_event_notice(&event(sdk::HookEventStatus::Succeeded, result())).is_none());
    }

    #[test]
    fn spinner_detail_displays_command_basename_for_project_template_path() {
        let event = sdk::HookEventView {
            hook_name: "Stop".to_string(),
            status: sdk::HookEventStatus::Running,
            matcher: None,
            command: Some("{AEMEATH_PROJECT_DIR}/build_cli.sh".to_string()),
            result: None,
        };

        let phase = hook_spinner_phase(&event);

        assert_eq!(
            phase,
            crate::tui::model::runtime::spinner::SpinnerPhase::Hook {
                event: "Stop".to_string(),
                detail: "build_cli.sh".to_string(),
                outcome: crate::tui::model::runtime::spinner::HookOutcome::Running,
            }
        );
    }

    #[test]
    fn spinner_detail_strips_wrapping_quote_after_basename() {
        let event = sdk::HookEventView {
            hook_name: "Stop".to_string(),
            status: sdk::HookEventStatus::Running,
            matcher: None,
            command: Some("\"$CLAUDE_PROJECT_DIR/.claude/hooks/stop-verify.sh\"".to_string()),
            result: None,
        };

        let phase = hook_spinner_phase(&event);

        assert_eq!(
            phase,
            crate::tui::model::runtime::spinner::SpinnerPhase::Hook {
                event: "Stop".to_string(),
                detail: "stop-verify.sh".to_string(),
                outcome: crate::tui::model::runtime::spinner::HookOutcome::Running,
            }
        );
    }

    #[test]
    fn notice_body_does_not_display_system_reminder_payload() {
        let mut result = result();
        result.reason = Some("<system-reminder>\nblocked\n</system-reminder>".to_string());
        let notice = hook_event_notice(&event(sdk::HookEventStatus::Blocked, result)).unwrap();
        assert_eq!(notice.body, "Hook returned a block decision.");
    }

    #[test]
    fn blocked_stop_notice_does_not_display_stderr() {
        let mut result = result();
        result.reason = None;
        result.stdout.clear();
        result.stderr = "stop hook stderr".to_string();
        let notice = hook_event_notice(&event(sdk::HookEventStatus::Blocked, result)).unwrap();

        assert_eq!(notice.body, "Hook returned a block decision.");
        assert!(!notice.details.unwrap().contains("Stderr:"));
    }
}
