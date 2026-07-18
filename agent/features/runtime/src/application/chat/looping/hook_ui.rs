use crate::application::chat::looping::{
    ChatEventSink, RuntimeHookEvent, RuntimeHookEventStatus, RuntimeHookExecutionResult,
    RuntimeStreamEvent,
};
use hook::api::{is_blocking, HookData, HookJsonOutput, HookResult, HookRunner};
use share::config::hooks::{HookEntry, HookEvent};
use std::path::Path;

#[derive(Clone)]
pub(crate) struct HookUi<S>
where
    S: ChatEventSink,
{
    sink: S,
}

impl<S> HookUi<S>
where
    S: ChatEventSink,
{
    pub(crate) fn new(sink: S) -> Self {
        Self { sink }
    }

    pub(crate) async fn run_json(
        &self,
        runner: &HookRunner,
        event: HookEvent,
        tool_name: Option<&str>,
        data: HookData,
        workspace_root: &Path,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_json_with_cancel(
            runner,
            event,
            tool_name,
            data,
            workspace_root,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
    }

    pub(crate) async fn run_json_with_cancel(
        &self,
        runner: &HookRunner,
        event: HookEvent,
        tool_name: Option<&str>,
        data: HookData,
        workspace_root: &Path,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        let hooks = runner.matching_hooks(event, tool_name);
        log::debug!(target: crate::LOG_TARGET,
            "hook ui dispatch: event={} tool_name={:?} matched={}",
            hook_event_name(event),
            tool_name,
            hooks.len()
        );
        if hooks.is_empty() {
            return Vec::new();
        }

        let input = hook::api::HookInput { event, data };
        let event_name = hook_event_name(event);
        let mut results = Vec::with_capacity(hooks.len());

        for hook in hooks {
            log::debug!(target: crate::LOG_TARGET,
                "hook timing start: event={} tool_name={:?} matcher={:?} command={} workspace_root={}",
                event_name,
                tool_name,
                non_empty_text(&hook.matcher),
                hook.command,
                workspace_root.display(),
            );
            let started_at = std::time::Instant::now();
            let _ = self
                .sink
                .send_event(RuntimeStreamEvent::HookEvent(runtime_hook_event_running(
                    event_name, hook,
                )))
                .await;

            let result = runner
                .execute_hook_with_cancel(hook, &input, workspace_root, cancel)
                .await;
            let elapsed_ms = started_at.elapsed().as_millis();
            let json_output = result.parse_json_output();
            let status = runtime_hook_event_status(&result, &json_output);
            let should_break =
                result.blocked || json_output.as_ref().is_some_and(|j| !j.r#continue);
            log::debug!(target: crate::LOG_TARGET,
                "hook timing finish: event={} tool_name={:?} matcher={:?} status={:?} blocked={} exit_code={:?} elapsed_ms={} should_break={}",
                event_name,
                tool_name,
                non_empty_text(&hook.matcher),
                status,
                result.blocked,
                result.exit_code,
                elapsed_ms,
                should_break,
            );

            let _ = self
                .sink
                .send_event(RuntimeStreamEvent::HookEvent(runtime_hook_event_finished(
                    event_name,
                    hook,
                    &result,
                    &json_output,
                )))
                .await;

            results.push((hook.clone(), result, json_output));
            if should_break {
                break;
            }
        }

        results
    }

    pub(crate) async fn run_plain(
        &self,
        runner: &HookRunner,
        event: HookEvent,
        tool_name: Option<&str>,
        data: HookData,
        workspace_root: &Path,
    ) -> Vec<HookResult> {
        self.run_json(runner, event, tool_name, data, workspace_root)
            .await
            .into_iter()
            .map(|(_, result, _)| result)
            .collect()
    }
}

pub(crate) fn runtime_hook_event_running(event_name: &str, hook: &HookEntry) -> RuntimeHookEvent {
    RuntimeHookEvent {
        hook_name: event_name.to_string(),
        status: RuntimeHookEventStatus::Running,
        matcher: non_empty_text(&hook.matcher),
        command: Some(hook.command.clone()),
        result: None,
    }
}

pub(crate) fn runtime_hook_event_finished(
    event_name: &str,
    hook: &HookEntry,
    result: &HookResult,
    json_output: &Option<HookJsonOutput>,
) -> RuntimeHookEvent {
    RuntimeHookEvent {
        hook_name: event_name.to_string(),
        status: runtime_hook_event_status(result, json_output),
        matcher: non_empty_text(&hook.matcher),
        command: Some(hook.command.clone()),
        result: Some(RuntimeHookExecutionResult {
            exit_code: result.exit_code,
            stdout: result.output.clone(),
            stderr: result.error.clone().unwrap_or_default(),
            decision: json_output.as_ref().and_then(|json| json.decision.clone()),
            reason: hook_result_reason(result, json_output),
            additional_context: json_output
                .as_ref()
                .and_then(|json| json.additional_context.clone()),
        }),
    }
}

fn runtime_hook_event_status(
    result: &HookResult,
    json_output: &Option<HookJsonOutput>,
) -> RuntimeHookEventStatus {
    if result.error.is_some() && !result.blocked {
        return RuntimeHookEventStatus::Failed;
    }
    if is_blocking(result, json_output) {
        return RuntimeHookEventStatus::Blocked;
    }
    RuntimeHookEventStatus::Succeeded
}

fn hook_result_reason(result: &HookResult, json_output: &Option<HookJsonOutput>) -> Option<String> {
    json_output
        .as_ref()
        .and_then(|json| {
            json.reason
                .clone()
                .or_else(|| json.system_message.clone())
                .or_else(|| json.stop_reason.clone())
        })
        .or_else(|| result.error.clone())
        .and_then(|text| non_empty_text(&text))
}

fn non_empty_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(crate) fn hook_event_name(event: HookEvent) -> &'static str {
    match event {
        HookEvent::PreToolUse => "PreToolUse",
        HookEvent::PostToolUse => "PostToolUse",
        HookEvent::PostToolUseFailure => "PostToolUseFailure",
        HookEvent::UserPromptSubmit => "UserPromptSubmit",
        HookEvent::Stop => "Stop",
        HookEvent::StopFailure => "StopFailure",
        HookEvent::SessionStart => "SessionStart",
        HookEvent::SessionEnd => "SessionEnd",
        HookEvent::PreCompact => "PreCompact",
        HookEvent::PostCompact => "PostCompact",
        HookEvent::PostToolBatch => "PostToolBatch",
        HookEvent::SubagentStart => "SubagentStart",
        HookEvent::SubagentStop => "SubagentStop",
        HookEvent::TaskCreated => "TaskCreated",
        HookEvent::TaskCompleted => "TaskCompleted",
        HookEvent::PermissionRequest => "PermissionRequest",
        HookEvent::PermissionDenied => "PermissionDenied",
        HookEvent::Notification => "Notification",
        HookEvent::InstructionsLoaded => "InstructionsLoaded",
        HookEvent::ConfigChange => "ConfigChange",
        HookEvent::Elicitation => "Elicitation",
        HookEvent::ElicitationResult => "ElicitationResult",
        HookEvent::UserPromptExpansion => "UserPromptExpansion",
        HookEvent::CwdChanged => "CwdChanged",
        HookEvent::FileChanged => "FileChanged",
        HookEvent::TeammateIdle => "TeammateIdle",
    }
}
