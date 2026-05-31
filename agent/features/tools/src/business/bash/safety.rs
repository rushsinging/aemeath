/// List of commands allowed at the start of a chain (directory changes, etc.)
const CHAIN_START_COMMANDS: &[&str] = &["cd", "pushd", "popd", "dirs"];

/// Detect redirection to a real device path (excluding the safe sinks
/// `/dev/null`, `/dev/stdout`, `/dev/stderr`, `/dev/fd/*`, `/dev/tty`).
/// Writes to these are universally safe; writes to e.g. `/dev/sda` are
/// genuinely destructive and must be blocked.
pub fn is_suspicious_dev_write(cmd: &str) -> bool {
    const SAFE_DEVS: &[&str] = &["/dev/null", "/dev/stdout", "/dev/stderr", "/dev/tty"];
    let mut rest = cmd;
    loop {
        let Some(pos) = rest.find("/dev/") else {
            return false;
        };
        // Look backwards to check this is a `>` or `>>` redirection target
        let before = &rest[..pos];
        let is_redirect = before.trim_end_matches([' ', '\t']).ends_with('>');
        if is_redirect {
            // Check the specific device path
            let after = &rest[pos..];
            let end = after
                .find(|c: char| c.is_whitespace() || c == ';' || c == '|' || c == '&' || c == ')')
                .unwrap_or(after.len());
            let dev_path = &after[..end];
            let is_safe = SAFE_DEVS.contains(&dev_path) || dev_path.starts_with("/dev/fd/");
            if !is_safe {
                return true;
            }
        }
        rest = &rest[pos + 5..];
    }
}

/// Check if a command is allowed in a chain (after &&, ||, etc.)
pub fn is_safe_chain_command(command: &str) -> bool {
    let cmd = command.trim();
    if cmd.is_empty() {
        return false;
    }

    // Check first command word
    let first_word = cmd.split_whitespace().next().unwrap_or("");
    let lower = first_word.to_lowercase();

    // Allow chain-start commands
    if CHAIN_START_COMMANDS.iter().any(|c| lower == *c) {
        return true;
    }

    // Allow read-only commands
    is_readonly_command(cmd)
}

/// Extract the inner command strings from $(…) and backtick command substitutions.
/// Handles nested $() by tracking parenthesis depth.
/// Returns a (possibly empty) list of inner command strings found.
pub fn extract_command_substitution_contents(command: &str) -> Vec<String> {
    let mut results = Vec::new();
    let chars: Vec<char> = command.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Look for $(…) with nesting support
        if i + 1 < len && chars[i] == '$' && chars[i + 1] == '(' {
            let start = i + 2; // start of inner content
            let mut depth = 1i32;
            i += 2;
            while i < len && depth > 0 {
                if chars[i] == '(' {
                    depth += 1;
                } else if chars[i] == ')' {
                    depth -= 1;
                }
                i += 1;
            }
            // depth == 0 means we found the matching closing ')'
            // inner content is chars[start..i-1] (the char before the closing ')')
            if depth == 0 {
                let inner: String = chars[start..i - 1].iter().collect();
                let inner = inner.trim().to_string();
                if !inner.is_empty() {
                    results.push(inner);
                }
            }
        } else if chars[i] == '`' {
            // Look for backtick substitution (no nesting in standard backticks;
            // backticks cannot be nested in bash, so we just find the next closing backtick)
            let start = i + 1;
            i += 1;
            while i < len && chars[i] != '`' {
                i += 1;
            }
            if i < len {
                // Found closing backtick
                let inner: String = chars[start..i].iter().collect();
                let inner = inner.trim().to_string();
                if !inner.is_empty() {
                    results.push(inner);
                }
                i += 1; // skip closing backtick
            }
            // else: unterminated backtick, skip
        } else {
            i += 1;
        }
    }

    results
}

/// Check for shell injection patterns.
/// Unlike before, this allows safe command chains like `cd /tmp && ls`.
pub fn check_shell_injection(command: &str) -> Option<&'static str> {
    let cmd = command.trim();

    // Command substitution: extract inner commands and validate each one
    if cmd.contains("$(") || cmd.contains("`") {
        let inner_cmds = extract_command_substitution_contents(cmd);
        if inner_cmds.is_empty() {
            // Unterminated or empty substitution — block it
            return Some("command substitution");
        }
        for inner in &inner_cmds {
            // Recursively check the inner command for shell injection patterns
            if let Some(reason) = check_shell_injection(inner) {
                return Some(reason);
            }
            // Also check if the inner command itself is a destructive/dangerous command
            if check_command_safety(inner).is_some() {
                return Some("dangerous command in substitution");
            }
            // The inner command must be a safe command (read-only or chain-safe)
            if !is_safe_chain_command(inner) {
                return Some("command substitution");
            }
        }
    }

    // Background execution: standalone & with spaces around it (not >&1 or 2>&1)
    // Only block true background execution like "sleep 10 &"
    // Handle redirections like 2>&1 and >&2 which are fd redirections, not background
    let cmd_for_bg_check = cmd
        .replace("2>&1", "")
        .replace(">&2", "")
        .replace(">&1", "")
        .replace("1>&2", "");
    for (i, ch) in cmd_for_bg_check.char_indices() {
        if ch == '&' {
            let before = cmd_for_bg_check[..i].trim_end();
            let after = cmd_for_bg_check[i + 1..].trim_start();
            if !before.is_empty() && !after.is_empty() && after != ">" && after != ">&" {
                return Some("background execution");
            }
        }
    }

    // I/O redirection to devices — only block non-standard ones.
    // /dev/null, /dev/stdout, /dev/stderr are universally safe sinks.
    if is_suspicious_dev_write(cmd) {
        return Some("write to device");
    }

    // Newline injection — allow newlines inside heredoc bodies, since
    // `cmd << MARKER ... MARKER` is a common, safe idiom.
    if cmd.contains('\n') && !cmd.contains("<<") {
        return Some("newline injection");
    }

    // Check command chains for dangerous patterns
    // Split by && and ||, but preserve the operators for checking
    let segments: Vec<&str> = cmd
        .split(['&', '|', ';'])
        .filter(|s| !s.trim().is_empty())
        .collect();

    // If we have multiple segments, check each one
    let has_chain = cmd.contains("&&") || cmd.contains("||") || cmd.contains(";");
    if has_chain && segments.len() > 1 {
        for segment in segments {
            if !is_safe_chain_command(segment) {
                return Some("unsafe command in chain");
            }
        }
    }

    None
}

/// Check for destructive/dangerous commands.
/// Aligned with Claude Code TS bashSecurity.ts patterns.
pub fn check_command_safety(command: &str) -> Option<&'static str> {
    let cmd = command.trim();
    let lower = cmd.to_lowercase();

    // === File system destruction ===
    if lower.contains("rm -rf") || lower.contains("rm -fr") {
        return Some("recursive force delete");
    }
    if lower.contains("mkfs") {
        return Some("format filesystem");
    }
    if lower.contains("dd if=") {
        return Some("raw disk write");
    }
    if is_suspicious_dev_write(cmd) {
        return Some("write to device");
    }

    // === Git dangerous operations ===
    if lower.contains("git reset --hard") {
        return Some("discard all changes");
    }
    if lower.contains("git clean -f") {
        return Some("delete untracked files");
    }
    if lower.contains("git checkout -- .") || lower.contains("git checkout .") {
        return Some("discard all changes");
    }
    if lower.contains("git push --force") || lower.contains("git push -f") {
        return Some("force push (may overwrite remote history)");
    }
    if lower.contains("git branch -D") {
        return Some("force delete branch");
    }
    if lower.contains("git restore .") {
        return Some("discard all changes");
    }

    // === Database destruction ===
    if lower.contains("drop table") {
        return Some("drop database table");
    }
    if lower.contains("drop database") {
        return Some("drop database");
    }
    if lower.contains("truncate table") {
        return Some("truncate table");
    }
    if lower.contains("delete from") && !lower.contains("where") {
        return Some("delete all rows (no WHERE clause)");
    }

    // === System-level danger ===
    if lower.contains("chmod -r 777") {
        return Some("open permissions recursively");
    }
    if lower.contains("chown -r") {
        return Some("recursive ownership change");
    }
    if cmd.contains(":(){ :|:& };:") {
        return Some("fork bomb");
    }
    if lower.contains("/proc/") && lower.contains("environ") {
        return Some("access process environment");
    }

    // === Shell injection patterns ===
    // Zsh module loading (can bypass sandbox)
    let zsh_dangerous = [
        "zmodload", "sysopen", "sysread", "syswrite", "zpty", "ztcp", "zsocket",
    ];
    for pattern in &zsh_dangerous {
        if lower.contains(pattern) {
            return Some("dangerous zsh module command");
        }
    }

    // Process substitution and command injection in arguments
    // (These are only blocked when they appear in suspicious contexts)
    if cmd.contains("$(") && (lower.contains("curl") || lower.contains("wget")) {
        return Some("command substitution in network command");
    }

    // === Kill/signal operations ===
    if lower.contains("kill -9") || lower.contains("killall") || lower.contains("pkill") {
        // Only block if targeting system processes
        if lower.contains("init") || lower.contains("systemd") || lower.contains("launchd") {
            return Some("kill system process");
        }
    }

    // === Dangerous redirections ===
    if lower.contains("> /etc/") || lower.contains(">> /etc/") {
        return Some("write to system config directory");
    }
    if lower.contains("> /usr/") || lower.contains(">> /usr/") {
        return Some("write to system directory");
    }

    None
}

/// List of commands considered read-only / safe to auto-approve.
/// Aligned with Claude Code TS READONLY_COMMANDS.
///
/// NOTE: Commands that can execute arbitrary code or modify state are NOT included
/// here and will go through normal approval flow. Removed dangerous entries:
/// - `python -c`, `node -e`, `ruby -e`: can execute arbitrary code
/// - `curl -s`, `wget -q`: can download content, access internal networks
/// - `xargs`: takes arbitrary commands as arguments
/// - `tee`: writes to files
/// - `gh api`: can make POST/PUT/DELETE requests
/// - `command`: shell builtin that can bypass command lookup
const READONLY_COMMANDS: &[&str] = &[
    "ls",
    "cat",
    "head",
    "tail",
    "wc",
    "nl",
    "stat",
    "file",
    "du",
    "df",
    "pwd",
    "whoami",
    "hostname",
    "uname",
    "date",
    "uptime",
    "env",
    "printenv",
    "echo",
    "printf",
    "which",
    "where",
    "type",
    "find",
    "locate",
    "tree",
    "grep",
    "rg",
    "ag",
    "ack",
    "git status",
    "git log",
    "git diff",
    "git show",
    "git branch",
    "git remote",
    "git tag",
    "git blame",
    "git stash list",
    "cargo check",
    "cargo test",
    "cargo clippy",
    "cargo doc",
    "npm test",
    "npm run lint",
    "npx tsc --noEmit",
    "jq",
    "yq",
    "sort",
    "uniq",
    "cut",
    "tr",
    "docker ps",
    "docker images",
    "docker logs",
    "kubectl get",
    "kubectl describe",
    "kubectl logs",
    "gh pr view",
    "gh issue view",
    "man",
    "help",
    "less",
    "more",
];

/// Check if a command is read-only (safe to auto-approve)
pub fn is_readonly_command(command: &str) -> bool {
    let cmd = command.trim();

    // Reject if command contains output redirection
    if cmd.contains(" > ") || cmd.contains(" >> ") || cmd.ends_with('>') {
        return false;
    }

    // Reject command substitution in arguments (but check_shell_injection handles chains)
    // This allows safe commands like `echo $(whoami)` to pass through
    // as long as they don't contain other dangerous patterns

    // Check first command in pipe chain (pipes are still blocked)
    let first = cmd.split('|').next().unwrap_or(cmd).trim();
    for pattern in READONLY_COMMANDS {
        if first.starts_with(pattern) {
            return true;
        }
    }
    false
}
