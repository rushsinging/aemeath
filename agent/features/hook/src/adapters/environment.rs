//! Hook 子进程的受管环境白名单与可配置透传。

use std::collections::HashMap;

pub(super) const BASIC_ENVIRONMENT_VARIABLES: [&str; 6] =
    ["PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TERM"];

/// `AEMEATH_*` 前缀的按次权威变量命名空间：仅由 Dispatcher 注入，
/// 父环境同名值 **NEVER** 透传（防伪造）。
const RESERVED_PREFIX: &str = "AEMEATH_";

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

/// 受管环境 = 基础白名单 + 命中透传模式的父环境变量。
pub(super) fn managed_environment(env_passthrough: &[String]) -> HashMap<String, String> {
    let mut environment = capture_basic_environment();
    environment.extend(capture_passthrough_environment(env_passthrough));
    environment
}

pub(super) fn capture_passthrough_environment(
    env_passthrough: &[String],
) -> HashMap<String, String> {
    passthrough_environment_from(env_passthrough, std::env::vars())
}

/// 从父环境变量集合中筛出命中任一 glob 模式且不属于保留命名空间 /
/// 基础白名单的变量。模式仅支持 `*` 通配（任意段，可组合）。
pub(super) fn passthrough_environment_from(
    env_passthrough: &[String],
    parent_env: impl IntoIterator<Item = (String, String)>,
) -> HashMap<String, String> {
    parent_env
        .into_iter()
        .filter(|(name, _)| {
            !name.starts_with(RESERVED_PREFIX)
                && !BASIC_ENVIRONMENT_VARIABLES.contains(&name.as_str())
                && env_passthrough
                    .iter()
                    .any(|pattern| env_pattern_matches(pattern, name))
        })
        .collect()
}

/// glob 匹配：`*` 匹配任意字符序列（含空），其余字符精确比较。
pub(super) fn env_pattern_matches(pattern: &str, name: &str) -> bool {
    // 按 `*` 切段：段之间由任意序列衔接，首段锚定开头、末段锚定结尾。
    let segments: Vec<&str> = pattern.split('*').collect();
    let Some((first, rest)) = segments.split_first() else {
        return false;
    };
    let Some(mut remainder) = name.strip_prefix(first) else {
        return false;
    };
    let Some((last, middle)) = rest.split_last() else {
        // 无 `*`：整串精确匹配
        return remainder.is_empty();
    };
    for segment in middle {
        let Some(index) = remainder.find(segment) else {
            return false;
        };
        // find 匹配点与 segment 长度均落在 char 边界（子串匹配起点即边界）。
        remainder = &remainder[index + segment.len()..]; // allow unsafe_text_op: find offset
    }
    remainder.ends_with(last) && remainder.len() >= last.len()
}

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;
