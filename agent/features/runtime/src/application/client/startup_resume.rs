//! Startup session 解析：startup resume 视图 → SDK backing 映射（from_args 职责 1）。
//!
//! `from_args_with_workspace` 只消费这里产出的 `(session_id, Option<backing>)`；
//! 逐字段映射语义由 `startup_resume_tests` 锁定，`FinalizeCause` 枚举完备性
//! 由 `sdk_event_mapper_tests` 承担。

use sdk::SdkError;
use sdk::{
    DisplayHistoryIndex, DisplayHistoryStepReference, LocalResumedSessionStep,
    LocalSessionResumeBacking,
};

use super::mapping::map_finalize_cause_to_sdk;
use super::resume_helper::resume_session_to_backing;

/// 将 resume view 逐字段映射为 SDK `LocalSessionResumeBacking`。
///
/// created_at 解析失败时退化为 0（resume 历史展示降级），不阻断 bootstrap。
pub(crate) fn map_resume_view_to_sdk_backing(
    resume_view: context::SessionResumeView,
) -> LocalSessionResumeBacking {
    LocalSessionResumeBacking {
        steps: resume_view
            .display_steps
            .into_iter()
            .map(|step| LocalResumedSessionStep {
                run_id: step.run_id,
                step_id: step.step_id,
                message_segments: step.message_segments,
                finalize_cause: step.finalize_cause.map(map_finalize_cause_to_sdk),
                duration_ms: step.duration_ms,
            })
            .collect(),
        display_history: resume_view
            .display_history
            .map(|index| DisplayHistoryIndex {
                session_id: index.session_id().to_string(),
                generation_revision: index.generation_revision(),
                steps: index
                    .steps()
                    .iter()
                    .map(|step| DisplayHistoryStepReference {
                        run_id: step.run_id().to_string(),
                        step_id: step.step_id().to_string(),
                        member_name: step.member_name().to_string(),
                        estimated_lines: step.estimated_lines(),
                        user_input_history: step.user_input_history().to_vec(),
                        finalize_cause: step.finalize_cause().map(map_finalize_cause_to_sdk),
                        duration_ms: step.duration_ms(),
                    })
                    .collect(),
            }),
        session_id: resume_view.session_id,
        created_at: chrono::DateTime::parse_from_rfc3339(&resume_view.created_at)
            .map(|date_time| date_time.timestamp_millis() as u64)
            .unwrap_or(0),
        compacted: resume_view.compacted,
    }
}

/// 解析启动 session：
/// - `resume` 指定 session → 经 wiring 恢复并映射 SDK backing；
/// - 无 `resume` → 使用 wiring 已提交的 canonical session id；
/// - 恢复失败 → `SdkError::Init`（跨项目 resume 由 wiring 侧拒绝，committed snapshot 不变）。
pub(crate) async fn resolve_startup_session(
    resume: Option<&str>,
    wiring: &context::MainSessionWiring,
) -> Result<(String, Option<LocalSessionResumeBacking>), SdkError> {
    match resume {
        Some(resume_id) => {
            let resume_view =
                resume_session_to_backing(resume_id, wiring)
                    .await
                    .map_err(|error| {
                        SdkError::Init(format!(
                            "startup resume of session {resume_id} failed: {error}"
                        ))
                    })?;
            log::info!(
                target: crate::LOG_TARGET,
                "startup resume: {}",
                resume_view.session_id
            );
            log::debug!(
                target: crate::LOG_TARGET,
                "resume_lifecycle boundary=startup_view stage=view_created session_id={} display_index_steps={} legacy_steps={} active_messages={}",
                resume_view.session_id,
                resume_view
                    .display_history
                    .as_ref()
                    .map_or(0, |index| index.steps().len()),
                resume_view.display_steps.len(),
                resume_view.active_messages.len(),
            );
            let session_id = resume_view.session_id.clone();
            let startup_resume = map_resume_view_to_sdk_backing(resume_view);
            Ok((session_id, Some(startup_resume)))
        }
        None => {
            let session_id = wiring.committed_session().id.clone();
            log::info!(target: crate::LOG_TARGET, "session started");
            Ok((session_id, None))
        }
    }
}

#[cfg(test)]
#[path = "startup_resume_tests.rs"]
mod tests;
