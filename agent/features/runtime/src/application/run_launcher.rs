//! RunLauncher — 唯一拥有 Run 创建、ActiveRun 注册/释放、shared `run_loop`
//! 调用与 typed terminal 映射的应用服务。
//!
//! Main 和 Sub 各自构造 `RunLaunchInput` + `LoopEnginePort` adapter，调
//! `RunLauncher::launch`，不自行创建 `Run` / cancel / registry。
//!
//! #1280: await_user_input 在 adapter 内部 async park（Main 等 channel，
//! Sub 等 FixedInputBuffer），engine 在 await_interruptible 内消费。
//! run_loop 只返回 Terminal。launcher 不需要 AwaitUser re-entry。

use crate::application::loop_engine::{
    execute_prepared_loop, fail_run, LoopDirective, LoopEngineError, LoopEnginePort,
};
use crate::application::run_execution_state::RunExecutionState;
use crate::application::runtime_context::RuntimeContext;
use crate::domain::agent_run::{ActiveRunPort, Run};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// launcher 返回的 typed 终态。
#[derive(Debug)]
pub enum RunLaunchResult {
    /// Run 正常终止（Completed / Failed / Terminated）。
    Terminal,
    /// shared run_loop 返回引擎错误。
    Failed(LoopEngineError),
}

/// 唯一启动入口，消费 PreparedRun 中创建的领域 Run，不再按 identity 重建。
pub async fn launch_prepared<P>(
    mut run: Run,
    mut execution: RunExecutionState,
    context: &RuntimeContext,
    cancel: CancellationToken,
    active_run: Arc<dyn ActiveRunPort>,
    port: &mut P,
) -> (RunLaunchResult, RunExecutionState)
where
    P: LoopEnginePort
        + crate::application::loop_engine::InputPort
        + crate::application::loop_engine::EventSinkPort
        + crate::application::loop_engine::RunControlPort
        + crate::application::loop_engine::RunLifecyclePort,
{
    let run_id = run.id().clone();
    active_run.activate(run_id.clone(), cancel.clone());

    let result = match execute_prepared_loop(&mut run, &mut execution, context, &cancel, port).await
    {
        Ok(LoopDirective::Terminal) | Ok(LoopDirective::AwaitUser) => RunLaunchResult::Terminal,
        Err(error) => {
            log::error!(
                target: crate::LOG_TARGET,
                "[run_launcher] shared run_loop failed: {error}"
            );
            if let Err(terminal_error) = fail_run(&mut run, port, error.to_string()).await {
                log::error!(
                    target: crate::LOG_TARGET,
                    "[run_launcher] failed to publish RunFailed: {terminal_error}"
                );
            }
            RunLaunchResult::Failed(error)
        }
    };

    active_run.clear(&run_id);
    (result, execution)
}

/// 迁移期兼容入口：旧调用方仍由 adapter 暂存 execution；P5 生产路径必须调用
/// [`launch_prepared`]，P6 删除该入口及 adapter-owned execution。
pub async fn launch<P>(
    mut run: Run,
    cancel: CancellationToken,
    active_run: Arc<dyn ActiveRunPort>,
    port: &mut P,
) -> RunLaunchResult
where
    P: LoopEnginePort
        + crate::application::loop_engine::InputPort
        + crate::application::loop_engine::EventSinkPort
        + crate::application::loop_engine::RunControlPort
        + crate::application::loop_engine::RunLifecyclePort,
{
    let run_id = run.id().clone();
    active_run.activate(run_id.clone(), cancel.clone());
    let result = match crate::application::loop_engine::run_loop(&mut run, &cancel, port).await {
        Ok(LoopDirective::Terminal) | Ok(LoopDirective::AwaitUser) => RunLaunchResult::Terminal,
        Err(error) => {
            if let Err(terminal_error) = fail_run(&mut run, port, error.to_string()).await {
                log::error!(
                    target: crate::LOG_TARGET,
                    "[run_launcher] failed to publish RunFailed: {terminal_error}"
                );
            }
            RunLaunchResult::Failed(error)
        }
    };
    active_run.clear(&run_id);
    result
}

#[cfg(test)]
#[path = "run_launcher_tests.rs"]
mod tests;
