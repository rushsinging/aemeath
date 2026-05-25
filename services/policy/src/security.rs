//! Content security scanning for prompt injection detection.
//!
//! Scans external content (CLAUDE.md, guidance files) for known
//! prompt injection patterns. Does NOT block loading — only warns.

/// A detected security threat in loaded content.
#[derive(Debug, Clone)]
pub struct SecurityWarning {
    pub filename: String,
    pub threat_type: String,
    pub matched_text: String,
    pub line_number: usize,
}

/// Known prompt injection patterns: (regex_pattern, threat_type_label)
const THREAT_PATTERNS: &[(&str, &str)] = &[
    (
        r"(?i)ignore\s+(previous|all|above|prior)\s+instructions",
        "prompt_injection",
    ),
    (r"(?i)do\s+not\s+tell\s+the\s+user", "deception"),
    (r"(?i)you\s+are\s+now\s+(?:a|an|DAN)", "jailbreak"),
    (r"(?i)system:\s*", "role_hijack"),
    (
        r"(?i)forget\s+(everything|all|your)\s+(above|previous|prior)",
        "prompt_injection",
    ),
    (r"(?i)new\s+instructions?\s*:", "prompt_injection"),
];

/// Invisible Unicode characters that may hide injected text.
const INVISIBLE_CHARS: &[(char, &str)] = &[
    ('\u{200B}', "zero-width space"),
    ('\u{200C}', "zero-width non-joiner"),
    ('\u{200D}', "zero-width joiner"),
    ('\u{200E}', "left-to-right mark"),
    ('\u{200F}', "right-to-left mark"),
    ('\u{202A}', "left-to-right embedding"),
    ('\u{202B}', "right-to-left embedding"),
    ('\u{202C}', "pop directional formatting"),
    ('\u{202D}', "left-to-right override"),
    ('\u{202E}', "right-to-left override"),
    ('\u{FEFF}', "byte order mark"),
];

/// Scan content for prompt injection patterns and invisible characters.
pub fn scan_content(filename: &str, content: &str) -> Vec<SecurityWarning> {
    let mut warnings = Vec::new();

    for (pattern, threat_type) in THREAT_PATTERNS {
        if let Ok(re) = regex::Regex::new(pattern) {
            for mat in re.find_iter(content) {
                let line_number = content[..mat.start()].lines().count() + 1;
                warnings.push(SecurityWarning {
                    filename: filename.to_string(),
                    threat_type: threat_type.to_string(),
                    matched_text: mat.as_str().to_string(),
                    line_number,
                });
            }
        }
    }

    for (line_num, line) in content.lines().enumerate() {
        for (ch, name) in INVISIBLE_CHARS {
            if line.contains(*ch) {
                warnings.push(SecurityWarning {
                    filename: filename.to_string(),
                    threat_type: format!("invisible_char: {}", name),
                    matched_text: format!("U+{:04X}", *ch as u32),
                    line_number: line_num + 1,
                });
            }
        }
    }

    warnings
}

/// Format warnings as a prefix string to prepend to injected content.
pub fn format_warnings(warnings: &[SecurityWarning]) -> Option<String> {
    if warnings.is_empty() {
        return None;
    }

    let details: Vec<String> = warnings
        .iter()
        .map(|w| {
            format!(
                "  - [{}] line {}: \"{}\"",
                w.threat_type, w.line_number, w.matched_text
            )
        })
        .collect();

    Some(format!(
        "[security: possible prompt injection detected in {}]\n{}",
        warnings[0].filename,
        details.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_prompt_injection() {
        let content = "Normal text\nignore all instructions\nMore text";
        let warnings = scan_content("test.md", content);
        assert!(!warnings.is_empty());
        assert_eq!(warnings[0].threat_type, "prompt_injection");
        assert_eq!(warnings[0].line_number, 2);
    }

    #[test]
    fn test_clean_content() {
        let content = "This is a normal CLAUDE.md with instructions for coding.";
        let warnings = scan_content("test.md", content);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_invisible_chars() {
        let content = "normal text\u{200B}hidden";
        let warnings = scan_content("test.md", content);
        assert!(!warnings.is_empty());
        assert!(warnings[0].threat_type.contains("invisible_char"));
    }

    #[test]
    fn test_format_warnings_empty() {
        assert!(format_warnings(&[]).is_none());
    }

    #[test]
    fn test_format_warnings_nonempty() {
        let warnings = vec![SecurityWarning {
            filename: "test.md".to_string(),
            threat_type: "prompt_injection".to_string(),
            matched_text: "ignore all previous instructions".to_string(),
            line_number: 2,
        }];
        let result = format_warnings(&warnings);
        assert!(result.is_some());
        assert!(result.unwrap().contains("test.md"));
    }
}
