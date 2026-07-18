use std::collections::HashMap;

/// Environment variable keys that are too dangerous to allow MCP servers to override.
const BLOCKED_ENV_KEYS: &[&str] = &[
    "PATH",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "HOME",
    "USER",
    "SHELL",
    "IFS",
    "CDPATH",
    "ENV",
    "BASH_ENV",
    "TERMINFO",
    "TERMINFO_DIRS",
    "LOCPATH",
    "NLSPATH",
];

/// Validate that the MCP server command is safe to execute.
///
/// Rejects:
/// - Relative paths (must be absolute)
/// - Shell metacharacters (`|`, `&`, `;`, `$`, backticks, `>`, `<`, `(`, `)`)
/// - Known shell names (sh, bash, zsh, fish, etc.)
pub(crate) fn validate_command(command: &str) -> Result<(), String> {
    if command.contains('|')
        || command.contains('&')
        || command.contains(';')
        || command.contains('$')
        || command.contains('`')
        || command.contains('>')
        || command.contains('<')
        || command.contains('(')
        || command.contains(')')
    {
        return Err(format!(
            "MCP command '{}' contains shell metacharacters — rejected for security",
            command
        ));
    }

    if !command.starts_with('/') {
        return Err(format!(
            "MCP command '{}' must be an absolute path — rejected for security",
            command
        ));
    }

    // Block obvious shell invocations
    let basename = command.rsplit('/').next().unwrap_or(command);
    let blocked_commands = [
        "sh", "bash", "zsh", "fish", "dash", "ksh", "csh", "tcsh", "python", "python3", "node",
        "ruby", "perl", "lua",
    ];
    if blocked_commands.contains(&basename) {
        return Err(format!(
            "MCP command '{}' is a shell/interpreter — use the actual executable path instead",
            command
        ));
    }

    Ok(())
}

/// Filter out dangerous environment variables from the MCP server config.
pub(crate) fn filter_env(
    env: &std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    env.iter()
        .filter(|(k, _)| {
            let upper = k.to_uppercase();
            !BLOCKED_ENV_KEYS.contains(&upper.as_str())
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

pub fn validate_remote_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("invalid MCP url: {e}"))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" => {
            let host = parsed.host_str().unwrap_or_default();
            if host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]" {
                Ok(())
            } else {
                Err("remote MCP url must use https unless it points to localhost".to_string())
            }
        }
        other => Err(format!("unsupported MCP url scheme: {other}")),
    }
}

pub fn redact_headers(headers: &HashMap<String, String>) -> HashMap<String, String> {
    headers
        .iter()
        .map(|(key, value)| {
            let lower = key.to_ascii_lowercase();
            let sensitive = lower == "authorization"
                || lower == "cookie"
                || lower == "x-api-key"
                || lower == "proxy-authorization";
            if sensitive {
                (key.clone(), "<redacted>".to_string())
            } else {
                (key.clone(), value.clone())
            }
        })
        .collect()
}
