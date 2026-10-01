//! Content security scanning for prompt injection detection.
//!
//! Scans external prompt and guidance content for known prompt injection
//! patterns. It does not block loading — only warns.

/// A detected security threat in loaded content.
use super::constants::{INVISIBLE_CHARS, THREAT_PATTERNS};

#[derive(Debug, Clone)]
pub struct SecurityWarning {
    pub filename: String,
    pub threat_type: String,
    pub matched_text: String,
    pub line_number: usize,
}

/// Context-owned assessment result for guidance content.
#[derive(Debug, Clone)]
pub struct GuidanceAssessment {
    pub content: String,
    pub warnings: Vec<SecurityWarning>,
}

pub fn assess_guidance(filename: &str, content: &str) -> GuidanceAssessment {
    let warnings = scan_content(filename, content);
    let assessed_content = format_warnings(&warnings)
        .map(|prefix| format!("{prefix}\n\n{content}"))
        .unwrap_or_else(|| content.to_string());

    GuidanceAssessment {
        content: assessed_content,
        warnings,
    }
}

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
#[path = "security_tests.rs"]
mod tests;
