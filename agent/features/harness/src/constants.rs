//! Harness 模块常量。

/// 本 crate 日志 target（路由规则见 `specs/3.15-logging.md`；
/// feature crate 前缀约定 `aemeath:agent:<crate>`）。
pub(crate) const LOG_TARGET: &str = "aemeath:agent:harness";

/// 边界错误来源标识（`share::error::DomainError` 的 domain 字段）。
pub const ERROR_DOMAIN: &str = "harness";

/// 控制帧长度前缀（u32 大端）上限：控制帧远小于此，超限视为协议破坏。
pub const CONTROL_FRAME_MAX_BYTES: usize = 64 * 1024;

/// 经验条目单条 lesson 文本上限（对齐 memory content 上限族）。
pub const LESSON_MAX_CHARS: usize = 500;

/// 经验条目证据摘录上限。
pub const EXCERPT_MAX_CHARS: usize = 200;
