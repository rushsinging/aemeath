use std::collections::HashSet;

/// 词法 token + CJK 字符 bigram 的 Jaccard 相似度。
///
/// 拉丁/数字段按词切分；CJK 连续段按字符 bigram 切分，避免整句中文被
/// 视作单个 token 导致相似度恒为 0（对齐 #1987 的分词口径）。
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
    let mut tokens = HashSet::new();
    let mut latin_run = String::new();
    let mut cjk_run = String::new();
    for character in text.chars() {
        if is_cjk(character) {
            flush_latin_run(&mut latin_run, &mut tokens);
            cjk_run.push(character);
        } else if character.is_alphanumeric() {
            flush_cjk_run(&mut cjk_run, &mut tokens);
            latin_run.push(character.to_ascii_lowercase());
        } else {
            flush_latin_run(&mut latin_run, &mut tokens);
            flush_cjk_run(&mut cjk_run, &mut tokens);
        }
    }
    flush_latin_run(&mut latin_run, &mut tokens);
    flush_cjk_run(&mut cjk_run, &mut tokens);
    tokens
}

fn flush_latin_run(run: &mut String, tokens: &mut HashSet<String>) {
    if !run.is_empty() {
        tokens.insert(std::mem::take(run));
    }
}

fn flush_cjk_run(run: &mut String, tokens: &mut HashSet<String>) {
    if run.is_empty() {
        return;
    }
    let chars: Vec<char> = run.chars().collect();
    if chars.len() == 1 {
        tokens.insert(chars[0].to_string());
    } else {
        for window in chars.windows(2) {
            tokens.insert(window.iter().collect());
        }
    }
    run.clear();
}

fn is_cjk(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{3040}'..='\u{30FF}'
            | '\u{AC00}'..='\u{D7AF}'
    )
}

#[cfg(test)]
#[path = "dedup_tests.rs"]
mod tests;
