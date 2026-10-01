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
