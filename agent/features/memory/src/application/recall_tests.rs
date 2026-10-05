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
