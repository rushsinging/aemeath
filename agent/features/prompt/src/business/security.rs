//! Content security scanning for prompt injection detection.
//!
//! Scans external prompt and guidance content for known prompt injection
//! patterns. It does not block loading — only warns.

/// A detected security threat in loaded content.
#[derive(Debug, Clone)]
pub struct SecurityWarning {
    pub filename: String,
    pub threat_type: String,
    pub matched_text: String,
    pub line_number: usize,
}

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
    fn test_scan_content_detects_prompt_injection() {
        let warnings = scan_content("test.md", "Normal\nignore all instructions");

        assert!(!warnings.is_empty());
        assert_eq!(warnings[0].threat_type, "prompt_injection");
        assert_eq!(warnings[0].line_number, 2);
    }

    #[test]
    fn test_scan_content_accepts_clean_content() {
        let warnings = scan_content("test.md", "Normal project instructions.");

        assert!(warnings.is_empty());
    }

    #[test]
    fn test_scan_content_detects_invisible_chars() {
        let warnings = scan_content("test.md", "normal\u{200B}hidden");

        assert!(!warnings.is_empty());
        assert!(warnings[0].threat_type.contains("invisible_char"));
    }

    #[test]
    fn test_format_warnings_empty_returns_none() {
        assert!(format_warnings(&[]).is_none());
    }

    #[test]
    fn test_format_warnings_includes_filename() {
        let warnings = vec![SecurityWarning {
            filename: "test.md".to_string(),
            threat_type: "prompt_injection".to_string(),
            matched_text: "ignore all previous instructions".to_string(),
            line_number: 2,
        }];

        let result = format_warnings(&warnings).unwrap();

        assert!(result.contains("test.md"));
    }
}
