use memory::api::{
    ReflectionApplyStatus, ReflectionErrorCategory, ReflectionSafeSummary, ReflectionStatus,
    ReflectionTrigger,
};
use sdk::{
    ReflectionApplyStatusView, ReflectionErrorCategoryView, ReflectionHistoryView,
    ReflectionStatusView, ReflectionTokenUsageView, ReflectionTriggerView, SdkError,
};

use super::accessors::AgentClientImpl;

type Result<T> = std::result::Result<T, SdkError>;

/// Lists persisted Reflection history and maps each record through Memory's
/// safe-summary projection before crossing the SDK boundary.
pub(super) async fn list_reflection_history_impl(
    me: &AgentClientImpl,
    limit: usize,
) -> Result<Vec<ReflectionHistoryView>> {
    let records = me
        .inner
        .context
        .resources
        .reflection_history
        .list(limit)
        .await
        .map_err(|error| SdkError::Internal(format!("List reflection history failed: {error}")))?;
    Ok(records
        .iter()
        .map(memory::api::ReflectionRecord::safe_summary)
        .map(summary_to_sdk)
        .collect())
}

fn summary_to_sdk(summary: ReflectionSafeSummary) -> ReflectionHistoryView {
    ReflectionHistoryView {
        id: summary.id,
        timestamp: summary.timestamp,
        trigger: match summary.trigger {
            ReflectionTrigger::Interval => ReflectionTriggerView::Interval,
            ReflectionTrigger::PreCompact => ReflectionTriggerView::PreCompact,
            ReflectionTrigger::Manual => ReflectionTriggerView::Manual,
        },
        status: match summary.status {
            ReflectionStatus::Running => ReflectionStatusView::Running,
            ReflectionStatus::Succeeded => ReflectionStatusView::Succeeded,
            ReflectionStatus::Failed => ReflectionStatusView::Failed,
        },
        deviations: summary.deviations,
        suggestions: summary.suggestions,
        outdated: summary.outdated,
        apply_status: match summary.apply_status {
            ReflectionApplyStatus::NotApplied => ReflectionApplyStatusView::NotApplied,
            ReflectionApplyStatus::Applied => ReflectionApplyStatusView::Applied,
            ReflectionApplyStatus::PartiallyApplied => ReflectionApplyStatusView::PartiallyApplied,
        },
        error_category: summary.error_category.map(|category| match category {
            ReflectionErrorCategory::LlmCall => ReflectionErrorCategoryView::LlmCall,
            ReflectionErrorCategory::EmptyResponse => ReflectionErrorCategoryView::EmptyResponse,
            ReflectionErrorCategory::Parse => ReflectionErrorCategoryView::Parse,
            ReflectionErrorCategory::InvalidSuggestion => {
                ReflectionErrorCategoryView::InvalidSuggestion
            }
            ReflectionErrorCategory::Apply => ReflectionErrorCategoryView::Apply,
            ReflectionErrorCategory::History => ReflectionErrorCategoryView::History,
            ReflectionErrorCategory::Cancelled => ReflectionErrorCategoryView::Cancelled,
            ReflectionErrorCategory::TimedOut => ReflectionErrorCategoryView::TimedOut,
        }),
        token_usage: summary.token_usage.map(|usage| ReflectionTokenUsageView {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        }),
        duration_ms: summary.duration_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_sdk_view_contains_only_metadata_and_counts() {
        let view = summary_to_sdk(ReflectionSafeSummary {
            id: "reflection-1".into(),
            timestamp: 42,
            trigger: ReflectionTrigger::PreCompact,
            status: ReflectionStatus::Succeeded,
            deviations: 1,
            suggestions: 2,
            outdated: 3,
            apply_status: ReflectionApplyStatus::Applied,
            error_category: None,
            token_usage: Some(memory::api::ReflectionTokenUsage {
                input_tokens: 10,
                output_tokens: 20,
            }),
            duration_ms: 30,
        });
        assert_eq!(view.trigger, ReflectionTriggerView::PreCompact);
        assert_eq!(view.suggestions, 2);
        assert_eq!(view.token_usage.unwrap().input_tokens, 10);
    }
}
