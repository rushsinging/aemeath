//! 状态容器（#1146 placement 归位）。

/// guidance resolver 测试的环境锁（serial 化 env 读写）。
#[cfg(test)]
pub(crate) static GUIDANCE_ENV_LOCK: std::sync::Mutex<()> = const { std::sync::Mutex::new(()) };
