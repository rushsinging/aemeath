//! Hook adapters 层常量（#1146 双轨归位）。

use std::time::Duration;

pub(crate) const BASIC_ENVIRONMENT_VARIABLES: [&str; 6] =
    ["PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TERM"];

pub(crate) const CONTEXT_SEPARATOR: &str = "\n";

pub(crate) const DEFAULT_OUTPUT_LIMIT: usize = 8 * 1024;
pub(crate) const TERMINATION_GRACE: Duration = Duration::from_millis(250);
/// `AEMEATH_*` 前缀的按次权威变量命名空间：仅由 Dispatcher 注入，
/// 语义上与 `CLAUDE_*` 前缀兼容层共享归属。
pub(crate) const RESERVED_PREFIX: &str = "AEMEATH_";
