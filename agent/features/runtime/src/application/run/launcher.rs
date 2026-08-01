//! RunLauncher — 唯一拥有 ActiveRun 注册/释放、shared `run_loop`
//! 调用与 typed terminal 映射的应用服务。
//!
//! Main 和派生 Run 各自构造窄 observer 后，调用 `launch` 并交入完整
//! `RunInstance`；调用方不再拆散 `Run`、`RunExecutionState` 与 `RuntimeContext`。
//!
//! #1280: await_user_input 在 adapter 内部 async park（Main 等 channel，
//! Sub 等 FixedInputBuffer），engine 在 await_interruptible 内消费。
//! run_loop 只返回 Terminal。launcher 不需要 AwaitUser re-entry。

use crate::application::loop_engine::{
    execute_prepared_loop, fail_run, LoopDirective, LoopEngineError,
};
use crate::application::run::creation::RunInstance;
use crate::domain::agent_run::ActiveRunPort;
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

/// 唯一启动入口，消费完整 `RunInstance`，不按来源重建或拆散运行状态。
pub async fn launch(
    instance: &mut RunInstance,
    cancel: CancellationToken,
    active_run: Arc<dyn ActiveRunPort>,
    loop_context: &mut crate::application::loop_engine::RunLoop<'_>,
) -> RunLaunchResult {
    let run_id = instance.run().id().clone();
    if instance.run().parent_id().is_none() {
        active_run.activate_session(run_id.clone(), cancel.clone());
    } else {
        active_run.activate(run_id.clone(), cancel.clone());
    }
    let (run, execution, context) = instance.execution_parts_mut();

    let result = match execute_prepared_loop(run, execution, context, &cancel, loop_context).await {
        Ok(LoopDirective::Terminal) | Ok(LoopDirective::AwaitUser) => RunLaunchResult::Terminal,
        Err(error) => {
            log::error!(
                target: crate::LOG_TARGET,
                "[run_launcher] shared run_loop failed: {error}"
            );
            if let Err(terminal_error) =
                fail_run(run, execution, loop_context, error.to_string()).await
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
