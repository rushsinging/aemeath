use super::block::{HookNoticeContent, HookNoticeKind};
use super::system_reminder::strip_system_reminder_envelope;

pub fn stop_hook_notice_content(message: &sdk::ChatMessage) -> HookNoticeContent {
    if let Some(payload) = message
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.stop_hook.as_ref())
    {
        let mut details = vec![format!("Command: {}", payload.command)];
        if let Some(exit_code) = payload.exit_code {
            details.push(format!("Exit code: {exit_code}"));
        }
        if !payload.reason.is_empty() {
            details.push(format!("Reason: {}", payload.reason));
        }
        if !payload.stderr_preview.is_empty() {
            details.push(format!("stderr:\n{}", payload.stderr_preview));
        }
        if !payload.stdout_preview.is_empty() {
            details.push(format!("stdout:\n{}", payload.stdout_preview));
        }
        if payload.stderr_truncated {
            details.push("stderr preview truncated".to_string());
        }
        if payload.stdout_truncated {
            details.push("stdout preview truncated".to_string());
        }
        if let Some(path) = &payload.output_file {
            details.push(format!("Full output: {path}"));
        }
        return HookNoticeContent {
            kind: HookNoticeKind::Blocked,
            title: "Hook blocked: Stop".to_string(),
            body: payload.summary.clone(),
            details: Some(details.join("\n\n")),
        };
    }

    HookNoticeContent {
        kind: HookNoticeKind::Blocked,
        title: "Hook blocked: Stop".to_string(),
        body: strip_system_reminder_envelope(&message.text_content()).to_string(),
        details: Some("Historical record has no structured hook execution details.".to_string()),
    }
}
