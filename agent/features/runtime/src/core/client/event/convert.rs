use crate::business::chat::looping::RuntimeTurnContext;
use crate::business::chat::{RuntimeHookEvent, RuntimeHookEventStatus};
use crate::LOG_TARGET;
use sdk::{
    AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView, ChangeSet, ChatEvent,
    ChatEventContext, HookEventStatus, HookEventView, HookExecutionResultView, ToolCallStatusView,
    ToolResultImage,
};

fn turn_context_to_sdk(context: RuntimeTurnContext) -> ChatEventContext {
    ChatEventContext::new(context.chat_id, context.turn_id)
}

fn tool_call_status_to_sdk(
    status: crate::business::chat::RuntimeToolCallStatus,
) -> ToolCallStatusView {
    match status {
        crate::business::chat::RuntimeToolCallStatus::PendingArgs => {
            ToolCallStatusView::PendingArgs
        }
        crate::business::chat::RuntimeToolCallStatus::Ready => ToolCallStatusView::Ready,
        crate::business::chat::RuntimeToolCallStatus::Running => ToolCallStatusView::Running,
    }
}

pub(crate) fn runtime_event_to_sdk_event(
    event: crate::business::chat::RuntimeStreamEvent,
    change_tx: &tokio::sync::watch::Sender<ChangeSet>,
) -> ChatEvent {
    match event {
        crate::business::chat::RuntimeStreamEvent::Text { context, text } => ChatEvent::Token {
            context: turn_context_to_sdk(context),
            text,
        },
        crate::business::chat::RuntimeStreamEvent::Thinking { context, text } => {
            ChatEvent::Thinking {
                context: turn_context_to_sdk(context),
                text,
            }
        }
        crate::business::chat::RuntimeStreamEvent::BlockComplete { context, text } => {
            ChatEvent::BlockComplete {
                context: turn_context_to_sdk(context),
                text,
            }
        }
        crate::business::chat::RuntimeStreamEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => {
            log::trace!(
                target: LOG_TARGET,
                "runtime->sdk tool_call_start chat_id={} turn_id={} id={} provider_id={:?} name={} index={}",
                context.chat_id,
                context.turn_id,
                id,
                provider_id,
                name,
                index
            );
            ChatEvent::ToolCallStart {
                context: turn_context_to_sdk(context),
                id,
                provider_id,
                name,
                index,
            }
        }
        crate::business::chat::RuntimeStreamEvent::ToolCallUpdate {
            context,
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        } => {
            log::trace!(
                target: LOG_TARGET,
                "runtime->sdk tool_call_update chat_id={} turn_id={} id={} provider_id={:?} name={} index={} status={:?} args_delta_len={} args_present={}",
                context.chat_id,
                context.turn_id,
                id,
                provider_id,
                name,
                index,
                status,
                arguments_delta.as_ref().map(|value| value.len()).unwrap_or(0),
                arguments.is_some()
            );
            ChatEvent::ToolCallUpdate {
                context: turn_context_to_sdk(context),
                id,
                provider_id,
                name,
                index,
                arguments_delta,
                arguments,
                status: tool_call_status_to_sdk(status),
            }
        }
        crate::business::chat::RuntimeStreamEvent::ToolResult {
            context,
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
        } => {
            log::trace!(
                target: LOG_TARGET,
                "runtime->sdk tool_result chat_id={} turn_id={} id={} provider_id={} tool_name={} output_len={} content_kind={} is_error={} image_count={}",
                context.chat_id,
                context.turn_id,
                id,
                provider_id,
                tool_name,
                output.len(),
                match &content {
                    serde_json::Value::Null => "null",
                    serde_json::Value::Bool(_) => "bool",
                    serde_json::Value::Number(_) => "number",
                    serde_json::Value::String(_) => "string",
                    serde_json::Value::Array(_) => "array",
                    serde_json::Value::Object(_) => "object",
                },
                is_error,
                images.len()
            );
            ChatEvent::ToolResult {
                context: turn_context_to_sdk(context),
                id,
                provider_id,
                tool_name,
                output,
                content,
                is_error,
                images: images
                    .into_iter()
                    .map(|image| ToolResultImage {
                        base64: image.base64,
                        media_type: image.media_type,
                    })
                    .collect(),
            }
        }
        crate::business::chat::RuntimeStreamEvent::SystemMessage(msg) => {
            ChatEvent::SystemMessage(msg)
        }
        crate::business::chat::RuntimeStreamEvent::ModelStreamWaiting {
            context,
            elapsed_secs,
            phase,
        } => ChatEvent::ModelStreamWaiting {
            context: turn_context_to_sdk(context),
            elapsed_secs,
            phase,
        },
        crate::business::chat::RuntimeStreamEvent::Error(msg) => ChatEvent::Error(msg),
        crate::business::chat::RuntimeStreamEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        } => ChatEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        },
        crate::business::chat::RuntimeStreamEvent::TurnStarted { messages } => {
            ChatEvent::TurnStarted {
                messages: messages
                    .into_iter()
                    .map(super::super::mapping::message_to_sdk)
                    .collect(),
            }
        }
        crate::business::chat::RuntimeStreamEvent::MicrocompactDone {
            messages,
            cleared_count,
        } => ChatEvent::MicrocompactDone {
            messages: messages
                .into_iter()
                .map(super::super::mapping::message_to_sdk)
                .collect(),
            cleared_count,
        },
        crate::business::chat::RuntimeStreamEvent::StopHookBlocked { messages } => {
            ChatEvent::StopHookBlocked {
                messages: messages
                    .into_iter()
                    .map(super::super::mapping::message_to_sdk)
                    .collect(),
            }
        }
        crate::business::chat::RuntimeStreamEvent::PostToolExecutionSync { messages } => {
            ChatEvent::PostToolExecutionSync {
                messages: messages
                    .into_iter()
                    .map(super::super::mapping::message_to_sdk)
                    .collect(),
            }
        }
        crate::business::chat::RuntimeStreamEvent::ApiError { messages, error } => {
            ChatEvent::ApiError {
                messages: messages
                    .into_iter()
                    .map(super::super::mapping::message_to_sdk)
                    .collect(),
                error,
            }
        }
        crate::business::chat::RuntimeStreamEvent::CompactRollback { messages } => {
            ChatEvent::CompactRollback {
                messages: messages
                    .into_iter()
                    .map(super::super::mapping::message_to_sdk)
                    .collect(),
            }
        }
        crate::business::chat::RuntimeStreamEvent::CompactFinished { messages } => {
            ChatEvent::CompactFinished {
                messages: messages
                    .into_iter()
                    .map(super::super::mapping::message_to_sdk)
                    .collect(),
            }
        }
        crate::business::chat::RuntimeStreamEvent::UserMessagesAdopted { items, queued } => {
            // #507 修复：把 (InputId, Message) 元组映射为带 input_id 的 ChatMessage，
            // 让 TUI 端 handler 按 input_id 清占位 + 用 text_content() 还原回显。
            ChatEvent::UserMessagesAdopted {
                items: items
                    .into_iter()
                    .map(|(id, message)| {
                        let mut sdk_msg = super::super::mapping::message_to_sdk(message);
                        sdk_msg.input_id = Some(id);
                        sdk_msg
                    })
                    .collect(),
                queued: queued
                    .into_iter()
                    .map(|(id, message)| {
                        let mut sdk_msg = super::super::mapping::message_to_sdk(message);
                        sdk_msg.input_id = Some(id);
                        sdk_msg
                    })
                    .collect(),
            }
        }
        crate::business::chat::RuntimeStreamEvent::UserMessagesQueued { queued } => {
            ChatEvent::UserMessagesQueued {
                queued: queued
                    .into_iter()
                    .map(|(id, message)| {
                        let mut sdk_msg = super::super::mapping::message_to_sdk(message);
                        sdk_msg.input_id = Some(id);
                        sdk_msg
                    })
                    .collect(),
            }
        }
        crate::business::chat::RuntimeStreamEvent::Done { context } => ChatEvent::Done {
            context: ChatEventContext::new(context.chat_id, context.turn_id),
        },
        crate::business::chat::RuntimeStreamEvent::DoneWithDuration { context, duration } => {
            ChatEvent::DoneWithDurationMs {
                context: ChatEventContext::new(context.chat_id, context.turn_id),
                duration_ms: duration.as_millis() as u64,
            }
        }
        crate::business::chat::RuntimeStreamEvent::Cancelled { context } => ChatEvent::Cancelled {
            context: ChatEventContext::new(context.chat_id, context.turn_id),
        },
        crate::business::chat::RuntimeStreamEvent::LiveTps(tps) => ChatEvent::LiveTps(tps),
        crate::business::chat::RuntimeStreamEvent::TurnChanged(turn) => {
            ChatEvent::CurrentTurnChanged(turn)
        }
        crate::business::chat::RuntimeStreamEvent::HookEvent(event) => {
            ChatEvent::HookEvent(runtime_hook_event_to_sdk(event))
        }
        crate::business::chat::RuntimeStreamEvent::AskUserBatch { items, reply_tx } => {
            ChatEvent::AskUserBatch { items, reply_tx }
        }
        crate::business::chat::RuntimeStreamEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => ChatEvent::AgentProgress {
            context: ChatEventContext::new(context.chat_id, context.turn_id),
            tool_id,
            event: agent_progress_event_to_sdk(event),
        },
        crate::business::chat::RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base,
            workspace_root,
            workspace,
        } => {
            // service 已是单一可变源，工具调用时已直接更新它；此处仅作 UI/SDK 通知，
            // 事件自带快照 DTO，无需回写任何 runtime 状态。
            let previous = *change_tx.borrow();
            change_tx.send_replace(previous | ChangeSet::PROJECT);
            ChatEvent::WorkingDirectoryChanged {
                path_base,
                workspace_root,
                workspace: super::super::mapping::workspace_context_to_sdk(workspace),
            }
        }
        crate::business::chat::RuntimeStreamEvent::ConfigReloaded { changed_keys } => {
            ChatEvent::ConfigReloaded { changed_keys }
        }
        crate::business::chat::RuntimeStreamEvent::GraphPhaseChanged { node, effort, prev } => {
            ChatEvent::GraphPhaseChanged {
                node: format!("{node}"),
                effort: format!("{effort:?}").to_lowercase(),
                prev: format!("{prev}"),
            }
        }
        crate::business::chat::RuntimeStreamEvent::SessionReset => ChatEvent::SessionReset,
        crate::business::chat::RuntimeStreamEvent::UserMessagesWithdrawn { texts } => {
            ChatEvent::UserMessagesWithdrawn { texts }
        }
        crate::business::chat::RuntimeStreamEvent::CompactProgress {
            stage,
            current,
            total,
        } => ChatEvent::CompactProgress {
            stage: stage.as_str().to_string(),
            current: current.map(|n| n as u32),
            total: total.map(|n| n as u32),
        },
        crate::business::chat::RuntimeStreamEvent::ModelSwitched { result } => {
            ChatEvent::ModelSwitched { result }
        }
        crate::business::chat::RuntimeStreamEvent::ThinkingChanged { enabled } => {
            ChatEvent::ThinkingChanged { enabled }
        }
        crate::business::chat::RuntimeStreamEvent::ContextEstimated {
            estimate,
            message_count,
        } => ChatEvent::ContextEstimated {
            estimate,
            message_count,
        },
        crate::business::chat::RuntimeStreamEvent::CommandResultText { text, is_error } => {
            ChatEvent::CommandResultText { text, is_error }
        }
        crate::business::chat::RuntimeStreamEvent::SessionResumed {
            messages,
            session_id,
            created_at,
        } => {
            let sdk_messages = messages
                .into_iter()
                .map(super::super::mapping::message_to_sdk)
                .collect();
            ChatEvent::SessionResumed {
                messages: sdk_messages,
                session_id,
                created_at,
            }
        }
        crate::business::chat::RuntimeStreamEvent::SessionResumeFailed { kind, id, message } => {
            ChatEvent::SessionResumeFailed { kind, id, message }
        }
        crate::business::chat::RuntimeStreamEvent::ReflectionResult { output } => {
            ChatEvent::ReflectionResult { output }
        }
        crate::business::chat::RuntimeStreamEvent::ModelList { models } => {
            ChatEvent::ModelList { models }
        }
        crate::business::chat::RuntimeStreamEvent::ReminderList { reminders } => {
            ChatEvent::ReminderList { reminders }
        }
        crate::business::chat::RuntimeStreamEvent::SessionList { sessions } => {
            ChatEvent::SessionList { sessions }
        }
        crate::business::chat::RuntimeStreamEvent::ProjectInfo { project } => {
            ChatEvent::ProjectInfo { project }
        }
        crate::business::chat::RuntimeStreamEvent::TasksSnapshot { tasks } => {
            ChatEvent::TasksSnapshot { tasks }
        }
        crate::business::chat::RuntimeStreamEvent::CostUpdate { cost } => {
            ChatEvent::CostUpdate { cost }
        }
    }
}

pub(super) fn runtime_hook_event_to_sdk(event: RuntimeHookEvent) -> HookEventView {
    HookEventView {
        hook_name: event.hook_name,
        status: match event.status {
            RuntimeHookEventStatus::Running => HookEventStatus::Running,
            RuntimeHookEventStatus::Succeeded => HookEventStatus::Succeeded,
            RuntimeHookEventStatus::Blocked => HookEventStatus::Blocked,
            RuntimeHookEventStatus::Failed => HookEventStatus::Failed,
        },
        matcher: event.matcher,
        command: event.command,
        result: event.result.map(|result| HookExecutionResultView {
            exit_code: result.exit_code,
            stdout: result.stdout,
            stderr: result.stderr,
            decision: result.decision,
            reason: result.reason,
            additional_context: result.additional_context,
        }),
    }
}

pub(super) fn agent_progress_event_to_sdk(
    event: share::tool::AgentProgressEvent,
) -> AgentProgressEventView {
    let kind = match event.kind {
        share::tool::AgentProgressKind::ToolCalls { calls } => AgentProgressKindView::ToolCalls {
            calls: calls
                .into_iter()
                .map(|call| AgentToolCallProgressView {
                    id: sdk::ids::ToolCallId::from_legacy_or_new(&call.id),
                    name: call.name,
                    input: call.input,
                })
                .collect(),
        },
        share::tool::AgentProgressKind::ToolOutput { tool_name, text } => {
            AgentProgressKindView::ToolOutput { tool_name, text }
        }
        share::tool::AgentProgressKind::Message { text } => AgentProgressKindView::Message { text },
        share::tool::AgentProgressKind::Started { role, model } => {
            AgentProgressKindView::Started { role, model }
        }
    };
    AgentProgressEventView {
        sequence: event.sequence,
        kind,
    }
}
