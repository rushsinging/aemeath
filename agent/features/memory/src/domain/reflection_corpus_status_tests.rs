//! #1987：corpus 现状注入 reflect prompt（相似对检测与现状段渲染）。

use super::*;
use crate::domain::{MemoryId, MemorySource};

fn engine() -> ReflectionEngine {
    ReflectionEngine
}

fn pair_entry(content: &str) -> MemoryEntry {
    MemoryEntry::new(
        MemoryId::now_v7(),
        1,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        content,
        MemorySource::Llm,
    )
    .unwrap()
}

#[test]
fn top_similar_pairs_flags_high_jaccard_duplicates_only() {
    // CJK 连续段按字符 bigram 分词（#1987）：中文重复对必须能被检出——
    // 真 corpus eligible 以中文为主，纯词法 token 会漏检全部重复。
    let entries = vec![
        pair_entry("Bash 长耗时命令必须放后台跑，靠完成通知继续"),
        pair_entry("Bash 长耗时命令必须放后台跑，靠完成通知推进"),
        pair_entry("TUI 主题色板使用 Catppuccin Macchiato"),
    ];
    let pairs = ReflectionEngine::top_similar_pairs(&entries, 0.70, 8);
    assert_eq!(pairs.len(), 1, "只应命中高相似对: {pairs:?}");
    assert_eq!(pairs[0].left_ordinal, 1);
    assert_eq!(pairs[0].right_ordinal, 2);
    assert!(pairs[0].score >= 0.70);
}

#[test]
fn top_similar_pairs_respects_top_k_cap() {
    let mut entries = Vec::new();
    for index in 0..6 {
        entries.push(pair_entry(&format!(
            "重复事实编号 {index}：Bash 长耗时命令必须放后台跑"
        )));
    }
    let pairs = ReflectionEngine::top_similar_pairs(&entries, 0.30, 3);
    assert_eq!(pairs.len(), 3, "top_k MUST 截断: {pairs:?}");
    assert!(
        pairs.iter().all(|pair| pair.score >= 0.30),
        "阈值以下条目 MUST 被过滤"
    );
}

#[test]
fn corpus_status_section_renders_stats_and_duplicate_pairs() {
    let status = ReflectionCorpusStatus {
        archive_count: 170,
        category_counts: vec![(MemoryCategory::Decision, 71), (MemoryCategory::Fact, 76)],
        duplicate_pairs: vec![ReflectionDuplicatePair {
            left_ordinal: 3,
            right_ordinal: 17,
            score: 0.73,
        }],
    };
    let section = ReflectionEngine::corpus_status_section(&status, "zh");
    assert!(section.contains("# 记忆库现状"));
    assert!(section.contains("archive=170"));
    assert!(section.contains("decision=71"));
    assert!(section.contains("fact=76"));
    assert!(section.contains("M3 × M17"));
    assert!(section.contains("0.73"));
    assert!(section.contains("MUST 给出 synthesizes"));

    let en = ReflectionEngine::corpus_status_section(&status, "en");
    assert!(en.contains("# Corpus status"));
    assert!(en.contains("M3 × M17"));
    assert!(en.contains("must produce a synthesizes"));
}

#[test]
fn corpus_status_section_without_duplicates_says_none() {
    let status = ReflectionCorpusStatus {
        archive_count: 4,
        category_counts: vec![(MemoryCategory::Fact, 2)],
        duplicate_pairs: vec![],
    };
    let zh = ReflectionEngine::corpus_status_section(&status, "zh");
    assert!(zh.contains("未检出高相似对"));
    let en = ReflectionEngine::corpus_status_section(&status, "en");
    assert!(en.contains("no high-similarity pairs"));
}

#[test]
fn build_prompt_embeds_corpus_status_section() {
    let status = ReflectionCorpusStatus {
        archive_count: 9,
        category_counts: vec![(MemoryCategory::Fact, 2)],
        duplicate_pairs: vec![ReflectionDuplicatePair {
            left_ordinal: 1,
            right_ordinal: 2,
            score: 0.71,
        }],
    };
    let capacity = ReflectionCapacityBudget {
        active_count: 2,
        target_active_entries: 150,
        max_entries: 200,
    };
    let zh = engine().build_prompt("MEM", "SUMMARY", "zh", capacity, Some(&status));
    assert!(zh.contains("# 记忆库现状"));
    assert!(zh.contains("M1 × M2"));
    assert!(zh.contains("新增建议需搭配等量淘汰"));
    let en = engine().build_prompt("MEM", "SUMMARY", "en", capacity, Some(&status));
    assert!(en.contains("# Corpus status"));
    assert!(en.contains("each new suggestion must be paired"));

    // None：现状段整体不渲染，占位符不泄漏。
    let bare = engine().build_prompt(
        "MEM",
        "SUMMARY",
        "zh",
        ReflectionCapacityBudget {
            active_count: 2,
            target_active_entries: 150,
            max_entries: 200,
        },
        None,
    );
    assert!(!bare.contains("# 记忆库现状"));
    assert!(!bare.contains("{corpus_status}"));
}
