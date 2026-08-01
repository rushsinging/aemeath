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
