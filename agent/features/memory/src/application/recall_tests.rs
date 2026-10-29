//! recall_relevant 行为测试：召回 + 评分编排、阈值所需概率带出、降级边界。

use super::*;
use crate::adapters::MemoryPolicy;
use crate::domain::{MemoryCategory, MemoryId, MemoryLayer, MemorySource};

/// 概率按 content 是否含关键词分配的评分桩。

async fn memory_with(entries: Vec<&str>) -> crate::adapters::InMemoryMemory {
    let memory = crate::adapters::InMemoryMemory::new(MemoryPolicy {
        max_entries: 50,
        similarity_threshold: 0.8,
        target_active_entries: 150,
        default_ttl_secs: 0,
    })
    .expect("policy 合法");
    for content in entries {
        memory
            .write(
                MemoryEntry::new(
                    MemoryId::now_v7(),
                    10,
                    MemoryLayer::Project,
                    MemoryCategory::Fact,
                    content,
                    MemorySource::User,
                )
                .expect("entry 构造"),
            )
            .await
            .expect("写入");
    }
    memory
}

/// #2146：ScoringPort 实现权归 systemone crate；关键词命中打分替身。
fn keyword_scoring(keyword: &'static str) -> std::sync::Arc<dyn systemone::ScoringPort> {
    systemone::test_support::scripted_scoring(move |_state, questions| {
        let criteria = match &questions[0] {
            systemone::ScoringQuestion::Choice { criteria, .. } => criteria,
            _ => panic!("应为 Choice"),
        };
        let probabilities: Vec<(String, f64)> = criteria
            .iter()
            .enumerate()
            .map(|(index, (_, content))| {
                let p = if content.contains(keyword) {
                    0.85
                } else {
                    0.05
                };
                (index.to_string(), p)
            })
            .collect();
        Ok(vec![systemone::ScoringAnswer::choice(
            probabilities
                .iter()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(k, _)| k.clone())
                .unwrap_or_default(),
            probabilities,
            0.9,
            systemone::CalibrationLevel::Raw,
        )
        .expect("答案构造")])
    })
}

/// 恒不可用替身。
fn failing_scoring() -> std::sync::Arc<dyn systemone::ScoringPort> {
    systemone::test_support::unavailable_scoring("服务未启动")
}

/// Noul 单值替身。
fn single_noul_scoring(p_true: f64) -> std::sync::Arc<dyn systemone::ScoringPort> {
    systemone::test_support::scripted_scoring(move |_state, questions| {
        assert!(
            matches!(questions[0], systemone::ScoringQuestion::Noul { .. }),
            "单候选必须走 Noul 题型"
        );
        Ok(vec![systemone::ScoringAnswer::noul(
            p_true,
            systemone::CalibrationLevel::Raw,
        )
        .expect("答案构造")])
    })
}

#[tokio::test]
async fn recall_relevant_returns_top_k_with_probabilities() {
    let memory = memory_with(vec![
        "rust ownership 的所有权规则",
        "Python 的 GC 机制",
        "rust 生命周期注解实践",
    ])
    .await;
    let scorer = keyword_scoring("ownership");

    let recalled = recall_relevant(&memory, scorer.as_ref(), "rust 所有权", 4_242, 20, 2)
        .await
        .expect("评分可用");

    assert_eq!(recalled.len(), 2, "top_k=2 截断");
    assert_eq!(recalled[0].entry.content, "rust ownership 的所有权规则");
    assert!(
        recalled[0].probability > recalled[1].probability,
        "概率降序：{} vs {}",
        recalled[0].probability,
        recalled[1].probability
    );
    assert!(recalled[0].probability > 0.5, "阈值门可用的概率语义");
}

#[tokio::test]
async fn recall_relevant_returns_empty_when_no_lexical_hits() {
    let memory = memory_with(vec!["完全不相关的内容"]).await;
    let scorer = keyword_scoring("x");

    let recalled = recall_relevant(&memory, scorer.as_ref(), "zzz 无匹配词", 4_242, 20, 3)
        .await
        .expect("词法零命中应 Ok(空)");

    assert!(recalled.is_empty());
}

#[tokio::test]
async fn recall_relevant_propagates_scoring_unavailable() {
    let memory = memory_with(vec!["rust ownership 规则", "rust 生命周期"]).await;

    let outcome = recall_relevant(&memory, failing_scoring().as_ref(), "rust", 4_242, 20, 3).await;

    let error = outcome.expect_err("评分失败必须向上传播");
    assert_eq!(error.kind(), systemone::UnavailableKind::Connect);
}

#[tokio::test]
async fn recall_single_candidate_uses_noul_gate() {
    let memory = memory_with(vec!["git worktree 的创建步骤"]).await;
    let scorer = single_noul_scoring(0.88);

    let recalled = recall_relevant(&memory, scorer.as_ref(), "worktree", 4_242, 20, 3)
        .await
        .expect("评分可用");

    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].entry.content, "git worktree 的创建步骤");
    assert!(
        (recalled[0].probability - 0.88).abs() < 1e-9,
        "p_true 即概率"
    );
}

#[tokio::test]
async fn event_read_ops_per_message_recall_emits_candidates_with_content() {
    use crate::domain::event::{EventChange, MemoryEventOp};
    use crate::ports::RecordingEventAppend;
    use std::sync::Arc;

    struct PortWithEvents {
        inner: crate::adapters::InMemoryMemory,
        events: Arc<dyn crate::ports::MemoryEventAppendPort>,
        recorder: Arc<RecordingEventAppend>,
    }
    impl crate::sealed::Sealed for PortWithEvents {}

    #[async_trait::async_trait]
    impl crate::ports::MemoryPort for PortWithEvents {
        async fn retrieve_for_inject(
            &self,
            query: &crate::ports::MemoryQuery,
        ) -> crate::ports::MemorySearchResult {
            self.inner.retrieve_for_inject(query).await
        }
        async fn search(
            &self,
            query: &crate::domain::MemorySearchQuery,
        ) -> crate::ports::MemorySearchResult {
            self.inner.search(query).await
        }
        async fn write(
            &self,
            entry: crate::domain::MemoryEntry,
        ) -> Result<crate::ports::WriteResult, crate::domain::MemoryError> {
            self.inner.write(entry).await
        }
        async fn update(
            &self,
            id: &MemoryId,
            content: &str,
        ) -> Result<bool, crate::domain::MemoryError> {
            self.inner.update(id, content).await
        }
        async fn delete(&self, id: &MemoryId) -> Result<bool, crate::domain::MemoryError> {
            self.inner.delete(id).await
        }
        async fn pin(
            &self,
            id: &MemoryId,
            pinned: bool,
        ) -> Result<bool, crate::domain::MemoryError> {
            self.inner.pin(id, pinned).await
        }
        async fn mark_outdated(&self, id: &MemoryId) -> Result<bool, crate::domain::MemoryError> {
            self.inner.mark_outdated(id).await
        }
        async fn apply_reflection(
            &self,
            output: &crate::domain::ReflectionOutput,
        ) -> Result<crate::ports::ReflectionApplyResult, crate::domain::MemoryError> {
            self.inner.apply_reflection(output).await
        }
        async fn archive(&self, ids: &[MemoryId]) -> Result<bool, crate::domain::MemoryError> {
            self.inner.archive(ids).await
        }
        async fn restore(
            &self,
            id: &MemoryId,
        ) -> Result<crate::ports::RestoreResult, crate::domain::MemoryError> {
            self.inner.restore(id).await
        }
        async fn compact(&self) -> Result<crate::ports::CompactResult, crate::domain::MemoryError> {
            self.inner.compact().await
        }
        async fn list(&self, layer: Option<MemoryLayer>) -> Vec<crate::domain::MemoryEntry> {
            self.inner.list(layer).await
        }
        async fn stats(&self) -> crate::ports::MemoryStats {
            self.inner.stats().await
        }
        fn event_append_port(&self) -> Option<Arc<dyn crate::ports::MemoryEventAppendPort>> {
            Some(Arc::clone(&self.events))
        }
    }

    // ≥2 候选走 Choice，避免单候选 Noul 与 KeywordScoringPort 不匹配。
    let inner = memory_with(vec![
        "worktree isolation matters",
        "unrelated cooking tip",
        "another worktree note",
    ])
    .await;
    let recorder = Arc::new(RecordingEventAppend::default());
    let port = PortWithEvents {
        inner,
        events: recorder.clone(),
        recorder: Arc::clone(&recorder),
    };
    let scorer = keyword_scoring("worktree");
    let recalled = recall_relevant(&port, scorer.as_ref(), "worktree", 4_242, 20, 3)
        .await
        .expect("recall");
    assert!(!recalled.is_empty());
    // search 也会 emit Search（inner 无 event port，故只见 PerMessageRecall）。
    let events = port.recorder.events();
    let recall_event = events
        .iter()
        .find(|event| event.op == MemoryEventOp::PerMessageRecall)
        .expect("PerMessageRecall event");
    match &recall_event.change {
        EventChange::Read {
            candidates,
            hit_count,
            ..
        } => {
            assert_eq!(*hit_count as usize, candidates.len());
            assert!(candidates
                .iter()
                .any(|entry| entry.content.contains("worktree")));
            assert!(candidates.iter().all(|entry| !entry.content.is_empty()));
        }
        other => panic!("expected Read, got {other:?}"),
    }
    assert!(recall_event
        .context
        .trigger_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("probabilities=")));
}

#[tokio::test]
async fn recall_relevant_excludes_stale_entries_from_injection() {
    // 注入链路必须以 exclude_stale 视角检索；先正常写入再标记过期，
    // 避免 write 时清理失效条目使测试未触及 search 过滤器。
    let memory = memory_with(vec!["rust ownership 权威指南", "rust 生命周期注解实践"]).await;
    let stale = MemoryEntry::new(
        MemoryId::now_v7(),
        10,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        "rust ownership 旧版借用编译器特殊操作守则",
        MemorySource::User,
    )
    .expect("entry 构造");
    let stale_id = stale.id;
    memory.write(stale).await.expect("写入待过期条目");
    assert!(memory.mark_outdated(&stale_id).await.expect("标记过期"));
    let scorer = keyword_scoring("ownership");

    let recalled = recall_relevant(&memory, scorer.as_ref(), "rust 所有权指南", 4_242, 20, 2)
        .await
        .expect("评分可用");

    assert!(
        recalled.iter().all(|hit| !hit.entry.outdated),
        "注入结果不得包含 outdated 条目，got {recalled:?}"
    );
    assert!(!recalled.is_empty(), "有效条目应顶上名额而非被过期条目挤占");
}
