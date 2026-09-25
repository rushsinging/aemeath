use std::collections::HashMap;

use super::{basic_environment_from, BASIC_ENVIRONMENT_VARIABLES};

#[test]
fn basic_environment_keeps_only_present_allowed_variables() {
    let source = HashMap::from([
        ("PATH", "/usr/bin"),
        ("HOME", "/home/test"),
        ("GITHUB_TOKEN", "secret"),
    ]);

    let environment =
        basic_environment_from(|name| source.get(name).map(|value| (*value).to_string()));

    assert_eq!(
        BASIC_ENVIRONMENT_VARIABLES,
        ["PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TERM"]
    );
    assert_eq!(
        environment.get("PATH").map(String::as_str),
        Some("/usr/bin")
    );
    assert_eq!(
        environment.get("HOME").map(String::as_str),
        Some("/home/test")
    );
    assert!(!environment.contains_key("SHELL"));
    assert!(!environment.contains_key("GITHUB_TOKEN"));
}

// ════════════════════════════════════════════════════════════
// env_passthrough：可配置透传白名单（glob 模式）
// ════════════════════════════════════════════════════════════

use super::{env_pattern_matches, passthrough_environment_from};

#[test]
fn env_pattern_matches_supports_exact_prefix_and_infix_star() {
    assert!(env_pattern_matches("SSH_AUTH_SOCK", "SSH_AUTH_SOCK"));
    assert!(!env_pattern_matches("SSH_AUTH_SOCK", "SSH_AUTH_SOCK_EXTRA"));
    // 前缀通配：CMUX_* 命中全部 CMUX_ 变量
    assert!(env_pattern_matches("CMUX_*", "CMUX_SURFACE_ID"));
    assert!(env_pattern_matches("CMUX_*", "CMUX_WORKSPACE_ID"));
    assert!(!env_pattern_matches("CMUX_*", "AEMEATH_SESSION_ID"));
    // 中缀通配
    assert!(env_pattern_matches("*_TOKEN", "GITHUB_TOKEN"));
    assert!(env_pattern_matches("CMUX_*_ID", "CMUX_SURFACE_ID"));
    assert!(env_pattern_matches("CMUX_*_ID", "CMUX_WORKSPACE_ID"));
    assert!(!env_pattern_matches("CMUX_*_ID", "CMUX_WORKSPACE_NAME"));
    // 全通配
    assert!(env_pattern_matches("*", "ANY_VARIABLE"));
}

#[test]
fn passthrough_environment_only_carries_matching_parent_variables() {
    let parent_env = vec![
        ("CMUX_SURFACE_ID", "surface-1"),
        ("CMUX_SOCKET_PATH", "/tmp/cmux.sock"),
        ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
        ("GITHUB_TOKEN", "secret"),
        ("TERM_PROGRAM", "ghostty"),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value.to_string()))
    .collect::<Vec<_>>();
    let patterns = vec!["CMUX_*".to_string(), "SSH_AUTH_SOCK".to_string()];

    let environment = passthrough_environment_from(&patterns, parent_env);

    assert_eq!(
        environment.get("CMUX_SURFACE_ID").map(String::as_str),
        Some("surface-1")
    );
    assert_eq!(
        environment.get("SSH_AUTH_SOCK").map(String::as_str),
        Some("/tmp/agent.sock")
    );
    assert!(!environment.contains_key("GITHUB_TOKEN"));
    assert!(!environment.contains_key("TERM_PROGRAM"));
}

#[test]
fn passthrough_environment_never_carries_aemeath_or_basic_variables() {
    // AEMEATH_* 是按次权威变量，仅由 Dispatcher 注入；父 env 同名值 MUST 被忽略。
    let parent_env = vec![
        ("AEMEATH_SESSION_ID", "forged"),
        ("AEMEATH_HOOK_EVENT", "forged"),
        ("PATH", "/forged/bin"),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value.to_string()))
    .collect::<Vec<_>>();
    let patterns = vec!["*".to_string()];

    let environment = passthrough_environment_from(&patterns, parent_env);

    assert!(environment.is_empty());
}
