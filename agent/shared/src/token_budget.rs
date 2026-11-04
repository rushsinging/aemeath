//! Provider 无关的 token 成本估算。
//!
//! 这是上下文预算的统一口径：CJK 字符约 0.6 token/字，非 CJK 约 0.3 token/字。
//! 它不替代具体模型的 tokenizer；需要精确计数时由持有模型 tokenizer 资产的适配器
//! 另行实现。对预算门禁而言，稳定的近似值比按字符数比较更接近真实上下文成本。

use crate::constants::{TOKENS_PER_CJK_CHAR, TOKENS_PER_LATIN_CHAR};

/// 估算文本的 token 数。
///
/// 先分别累计 CJK 与非 CJK 字符，再统一向上取整，避免逐段取整造成系统性高估。
pub fn estimate_tokens(text: &str) -> usize {
    estimate_tokens_with_char_rates(text, TOKENS_PER_CJK_CHAR, TOKENS_PER_LATIN_CHAR)
}

/// 使用自定义的单字符 token 比率估算。
///
/// 比率会被 clamp 到 `[0.1, 2.0]`，防止配置或调用方传入异常值。
pub fn estimate_tokens_with_char_rates(
    text: &str,
    tokens_per_cjk_char: f64,
    tokens_per_latin_char: f64,
) -> usize {
    let mut cjk_chars = 0usize;
    let mut latin_chars = 0usize;

    for ch in text.chars() {
        if is_cjk_char(ch) {
            cjk_chars += 1;
        } else {
            latin_chars += 1;
        }
    }

    let cjk_rate = tokens_per_cjk_char.clamp(0.1, 2.0);
    let latin_rate = tokens_per_latin_char.clamp(0.1, 2.0);
    ((cjk_chars as f64) * cjk_rate + (latin_chars as f64) * latin_rate).ceil() as usize
}

/// 兼容旧调用：`bytes_per_token` 换算为拉丁字符比率 `1 / bytes_per_token`，
/// CJK 仍用默认 0.6。新代码应优先 [`estimate_tokens`]。
pub fn estimate_tokens_with_ratio(text: &str, bytes_per_token: f64) -> usize {
    let ratio = bytes_per_token.clamp(2.0, 6.0);
    estimate_tokens_with_char_rates(text, TOKENS_PER_CJK_CHAR, 1.0 / ratio)
}

/// JSON 文本与普通文本使用同一套字符比率估算。
pub fn estimate_json_tokens(text: &str) -> usize {
    estimate_tokens(text)
}

/// 按 token 预算截断文本，超预算时追加 `marker`。
///
/// 截断点按字符边界二分查找，保证追加 marker 后总估算仍不超过预算。
/// 调用方应保证预算足以容纳 marker；预算不足时只返回 marker。
pub fn truncate_to_token_budget(text: &str, max_tokens: usize, marker: &str) -> String {
    if estimate_tokens(text) <= max_tokens {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut low = 0usize;
    let mut high = chars.len();
    while low < high {
        let mid = (low + high).div_ceil(2);
        let mut candidate: String = chars[..mid].iter().collect();
        candidate.push_str(marker);
        if estimate_tokens(&candidate) <= max_tokens {
            low = mid;
        } else {
            high = mid - 1;
        }
    }

    let mut truncated: String = chars[..low].iter().collect();
    truncated.push_str(marker);
    truncated
}

/// 判断字符是否属于按 CJK 高成本计费的字符集。
fn is_cjk_char(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}' // CJK Unified Ideographs Extension A
        | '\u{F900}'..='\u{FAFF}' // CJK Compatibility Ideographs
        | '\u{3000}'..='\u{303F}' // CJK Symbols and Punctuation
        | '\u{FF00}'..='\u{FFEF}' // Fullwidth Forms
        | '\u{AC00}'..='\u{D7AF}' // Hangul Syllables
        | '\u{3040}'..='\u{309F}' // Hiragana
        | '\u{30A0}'..='\u{30FF}' // Katakana
    )
}

#[cfg(test)]
#[path = "token_budget_tests.rs"]
mod token_budget_tests;
