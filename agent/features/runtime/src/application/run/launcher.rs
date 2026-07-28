//! RunLauncher — 唯一拥有 Run 创建、ActiveRun 注册/释放、shared `run_loop`
//! 调用与 typed terminal 映射的应用服务。
//!
//! Main 和 Sub 各自构造窄能力 adapter，调用 `RunLauncher::launch_prepared`，
//! 不自行创建 `Run` / cancel / registry。
//!
//! #1280: await_user_input 在 adapter 内部 async park（Main 等 channel，
//! Sub 等 FixedInputBuffer），engine 在 await_interruptible 内消费。
//! run_loop 只返回 Terminal。launcher 不需要 AwaitUser re-entry。

use crate::application::loop_engine::{
    execute_prepared_loop, fail_run, LoopDirective, LoopEngineError,
};
use crate::application::run::context::RuntimeContext;
use crate::application::run::execution_state::RunExecutionState;
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
pub async fn launch_prepared(
    mut run: Run,
    execution: &mut RunExecutionState,
    context: &RuntimeContext,
    cancel: CancellationToken,
    active_run: Arc<dyn ActiveRunPort>,
    port: &mut dyn crate::application::loop_engine::LoopCapabilityAdapter,
) -> RunLaunchResult {
    let run_id = run.id().clone();
    active_run.activate(run_id.clone(), cancel.clone());

    let result = match execute_prepared_loop(&mut run, execution, context, &cancel, port).await {
        Ok(LoopDirective::Terminal) | Ok(LoopDirective::AwaitUser) => RunLaunchResult::Terminal,
        Err(error) => {
            log::error!(
                target: crate::LOG_TARGET,
                "[run_launcher] shared run_loop failed: {error}"
            );
            if let Err(terminal_error) =
                fail_run(&mut run, execution, port, error.to_string()).await
            {
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
#[path = "launcher_tests.rs"]
mod tests;
