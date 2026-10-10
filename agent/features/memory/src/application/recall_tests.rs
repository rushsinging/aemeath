//! recall_relevant 行为测试：召回 + 评分编排、阈值所需概率带出、降级边界。

use super::*;
use crate::adapters::MemoryPolicy;
use crate::domain::{MemoryCategory, MemoryId, MemoryLayer, MemorySource};

/// 概率按 content 是否含关键词分配的评分桩。
struct KeywordScoringPort {
    keyword: &'static str,
}

#[async_trait::async_trait]
impl systemone::ScoringPort for KeywordScoringPort {
    async fn answer(
        &self,
        _state: &systemone::ScoringState,
        questions: &[systemone::ScoringQuestion],
    ) -> Result<Vec<systemone::ScoringAnswer>, systemone::ScoringUnavailable> {
        let criteria = match &questions[0] {
            systemone::ScoringQuestion::Choice { criteria, .. } => criteria,
            _ => panic!("应为 Choice"),
        };
        let probabilities: Vec<(String, f64)> = criteria
            .iter()
            .enumerate()
            .map(|(index, (_, content))| {
                let p = if content.contains(self.keyword) {
                    0.85
                } else {
                    0.05
                };
                (index.to_string(), p)
            })
            .collect();
        let (top_key, top_probability) = probabilities
            .iter()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(key, probability)| (key.clone(), *probability))
            .expect("非空");
        Ok(vec![systemone::ScoringAnswer::choice(
            top_key,
            probabilities,
            top_probability,
            systemone::CalibrationLevel::Raw,
        )
        .expect("答案构造")])
    }
}

async fn memory_with(entries: Vec<&str>) -> crate::adapters::InMemoryMemory {
    let memory = crate::adapters::InMemoryMemory::new(MemoryPolicy {
        max_entries: 50,
        similarity_threshold: 0.8,
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

#[tokio::test]
async fn recall_relevant_returns_top_k_with_probabilities() {
    let memory = memory_with(vec![
        "rust ownership 的所有权规则",
        "Python 的 GC 机制",
        "rust 生命周期注解实践",
    ])
    .await;
    let scorer = KeywordScoringPort {
        keyword: "ownership",
    };

    let recalled = recall_relevant(&memory, &scorer, "rust 所有权", 4_242, 20, 2)
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
    let scorer = KeywordScoringPort { keyword: "x" };

    let recalled = recall_relevant(&memory, &scorer, "zzz 无匹配词", 4_242, 20, 3)
        .await
        .expect("词法零命中应 Ok(空)");

    assert!(recalled.is_empty());
}

#[tokio::test]
async fn recall_relevant_propagates_scoring_unavailable() {
    struct FailingScoringPort;
    #[async_trait::async_trait]
    impl systemone::ScoringPort for FailingScoringPort {
        async fn answer(
            &self,
            _state: &systemone::ScoringState,
            _questions: &[systemone::ScoringQuestion],
        ) -> Result<Vec<systemone::ScoringAnswer>, systemone::ScoringUnavailable> {
            Err(systemone::ScoringUnavailable::new(
                systemone::UnavailableKind::Connect,
                "服务未启动",
            ))
        }
    }
    let memory = memory_with(vec!["rust ownership 规则", "rust 生命周期"]).await;

    let outcome = recall_relevant(&memory, &FailingScoringPort, "rust", 4_242, 20, 3).await;

    let error = outcome.expect_err("评分失败必须向上传播");
    assert_eq!(error.kind(), systemone::UnavailableKind::Connect);
}

/// 单候选走 Noul 题型的评分桩。
struct SingleNoulScoringPort {
    p_true: f64,
}

#[async_trait::async_trait]
impl systemone::ScoringPort for SingleNoulScoringPort {
    async fn answer(
        &self,
        _state: &systemone::ScoringState,
        questions: &[systemone::ScoringQuestion],
    ) -> Result<Vec<systemone::ScoringAnswer>, systemone::ScoringUnavailable> {
        assert!(
            matches!(questions[0], systemone::ScoringQuestion::Noul { .. }),
            "单候选必须走 Noul 题型"
        );
        Ok(vec![systemone::ScoringAnswer::noul(
            self.p_true,
            systemone::CalibrationLevel::Raw,
        )
        .expect("答案构造")])
    }
}

#[tokio::test]
async fn recall_single_candidate_uses_noul_gate() {
    let memory = memory_with(vec!["git worktree 的创建步骤"]).await;
    let scorer = SingleNoulScoringPort { p_true: 0.88 };

    let recalled = recall_relevant(&memory, &scorer, "worktree", 4_242, 20, 3)
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
    let scorer = KeywordScoringPort {
        keyword: "worktree",
    };
    let recalled = recall_relevant(&port, &scorer, "worktree", 4_242, 20, 3)
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
