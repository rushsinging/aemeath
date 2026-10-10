pub mod recall;
use crate::domain::event::{
    ConfigFingerprint, EventActor, EventChange, EventContext, EventOutcome, MemoryEvent,
    MemoryEventOp,
};
use crate::domain::{
    MemoryError, MemoryLayer, ReflectionEngine, ReflectionError, ReflectionErrorCategory,
    ReflectionMessage, ReflectionOutput, ReflectionPrompt, ReflectionRecord,
    ReflectionReferenceTable, ReflectionStatus, ReflectionTokenUsage, ReflectionTrigger,
};
use crate::ports::{MemoryPort, ReflectionApplyResult, ReflectionHistoryStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionExecutionIdentity {
    pub id: String,
    pub timestamp: u64,
    pub trigger: ReflectionTrigger,
    /// 反思游标：本次快照覆盖到的 session active 历史终点（消息计数）。
    /// 仅 Succeeded 落盘时写入 record；失败/取消不推进（None）。
    pub coverage_end: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionExecutionResult {
    pub output: ReflectionOutput,
    pub apply_result: Option<ReflectionApplyResult>,
    pub error_category: Option<ReflectionErrorCategory>,
    pub record_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReflectionWorkflowError {
    #[error("reflection response could not be parsed")]
    Unparseable,
    #[error("reflection response contains an invalid suggestion")]
    InvalidSuggestion,
    #[error("reflection history write failed")]
    HistoryWrite,
}

pub struct ReflectionWorkflow;

impl ReflectionWorkflow {
    /// `now` 是本次反思运行的时间戳（M12 的 TTL 判定需要它），由调用方从反思
    /// identity 传入，避免在此处另起时钟导致两次读取跨秒。
    ///
    /// 返回值携带本次输入的行序号表：模型输出里的引用（`M3` 等）必须在解析阶段
    /// 用同一张表映射回 UUID。
    pub async fn build_prompt(
        messages: &[share::message::Message],
        lang: &str,
        memory: &dyn MemoryPort,
        now: u64,
    ) -> ReflectionPrompt {
        let engine = ReflectionEngine;
        // M12：反思输入排除失效条目（outdated / 被取代 / TTL 过期）。
        let (project_memory, references) = engine.format_memory_summary(
            &memory
                .list(Some(MemoryLayer::Project))
                .await
                .into_iter()
                .filter(|entry| crate::domain::is_reflection_input_eligible(entry, now))
                .collect::<Vec<_>>(),
        );
        let messages = messages
            .iter()
            .map(|message| {
                let role = match message.role {
                    share::message::Role::User => "user",
                    share::message::Role::Assistant => "assistant",
                };
                ReflectionMessage::new(role, message.text_content())
            })
            .collect::<Vec<_>>();
        // 消息摘要字符预算（#1827）：增量路径天然小于预算；PreCompact 被丢弃段与
        // Manual 游标失效回退全量时由预算截断（取最近部分），NEVER 无界进入 prompt。
        const REFLECTION_MESSAGE_BUDGET_CHARS: usize = 24_000;
        let recent_summary =
            engine.recent_messages_summary(&messages, REFLECTION_MESSAGE_BUDGET_CHARS);
        ReflectionPrompt {
            text: engine.build_prompt(&project_memory, &recent_summary, lang),
            references,
        }
    }

    pub async fn append_running(
        history: &dyn ReflectionHistoryStore,
        identity: &ReflectionExecutionIdentity,
    ) -> Result<(), ReflectionWorkflowError> {
        history
            .append(&ReflectionRecord::running(
                identity.id.clone(),
                identity.timestamp,
                identity.trigger,
            ))
            .await
            .map_err(|_| ReflectionWorkflowError::HistoryWrite)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn complete(
        history: &dyn ReflectionHistoryStore,
        memory: &dyn MemoryPort,
        identity: &ReflectionExecutionIdentity,
        raw_response: &str,
        references: &ReflectionReferenceTable,
        _lang: &str,
        auto_apply: bool,
        token_usage: ReflectionTokenUsage,
        duration_ms: u64,
    ) -> Result<ReflectionExecutionResult, ReflectionWorkflowError> {
        Self::emit_reflection_event(
            memory,
            MemoryEventOp::ReflectionTriggered,
            identity,
            EventChange::Reflection {
                summary: format!("trigger={:?};auto_apply={auto_apply}", identity.trigger),
                affected: Vec::new(),
            },
            EventContext {
                coverage_range: identity.coverage_end.map(|end| format!("0..{end}")),
                trigger_summary: Some(format!("{:?}", identity.trigger)),
                ..EventContext::default()
            },
            None,
            None,
        )
        .await;

        let resolved = match ReflectionEngine.parse_output(raw_response, references) {
            Ok(resolved) => resolved,
            Err(error) => {
                let category = match error {
                    ReflectionError::InvalidSuggestion(_) => {
                        ReflectionErrorCategory::InvalidSuggestion
                    }
                    ReflectionError::Parse | ReflectionError::Unparseable => {
                        ReflectionErrorCategory::Parse
                    }
                    ReflectionError::Memory(_) => ReflectionErrorCategory::Apply,
                };
                Self::record_failure(history, identity, category, duration_ms).await?;
                Self::emit_reflection_event(
                    memory,
                    MemoryEventOp::ReflectionCost,
                    identity,
                    EventChange::Reflection {
                        summary: format!(
                            "status=failed;category={category:?};duration_ms={duration_ms};input_tokens={};output_tokens={}",
                            token_usage.input_tokens, token_usage.output_tokens
                        ),
                        affected: Vec::new(),
                    },
                    EventContext {
                        coverage_range: identity
                            .coverage_end
                            .map(|end| format!("0..{end}")),
                        trigger_summary: Some(format!("{:?}", identity.trigger)),
                        ..EventContext::default()
                    },
                    Some(token_usage),
                    Some(duration_ms),
                )
                .await;
                return Err(if category == ReflectionErrorCategory::InvalidSuggestion {
                    ReflectionWorkflowError::InvalidSuggestion
                } else {
                    ReflectionWorkflowError::Unparseable
                });
            }
        };
        // 无法解析的引用（模型编造的标识）逐条记录后跳过：合法建议照常写入，
        // NEVER 让单条坏引用作废整批。
        for reference in &resolved.unresolved {
            log::warn!(
                target: crate::LOG_TARGET,
                "memory_reflection_reference_unresolved field={:?} token={}",
                reference.field,
                reference.token,
            );
        }
        let output = resolved.output;

        let (apply_result, error_category) = if auto_apply {
            match memory.apply_reflection(&output).await {
                Ok(result) => (Some(result), None),
                Err(MemoryError::PartialApply {
                    result_attempted,
                    result_completed,
                    suggestions_added,
                    outdated_marked,
                    superseded,
                }) => (
                    Some(ReflectionApplyResult {
                        attempted: result_attempted,
                        completed: result_completed,
                        suggestions_added,
                        outdated_marked,
                        superseded,
                    }),
                    Some(ReflectionErrorCategory::Apply),
                ),
                Err(error) => {
                    log::warn!(target: crate::LOG_TARGET, "Reflection apply failed: {error}");
                    (None, Some(ReflectionErrorCategory::Apply))
                }
            }
        } else {
            (None, None)
        };

        if let Some(applied) = apply_result.as_ref() {
            Self::emit_reflection_event(
                memory,
                MemoryEventOp::ReflectionApplied,
                identity,
                EventChange::Reflection {
                    summary: format!(
                        "attempted={};completed={};suggestions_added={};outdated_marked={};superseded={}",
                        applied.attempted,
                        applied.completed,
                        applied.suggestions_added,
                        applied.outdated_marked,
                        applied.superseded
                    ),
                    affected: Vec::new(),
                },
                EventContext {
                    coverage_range: identity
                        .coverage_end
                        .map(|end| format!("0..{end}")),
                    trigger_summary: Some(format!("{:?}", identity.trigger)),
                    ..EventContext::default()
                },
                None,
                None,
            )
            .await;
        }

        let status = if error_category.is_some() {
            ReflectionStatus::Failed
        } else {
            ReflectionStatus::Succeeded
        };
        let record = ReflectionRecord {
            id: identity.id.clone(),
            timestamp: identity.timestamp,
            trigger: identity.trigger,
            status,
            output: Some(output.clone()),
            apply_result: apply_result.clone(),
            error_category,
            token_usage: Some(token_usage),
            duration_ms,
            coverage_end: if error_category.is_none() {
                identity.coverage_end
            } else {
                None
            },
        };
        history
            .upsert(&record)
            .await
            .map_err(|_| ReflectionWorkflowError::HistoryWrite)?;

        Self::emit_reflection_event(
            memory,
            MemoryEventOp::ReflectionCost,
            identity,
            EventChange::Reflection {
                summary: format!(
                    "status={status:?};category={error_category:?};duration_ms={duration_ms};input_tokens={};output_tokens={}",
                    token_usage.input_tokens, token_usage.output_tokens
                ),
                affected: Vec::new(),
            },
            EventContext {
                coverage_range: identity
                    .coverage_end
                    .map(|end| format!("0..{end}")),
                trigger_summary: Some(format!("{:?}", identity.trigger)),
                ..EventContext::default()
            },
            Some(token_usage),
            Some(duration_ms),
        )
        .await;

        Ok(ReflectionExecutionResult {
            output,
            apply_result,
            error_category,
            record_id: identity.id.clone(),
        })
    }

    pub async fn record_failure(
        history: &dyn ReflectionHistoryStore,
        identity: &ReflectionExecutionIdentity,
        category: ReflectionErrorCategory,
        duration_ms: u64,
    ) -> Result<(), ReflectionWorkflowError> {
        history
            .upsert(&ReflectionRecord::failed(
                identity.id.clone(),
                identity.timestamp,
                identity.trigger,
                category,
                duration_ms,
            ))
            .await
            .map_err(|_| ReflectionWorkflowError::HistoryWrite)
    }

    async fn emit_reflection_event(
        memory: &dyn MemoryPort,
        op: MemoryEventOp,
        identity: &ReflectionExecutionIdentity,
        change: EventChange,
        context: EventContext,
        _token_usage: Option<ReflectionTokenUsage>,
        _duration_ms: Option<u64>,
    ) {
        let Some(events) = memory.event_append_port() else {
            return;
        };
        let correlation = format!("{op:?}-{}", identity.id);
        let event = MemoryEvent::new(
            correlation.clone(),
            identity.timestamp.saturating_mul(1000),
            op,
            EventOutcome::Succeeded,
            correlation,
            EventActor::ReflectionWorkflow,
            change,
            context,
            ConfigFingerprint::default(),
        );
        if let Err(error) = events.append(&event).await {
            log::warn!(
                target: crate::LOG_TARGET,
                "memory_event_append_failed op={op:?} err={error}"
            );
        }
    }

    /// 预解析（不落盘）：供执行链在调用 [`Self::complete`] 之前判断响应是否
    /// 可解析，从而有界地发起格式修复。
    ///
    /// `Ok(())` 表示响应可正常解析；`Err` 携带精确校验错误摘要（直接用于
    /// 构造发回模型的纠错指令）。解析与引用解析逻辑与 [`Self::complete`] 同源
    /// （同一 `ReflectionEngine`），保证预解析与落盘解析的判定一致。
    pub fn parse_for_repair(
        raw_response: &str,
        references: &ReflectionReferenceTable,
    ) -> Result<(), String> {
        match ReflectionEngine.parse_output(raw_response, references) {
            Ok(_) => Ok(()),
            // InvalidSuggestion 自带字段/索引级细节，直接作为精确摘要；
            // 其余错误取稳定 Display（Unparseable / Parse 的描述）。
            Err(ReflectionError::InvalidSuggestion(detail)) => Err(detail),
            Err(error) => Err(error.to_string()),
        }
    }

    /// 收口悬挂的 Running 反思事实：进程被终止（崩溃/重启）时 `append_running`
    /// 留下的记录永不收口。调用方在反思执行前用「早于本次启动足够久」的阈值
    /// 扫描，把超龄 Running 记录 upsert 为 `Failed(Interrupted)`。
    ///
    /// 返回收口数量；未超龄的运行（并发中的其它进程/本次）不受影响。
    pub async fn reap_stale_running(
        history: &dyn ReflectionHistoryStore,
        now: u64,
        stale_after_secs: u64,
    ) -> Result<usize, ReflectionWorkflowError> {
        let summaries = history
            .list(crate::constants::REFLECTION_REAP_SCAN_LIMIT)
            .await
            .map_err(|_| ReflectionWorkflowError::HistoryWrite)?;
        let mut reaped = 0;
        for summary in summaries {
            if summary.status != ReflectionStatus::Running {
                continue;
            }
            let age_secs = now.saturating_sub(summary.timestamp);
            if age_secs < stale_after_secs {
                continue;
            }
            history
                .upsert(&ReflectionRecord::failed(
                    summary.id.clone(),
                    summary.timestamp,
                    summary.trigger,
                    ReflectionErrorCategory::Interrupted,
                    age_secs,
                ))
                .await
                .map_err(|_| ReflectionWorkflowError::HistoryWrite)?;
            log::info!(
                target: crate::LOG_TARGET,
                "reflection_stale_running_reaped id={} age_secs={}",
                summary.id,
                age_secs,
            );
            reaped += 1;
        }
        Ok(reaped)
    }
}

#[cfg(test)]
#[path = "application_tests.rs"]
mod tests;
