use super::*;
use mongodb::bson::oid::ObjectId;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn contains_any(content: &str, keywords: &[&str]) -> bool {
    let lowered = content.to_lowercase();
    keywords.iter().any(|keyword| lowered.contains(keyword))
}

pub(super) fn default_if_empty(value: String, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

pub(super) fn new_id() -> String {
    ObjectId::new().to_hex()
}

pub(super) fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as i64
}

pub(super) fn require_non_empty(field: &'static str, value: &str) -> Result<(), StoreError> {
    if value.trim().is_empty() {
        Err(StoreError::InvalidInput { field })
    } else {
        Ok(())
    }
}
