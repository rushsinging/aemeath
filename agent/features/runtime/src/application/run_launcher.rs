//! RunLauncher — 唯一拥有 Run 创建、ActiveRun 注册/释放、shared `run_loop`
//! 调用与 typed terminal 映射的应用服务。
//!
//! Main 和 Sub 各自构造 `RunLaunchInput` + `RunLoopPort` adapter，调
//! `RunLauncher::launch`，不自行创建 `Run` / cancel / registry。

use crate::application::loop_engine::{run_loop, LoopDirective, LoopEngineError, RunLoopPort};
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
    /// Run 进入 AwaitingUser，需要外部喂入输入后重新 launch。
    AwaitUser,
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
        Ok(LoopDirective::Terminal) => RunLaunchResult::Terminal,
        Ok(LoopDirective::AwaitUser) => RunLaunchResult::AwaitUser,
        Err(error) => {
            log::error!(
                target: crate::LOG_TARGET,
                "[run_launcher] shared run_loop failed: {error}"
            );
            RunLaunchResult::Failed(error)
        }
    };

    active_run.clear(&run_id);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::active_run::ActiveRunRegistry;
    use crate::application::loop_engine::{
        DrainEpoch, DrainOutcome, ModelStep, RunLoopPort, StepTokenUsage, StuckDecision,
        ToolGuardDecision, ToolStep,
    };
    use crate::domain::agent_run::{ActiveRunPort, RunDomainEvent, RunStepId};

    /// Minimal RunLoopPort stub: drain returns EmptyAndSealed immediately,
    /// so run_loop completes in one iteration.
    struct StubPort {
        events_emitted: Vec<RunDomainEvent>,
    }

    #[async_trait::async_trait]
    impl RunLoopPort for StubPort {
        async fn drain_input(
            &mut self,
            _expected_epoch: DrainEpoch,
        ) -> Result<DrainOutcome, LoopEngineError> {
            Ok(DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(0),
            })
        }
        async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
            Ok(false)
        }
        async fn compact(&mut self, _cancel: &CancellationToken) -> Result<(), LoopEngineError> {
            Ok(())
        }
        async fn invoke_model(
            &mut self,
            _cancel: &CancellationToken,
        ) -> Result<(ModelStep, StepTokenUsage), LoopEngineError> {
            Ok((
                ModelStep::Complete {
                    text: String::new(),
                },
                StepTokenUsage::default(),
            ))
        }
        async fn execute_tools(
            &mut self,
            _run_id: &RunId,
            _step_id: &RunStepId,
            _calls: &[(crate::application::agent::ToolCall, ToolGuardDecision)],
            _cancel: &CancellationToken,
        ) -> Result<ToolStep, LoopEngineError> {
            Ok(ToolStep::Continue)
        }
        async fn on_stuck(&mut self, _decision: &StuckDecision) -> Result<(), LoopEngineError> {
            Ok(())
        }
        async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError> {
            self.events_emitted.extend(events);
            Ok(())
        }
    }

    #[tokio::test]
    async fn launch_creates_run_and_returns_terminal() {
        let registry: Arc<dyn ActiveRunPort> = Arc::new(ActiveRunRegistry::default());
        let mut port = StubPort {
            events_emitted: Vec::new(),
        };
        let input = RunLaunchInput {
            run_id: RunId::new_v7(),
            spec: RunSpec::main(),
            parent_run_id: None,
            cancel: CancellationToken::new(),
        };

        let result = launch(input, registry.clone(), &mut port).await;
        assert!(matches!(result, RunLaunchResult::Terminal));
    }

    #[tokio::test]
    async fn launch_clears_active_run_after_completion() {
        let registry = Arc::new(ActiveRunRegistry::default());
        let run_id = RunId::new_v7();
        let mut port = StubPort {
            events_emitted: Vec::new(),
        };
        let input = RunLaunchInput {
            run_id: run_id.clone(),
            spec: RunSpec::main(),
            parent_run_id: None,
            cancel: CancellationToken::new(),
        };

        let _ = launch(input, registry.clone(), &mut port).await;

        // After launch returns, the run should be cleared from the registry.
        assert!(!registry.claim_terminal(&run_id));
    }
}
