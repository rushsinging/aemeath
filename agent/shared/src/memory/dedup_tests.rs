use super::*;

#[test]
fn test_jaccard_similarity_identical() {
    let score = jaccard_similarity("rust error handling", "rust error handling");

    assert_eq!(score, 1.0);
}

#[test]
fn test_jaccard_similarity_empty() {
    assert_eq!(jaccard_similarity("", ""), 1.0);
    assert_eq!(jaccard_similarity("rust", ""), 0.0);
}

#[test]
fn test_jaccard_similarity_partial() {
    let score = jaccard_similarity("rust error handling", "rust error display");

    assert!(score > 0.4);
    assert!(score < 1.0);
}

#[test]
fn test_jaccard_similarity_none() {
    let score = jaccard_similarity("alpha beta", "gamma delta");

    assert_eq!(score, 0.0);
}

/// CJK 连续段按字符 bigram 分词：中文重复对必须可检出，不再恒为 0。
#[test]
fn test_jaccard_similarity_chinese_bigram() {
    let score = jaccard_similarity(
        "编辑文件前必须先读取内容确认上下文",
        "编辑文件之前必须先读取内容并确认上下文",
    );
    assert!(score > 0.4, "中文相似度必须显著大于 0：{score}");
    assert!(score < 1.0);
}

/// 中英混合文本同时保留拉丁词 token 与 CJK bigram。
#[test]
fn test_jaccard_similarity_mixed_language() {
    let score = jaccard_similarity(
        "Bash 失败后先读取 stderr",
        "Bash 失败后应先读取 stderr 详情",
    );
    assert!(score > 0.3, "中英混合相似度：{score}");
}
