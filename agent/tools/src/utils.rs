/// utils/mod.rs — 工具（跑腿）：路径安全校验等纯辅助逻辑
// path_security 保留 *_from_base 之外的便捷包装（validate_and_normalize_path 等），
// 当前仅 *_from_base 变体被各 Tool 调用，包装函数保留备用（refs #61 D3）。
#[allow(dead_code)]
pub mod path_security;
