use crate::business::chat::looping::{ChatEventSink, RuntimeStreamEvent};
use hook::api::{HookData, HookJsonOutput, HookResult, HookRunner};
use share::config::hooks::{HookEntry, HookEvent};

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
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        let hooks = runner.matching_hooks(event, tool_name);
        if hooks.is_empty() {
            return Vec::new();
        }

        let input = hook::api::HookInput { event, data };
        let event_name = hook_event_name(event);
        let mut results = Vec::with_capacity(hooks.len());

        for hook in hooks {
            // 每个 hook 执行前通知 CLI 更新 spinner
            let _ = self
                .sink
                .send_event(RuntimeStreamEvent::HookStart {
                    event: event_name.to_string(),
                    command: hook.command.clone(),
                })
                .await;

            let result = runner.execute_hook(hook, &input).await;
            let json_output = result.parse_json_output();
            let should_break =
                result.blocked || json_output.as_ref().is_some_and(|j| !j.r#continue);

            // 每个 hook 执行后通知 CLI 更新 spinner
            let _ = self
                .sink
                .send_event(RuntimeStreamEvent::HookEnd {
                    event: event_name.to_string(),
                    blocked: result.blocked,
                    error: result.error.clone(),
                })
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
    ) -> Vec<HookResult> {
        self.run_json(runner, event, tool_name, data)
            .await
            .into_iter()
            .map(|(_, result, _)| result)
            .collect()
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
