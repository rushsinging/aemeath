//! Hook 子进程的固定基础环境白名单。

use std::collections::HashMap;

pub(super) const BASIC_ENVIRONMENT_VARIABLES: [&str; 6] =
    ["PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TERM"];

pub(super) fn capture_basic_environment() -> HashMap<String, String> {
    basic_environment_from(|name| std::env::var(name).ok())
}

pub(super) fn basic_environment_from(
    mut read: impl FnMut(&str) -> Option<String>,
) -> HashMap<String, String> {
    BASIC_ENVIRONMENT_VARIABLES
        .into_iter()
        .filter_map(|name| read(name).map(|value| (name.to_string(), value)))
        .collect()
}

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;
