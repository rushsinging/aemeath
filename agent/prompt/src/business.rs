/// business/mod.rs — 业务规则（规则专家）：guidance 解析 / skill 加载 / 内容安全扫描
// guidance / skill 内含若干面向完整性的加载/解析辅助（cached/filter 变体、
// 单文件 loader、命名文件异步加载等），收窄可见性后内部仅部分经 prompt::api
// 暴露消费，其余 re-export / 实现保留备用（refs #61 D3）。
#[allow(dead_code, unused_imports)]
pub mod guidance;
pub mod security;
#[allow(dead_code, unused_imports)]
pub mod skill;
