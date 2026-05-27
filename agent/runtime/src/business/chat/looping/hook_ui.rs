use crate::api::core::config::hooks::{HookEntry, HookEvent};
use crate::api::hook::hook::{HookData, HookJsonOutput, HookResult, HookRunner};
use crate::business::chat::looping::{ChatEventSink, RuntimeStreamEvent};

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

        let command = hooks
            .first()
            .map(|hook| hook.command.clone())
            .unwrap_or_default();
        let event_name = hook_event_name(event);
        let _ = self
            .sink
            .send_event(RuntimeStreamEvent::HookStart {
                event: event_name.to_string(),
                command,
            })
            .await;

        let hook_results = runner.run_hooks_with_json(event, tool_name, data).await;

        for (_, result, _) in &hook_results {
            let _ = self
                .sink
                .send_event(RuntimeStreamEvent::HookEnd {
                    event: event_name.to_string(),
                    blocked: result.blocked,
                    error: result.error.clone(),
                })
                .await;
        }
        hook_results
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
