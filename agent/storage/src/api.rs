//! storage crate 的 Public API 门面（DDD §6.4.3）。
//!
//! 对外仅经此模块暴露 use case 实际消费的持久化能力，
//! 内部 memory / history / tool_result_storage 模块保持 crate-private。

pub use crate::business::memory::{
    memory_base_dir, project_hash, project_hash_from_path, MemoryStore,
};
pub use crate::business::tool_result_storage::persist_oversized_results;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageApiMarker;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_is_copy() {
        let marker = StorageApiMarker;
        assert_eq!(marker, marker);
    }
}
