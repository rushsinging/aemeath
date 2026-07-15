use share::memory::entry::MemoryEntry;
use share::memory::error::{MemoryError, MemoryResult};

pub(super) fn validate_entry(entry: &MemoryEntry) -> MemoryResult<()> {
    if entry.content.trim().is_empty() {
        return Err(MemoryError::invalid_input("记忆内容不能为空"));
    }
    if entry.content.chars().count() > 500 {
        return Err(MemoryError::invalid_input("记忆内容不能超过 500 字符"));
    }
    Ok(())
}
