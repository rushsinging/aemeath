//! Session idle lifecycle. Active Run state belongs exclusively to agent_run.

use crate::application::loop_engine::chat::apply_gate;
use crate::application::loop_engine::chat::events::{ChatEventSink, RuntimeStreamEvent};
use crate::application::loop_engine::chat::input_gate::{
    event_kind_name, GateKind, InputEventDrainPort, PendingCommand, PendingInputBuffer,
};
use crate::application::loop_engine::AcceptedUserInput;
use share::reasoning::ReasoningLevel;

fn requested_level_for_thinking(
    reasoning: &std::sync::Mutex<ReasoningLevel>,
    desired: Option<bool>,
) -> ReasoningLevel {
    let mut current = reasoning.lock().unwrap_or_else(|error| error.into_inner());
    let enabled = desired.unwrap_or(matches!(*current, ReasoningLevel::Off));
    *current = if enabled {
        ReasoningLevel::Medium
    } else {
        ReasoningLevel::Off
    };
    *current
}

pub(crate) async fn execute_set_thinking<S>(
    reasoning: &std::sync::Mutex<ReasoningLevel>,
    sink: &S,
    desired: Option<bool>,
) -> ReasoningLevel
where
    S: ChatEventSink,
{
    let level = requested_level_for_thinking(reasoning, desired);
    let enabled = !matches!(level, ReasoningLevel::Off);
    sink.send_event(RuntimeStreamEvent::ThinkingChanged { enabled, level })
        .await;
    sink.send_event(RuntimeStreamEvent::SystemMessage(format!(
        "[thinking mode: {}]",
        level.as_str()
    )))
    .await;
    level
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
    /// 后台任务完成唤醒（#252）：无 active Run 时由 WakeupMailbox 信号驱动，
    /// 启动一次 `RunIntent::BackgroundTaskWakeup` 的 Main Run（D13：直接启动，
    /// 用户可 Esc 走标准取消协议；不合成用户 turn）。
    BackgroundTaskWakeup,
}

async fn await_idle_input<I: InputEventDrainPort>(
    input_events: &I,
    pending: &mut PendingInputBuffer,
    wakeup: Option<&mut crate::application::session::wakeup::WakeupWaiter>,
) -> IdleResult {
    let event = match pending.pop_front() {
        Some(event) => Some(event),
        None => match wakeup {
            // 无 wakeup 接入（如部分测试场景）：保持既有纯输入等待。
            None => input_events.recv_next_input().await,
            // idle 等待点 select：输入与后台任务唤醒竞争（D13：不仲裁，
            // 先到先服务；输入侧被选中时唤醒信号保留给下一轮 idle）。
            Some(waiter) => {
                tokio::select! {
                    event = input_events.recv_next_input() => event,
                    signal = waiter.wait() => match signal {
                        Some(()) => return IdleResult::BackgroundTaskWakeup,
                        // 发送端全部释放（session 退出）：不再等待 wakeup，
                        // 回落到纯输入等待语义。
                        None => input_events.recv_next_input().await,
                    },
                }
            }
        },
    };
    match event {
        Some(event) => {
            log::debug!(
                target: crate::LOG_TARGET,
                "session idle woken by event kind={}",
                event_kind_name(&event)
            );
            pending.push(event);
            IdleResult::Resumed {
                segment_id: String::new(),
                accepted_inputs: Vec::new(),
            }
        }
        None => IdleResult::Shutdown,
    }
}

pub(crate) async fn idle_until_resume_or_shutdown<I, S>(
    input_events: &I,
    sink: &S,
    pending: &mut PendingInputBuffer,
    task_access: &dyn task::TaskAccess,
    mut wakeup: Option<&mut crate::application::session::wakeup::WakeupWaiter>,
) -> IdleResult
where
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    loop {
        match await_idle_input(input_events, pending, wakeup.as_deref_mut()).await {
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
            IdleResult::BackgroundTaskWakeup => return IdleResult::BackgroundTaskWakeup,
        }
    }
}
