use super::*;

#[test]
fn test_glob_match_exact() {
    assert!(glob_match("glm-5.1", "glm-5.1"));
    assert!(glob_match("deepseek-*", "deepseek-chat"));
    assert!(!glob_match("glm-5", "glm-5.1"));
}

#[test]
fn test_glob_match_wildcard() {
    assert!(glob_match("glm-*", "glm-5.1"));
    assert!(glob_match("*-v4-*", "deepseek-v4-flash"));
    assert!(!glob_match("deepseek-*", "glm-5.1"));
}

#[test]
fn test_glob_match_double_wildcard() {
    assert!(glob_match("*", "anything"));
    assert!(glob_match("*glm*", "glm-5.1"));
}

#[test]
fn test_prefix_match_case_insensitive() {
    let model_lower = "GLM-5.1".to_lowercase();
    assert!(model_lower.starts_with(&"glm".to_lowercase()));
    assert!(!model_lower.starts_with(&"deepseek".to_lowercase()));
}
