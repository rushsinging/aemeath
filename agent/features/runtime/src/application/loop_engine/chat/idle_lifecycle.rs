//! Session idle lifecycle. Active Run state belongs exclusively to agent_run.

use crate::application::loop_engine::chat::apply_gate;
use crate::application::loop_engine::chat::events::{ChatEventSink, RuntimeStreamEvent};
use crate::application::loop_engine::chat::immediate_query::ImmediateQueryDispatcher;
use crate::application::loop_engine::chat::input_gate::{
    event_kind_name, GateKind, InputEventDrainPort, PendingCommand, PendingInputBuffer,
};
use crate::application::loop_engine::AcceptedUserInput;
use crate::application::run::session_reasoning::SessionReasoningControl;
use crate::ports::ProviderBindingData;
use share::reasoning::ReasoningLevel;

/// 执行 `/think` 档位调节（#2032）：
/// 1. `SessionReasoningControl::apply_thinking_request` 解析请求档位；
/// 2. `ModelInfo::resolve_reasoning` clamp 到当前模型能力上限得映射档位；
/// 3. `ThinkingChanged { requested, effective }` 回传 TUI（状态栏显示映射档位）。
///
/// 返回请求档位（canonical 字符串落盘由调用方负责）。
pub(crate) async fn execute_set_thinking<S>(
    reasoning: &std::sync::Mutex<SessionReasoningControl>,
    sink: &S,
    request: sdk::ThinkingRequest,
    binding: &ProviderBindingData,
) -> ReasoningLevel
where
    S: ChatEventSink,
{
    let requested = reasoning
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .apply_thinking_request(request);
    let effective = binding.model.resolve_reasoning(requested);
    let enabled = !matches!(effective, ReasoningLevel::Off);
    sink.send_event(RuntimeStreamEvent::ThinkingChanged {
        enabled,
        requested,
        effective,
    })
    .await;
    let receipt = if effective == requested {
        format!("[thinking mode: {}]", effective.as_str())
    } else {
        format!(
            "[thinking mode: {} (requested {}, clamped to model capability)]",
            effective.as_str(),
            requested.as_str()
        )
    };
    sink.send_event(RuntimeStreamEvent::SystemMessage(receipt))
        .await;
    requested
}

#[derive(Debug)]
pub(crate) enum IdleResult {
    Resumed {
        segment_id: String,
        accepted_inputs: Vec<AcceptedUserInput>,
    },
    ResetRequested,
    Shutdown,
    CommandRequested(PendingCommand),
    /// idle `/compact`：启动一次只执行压缩、不调用模型的 Run。
    ManualCompactionRequested,
    /// idle `/reflect-now`：启动一次只执行反思、不调用模型的 Run。
    ManualReflectionRequested,
    /// 后台进程完成唤醒（#252）：无 active Run 时由 WakeupMailbox 信号驱动，
    /// 启动一次 `RunIntent::BackgroundProcessWakeup` 的 Main Run（D13：直接启动，
    /// 用户可 Esc 走标准取消协议；不合成用户 turn）。
    BackgroundProcessWakeup,
}

async fn await_idle_input<I, S>(
    input_events: &I,
    pending: &mut PendingInputBuffer,
    wakeup: Option<&mut crate::application::session::wakeup::WakeupWaiter>,
    immediate_query: &ImmediateQueryDispatcher,
    sink: &S,
) -> IdleResult
where
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    let mut wakeup = wakeup;
    loop {
        let event = match pending.pop_front() {
            Some(event) => Some(event),
            None => match wakeup.as_deref_mut() {
                // 无 wakeup 接入（如部分测试场景）：保持既有纯输入等待。
                None => input_events.recv_next_input().await,
                // idle 等待点 select：输入与后台进程唤醒竞争（D13：不仲裁，
                // 先到先服务；输入侧被选中时唤醒信号保留给下一轮 idle）。
                Some(waiter) => {
                    tokio::select! {
                        event = input_events.recv_next_input() => event,
                        signal = waiter.wait() => match signal {
                            Some(()) => return IdleResult::BackgroundProcessWakeup,
                            // 发送端全部释放（session 退出）：不再等待 wakeup，
                            // 回落到纯输入等待语义。
                            None => input_events.recv_next_input().await,
                        },
                    }
                }
            },
        };
        let Some(event) = event else {
            return IdleResult::Shutdown;
        };
        log::debug!(
            target: crate::LOG_TARGET,
            "session idle woken by event kind={}",
            event_kind_name(&event)
        );
        // 查询类事件就地执行并回事件，不进入 pending：idle 不是排队的理由。
        if immediate_query.try_dispatch(&event, sink).await {
            continue;
        }
        pending.push(event);
        return IdleResult::Resumed {
            segment_id: String::new(),
            accepted_inputs: Vec::new(),
        };
    }
}

pub(crate) async fn idle_until_resume_or_shutdown<I, S>(
    input_events: &I,
    sink: &S,
    pending: &mut PendingInputBuffer,
    task_access: &dyn task::TaskAccess,
    mut wakeup: Option<&mut crate::application::session::wakeup::WakeupWaiter>,
    immediate_query: &ImmediateQueryDispatcher,
) -> IdleResult
where
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    loop {
        match await_idle_input(
            input_events,
            pending,
            wakeup.as_deref_mut(),
            immediate_query,
            sink,
        )
        .await
        {
            IdleResult::Resumed { .. } => {
                let segment_id = sdk::ChatId::new_v7().to_string();
                let gate = apply_gate(GateKind::BeforeLlm, pending, sink, task_access, true).await;
                if let Some(command) = gate.pending_command {
                    return IdleResult::CommandRequested(command);
                }
                if gate.reset_requested {
                    return IdleResult::ResetRequested;
                }
                if gate.appended_user_messages > 0 {
                    return IdleResult::Resumed {
                        segment_id,
                        accepted_inputs: gate.accepted_inputs,
                    };
                }
            }
            IdleResult::ResetRequested => return IdleResult::ResetRequested,
            IdleResult::Shutdown => return IdleResult::Shutdown,
            IdleResult::CommandRequested(command) => return IdleResult::CommandRequested(command),
            IdleResult::ManualCompactionRequested => {
                return IdleResult::ManualCompactionRequested;
            }
            IdleResult::ManualReflectionRequested => {
                return IdleResult::ManualReflectionRequested;
            }
            IdleResult::BackgroundProcessWakeup => return IdleResult::BackgroundProcessWakeup,
        }
    }
}
