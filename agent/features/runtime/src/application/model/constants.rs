//! model 层测试常量（`#[cfg(test)] mod constants` 门控，生产不编译）。

/// 重试推进的虚拟时间上限序列（测试专用）。
pub(crate) const RETRY_ADVANCE_LIMITS: [std::time::Duration; 10] = [
    std::time::Duration::from_secs(11),
    std::time::Duration::from_secs(21),
    std::time::Duration::from_secs(41),
    std::time::Duration::from_secs(81),
    std::time::Duration::from_secs(121),
    std::time::Duration::from_secs(121),
    std::time::Duration::from_secs(121),
    std::time::Duration::from_secs(121),
    std::time::Duration::from_secs(121),
    std::time::Duration::from_secs(121),
];
