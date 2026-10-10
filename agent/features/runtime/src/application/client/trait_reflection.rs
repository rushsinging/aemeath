use memory::api::reflection::ReflectionStatus;
use memory::api::reflection::{
    ReflectionApplyStatus, ReflectionErrorCategory, ReflectionSafeSummary, ReflectionTrigger,
};
use sdk::{
    ReflectionApplyStatusView, ReflectionErrorCategoryView, ReflectionHistoryView,
    ReflectionStatusView, ReflectionTokenUsageView, ReflectionTriggerView, SdkError,
};

use super::accessors::AgentClientImpl;

type Result<T> = std::result::Result<T, SdkError>;

/// Lists persisted Reflection history and maps each record through Memory's
/// safe-summary view before crossing the SDK boundary.
pub(super) async fn list_reflection_history_impl(
    me: &AgentClientImpl,
    limit: usize,
) -> Result<Vec<ReflectionHistoryView>> {
    let records = me
        .inner
        .shell
        .runtime_context_factory
        .services()
        .reflection_history
        .clone()
        // /reflect 是本地用户的显式内容查询：投影偏差文本与建议内容
        // （Safe 边界由 Memory 默认 `list` 保留，此处走显式内容投影）。
        .list_with_content(limit)
        .await
        .map_err(|error| SdkError::Internal(format!("List reflection history failed: {error}")))?;
    Ok(records.into_iter().map(summary_to_sdk).collect())
}

fn suggestion_to_sdk(suggestion: memory::api::MemorySuggestion) -> sdk::MemorySuggestionView {
    sdk::MemorySuggestionView {
        layer: match suggestion.layer {
            memory::api::MemoryLayer::Global => sdk::MemoryLayerView::Global,
            memory::api::MemoryLayer::Project => sdk::MemoryLayerView::Project,
        },
        category: match suggestion.category {
            memory::api::MemoryCategory::Fact => sdk::MemoryCategoryView::Fact,
            memory::api::MemoryCategory::Decision => sdk::MemoryCategoryView::Decision,
            memory::api::MemoryCategory::Preference => sdk::MemoryCategoryView::Preference,
            memory::api::MemoryCategory::Pattern => sdk::MemoryCategoryView::Pattern,
            memory::api::MemoryCategory::Pitfall => sdk::MemoryCategoryView::Pitfall,
        },
        content: suggestion.content,
        tags: suggestion.tags,
        reason: suggestion.reason,
    }
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
            ReflectionErrorCategory::Interrupted => ReflectionErrorCategoryView::Interrupted,
        }),
        token_usage: summary.token_usage.map(|usage| ReflectionTokenUsageView {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        }),
        duration_ms: summary.duration_ms,
        deviation_texts: summary.deviation_texts.unwrap_or_default(),
        suggested_memories: summary
            .suggested_memories
            .unwrap_or_default()
            .into_iter()
            .map(suggestion_to_sdk)
            .collect(),
    }
}

#[cfg(test)]
#[path = "trait_reflection_tests.rs"]
mod tests;
