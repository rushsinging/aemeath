//! RunLauncher — 唯一拥有 Run 创建、ActiveRun 注册/释放、shared `run_loop`
//! 调用与 typed terminal 映射的应用服务。
//!
//! Main 和 Sub 各自构造 `RunLaunchInput` + `RunLoopPort` adapter，调
//! `RunLauncher::launch`，不自行创建 `Run` / cancel / registry。
//!
//! #1280: await_user_input 在 adapter 内部 async park（Main 等 channel，
//! Sub 等 FixedInputBuffer），engine 在 await_interruptible 内消费。
//! run_loop 只返回 Terminal。launcher 不需要 AwaitUser re-entry。

use crate::application::loop_engine::{
    fail_run, run_loop, LoopDirective, LoopEngineError, RunLoopPort,
};
use crate::domain::agent_run::{ActiveRunPort, Run, RunId, RunSpec};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// 纯值启动参数。Main/Sub 分别构造，差异只在字段值。
pub struct RunLaunchInput {
    pub run_id: RunId,
    pub spec: RunSpec,
    pub parent_run_id: Option<RunId>,
    pub cancel: CancellationToken,
}

/// launcher 返回的 typed 终态。
#[derive(Debug)]
pub enum RunLaunchResult {
    /// Run 正常终止（Completed / Failed / Terminated）。
    Terminal,
    /// shared run_loop 返回引擎错误。
    Failed(LoopEngineError),
}

/// 唯一启动入口。
///
/// 创建 `Run`、注册 ActiveRun、调用 shared `run_loop`、返回 typed result。
/// Main/Sub 的所有 Run 生命周期启动经此函数。
pub async fn launch<P>(
    input: RunLaunchInput,
    active_run: Arc<dyn ActiveRunPort>,
    port: &mut P,
) -> RunLaunchResult
where
    P: RunLoopPort,
{
    let mut run = Run::with_id(input.run_id, input.spec, input.parent_run_id);
    let cancel = input.cancel;
    let run_id = run.id().clone();
    active_run.activate(run_id.clone(), cancel.clone());

    let result = match run_loop(&mut run, &cancel, port).await {
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
    result
}

#[cfg(test)]
#[path = "run_launcher_tests.rs"]
mod tests;
