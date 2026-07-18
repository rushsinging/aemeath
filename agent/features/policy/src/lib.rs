/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:policy";

mod domain;

pub use domain::{
    validate_and_normalize_path, validate_and_normalize_path_from_base, validate_search_path,
    validate_search_path_from_base,
};
