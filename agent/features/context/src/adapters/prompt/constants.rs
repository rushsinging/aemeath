//! prompt 子域常量（#1146 双轨归位）。

pub(crate) const THREAT_PATTERNS: &[(&str, &str)] = &[
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

pub(crate) const INVISIBLE_CHARS: &[(char, &str)] = &[
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
