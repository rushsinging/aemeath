use std::collections::HashSet;

pub fn jaccard_similarity(left: &str, right: &str) -> f64 {
    let left_tokens = tokenize(left);
    let right_tokens = tokenize(right);

    if left_tokens.is_empty() && right_tokens.is_empty() {
        return 1.0;
    }
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return 0.0;
    }

    let intersection = left_tokens.intersection(&right_tokens).count();
    let union = left_tokens.union(&right_tokens).count();

    intersection as f64 / union as f64
}

fn tokenize(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter_map(|token| {
            let token = token.trim().to_lowercase();
            if token.is_empty() {
                None
            } else {
                Some(token)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
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
}
