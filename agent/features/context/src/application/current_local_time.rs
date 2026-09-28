//! 请求级「当前本地时间」system block。
//!
//! 每次 `build_window` 在 uncached suffix 末尾追加一块，让模型在每步请求
//! 都知道当前日期与时间。放在 cache breakpoint 之后：该块随请求更新，
//! 进入 cacheable prefix 会导致缓存整段失效。

#[cfg(test)]
use chrono::TimeZone;
use chrono::{DateTime, FixedOffset};

use crate::domain::SystemBlock;

const CURRENT_LOCAL_TIME_LABEL_EN: &str = "Current local time";
const CURRENT_LOCAL_TIME_LABEL_ZH: &str = "当前本地时间";

/// 按 `Config.language` 选择时间块标签（specs/3.7 §44-47：常量 + 选择函数，
/// 禁止调用点内联 match）。
fn current_local_time_label(language: &str) -> &'static str {
    match language {
        "zh" => CURRENT_LOCAL_TIME_LABEL_ZH,
        _ => CURRENT_LOCAL_TIME_LABEL_EN,
    }
}

/// 构造请求级「当前本地时间」block（uncached、无 cache marker）。
pub(crate) fn current_local_time_block(language: &str, now: DateTime<FixedOffset>) -> SystemBlock {
    SystemBlock {
        kind: "current_local_time".into(),
        content: format!(
            "{}: {}",
            current_local_time_label(language),
            now.format("%Y-%m-%d %H:%M:%S %z")
        ),
        cacheable: false,
        cache_break: false,
    }
}

/// 生产时间源：系统本地时钟（固定 offset 表示，避免时区转换开销差异）。
pub(crate) fn local_now() -> DateTime<FixedOffset> {
    chrono::Local::now().fixed_offset()
}

/// 测试专用固定时间源（specs/3.2.5.4：时间 MUST 可注入或固定）。
#[cfg(test)]
pub(crate) fn fixed_local_test_now() -> DateTime<FixedOffset> {
    FixedOffset::east_opt(8 * 3600)
        .expect("UTC+8 offset 必须存在")
        .with_ymd_and_hms(2026, 6, 15, 14, 30, 5)
        .single()
        .expect("固定测试时刻必须唯一")
}

#[cfg(test)]
#[path = "current_local_time_tests.rs"]
mod tests;
