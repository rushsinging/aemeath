use std::fmt;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SafePathSegment(String);

impl SafePathSegment {
    pub fn new(value: impl Into<String>) -> Result<Self, StorageKeyError> {
        let value = value.into();
        validate_segment(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageNamespace {
    Sessions,
    Memory,
    Tasks,
    History,
    ToolResults,
    Audit,
}

impl StorageNamespace {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sessions => "sessions",
            Self::Memory => "memory",
            Self::Tasks => "tasks",
            Self::History => "history",
            Self::ToolResults => "tool-results",
            Self::Audit => "audit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StorageKey {
    namespace: StorageNamespace,
    segments: Vec<SafePathSegment>,
}

impl StorageKey {
    pub fn new(
        namespace: StorageNamespace,
        segments: impl IntoIterator<Item = SafePathSegment>,
    ) -> Result<Self, StorageKeyError> {
        let segments = segments.into_iter().collect::<Vec<_>>();
        if segments.is_empty() {
            return Err(StorageKeyError::new("存储键至少需要一个路径段"));
        }
        Ok(Self {
            namespace,
            segments,
        })
    }

    pub const fn namespace(&self) -> StorageNamespace {
        self.namespace
    }

    pub fn segments(&self) -> &[SafePathSegment] {
        &self.segments
    }

    pub fn child(&self, segment: SafePathSegment) -> Self {
        let mut segments = self.segments.clone();
        segments.push(segment);
        Self {
            namespace: self.namespace,
            segments,
        }
    }
}

impl fmt::Display for StorageKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.namespace.as_str())?;
        for segment in &self.segments {
            write!(formatter, "/{}", segment.as_str())?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("存储键无效：{reason}")]
pub struct StorageKeyError {
    reason: &'static str,
}

impl StorageKeyError {
    const fn new(reason: &'static str) -> Self {
        Self { reason }
    }

    pub const fn reason(&self) -> &'static str {
        self.reason
    }
}

fn validate_segment(value: &str) -> Result<(), StorageKeyError> {
    if value.is_empty() {
        return Err(StorageKeyError::new("路径段不能为空"));
    }
    if matches!(value, "." | "..") {
        return Err(StorageKeyError::new("路径段不能是点目录"));
    }
    if value.contains('\0') {
        return Err(StorageKeyError::new("路径段不能包含 NUL"));
    }
    if value.contains('/') || value.contains('\\') {
        return Err(StorageKeyError::new("路径段不能包含路径分隔符"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{SafePathSegment, StorageKey, StorageNamespace};

    #[test]
    fn test_safe_path_segment_validation_accepts_safe_names() {
        for value in ["session.json", "用户", "a-b_c.1"] {
            let segment = SafePathSegment::new(value).expect("安全路径段应构造成功");
            assert_eq!(segment.as_str(), value);
        }
    }

    #[test]
    fn test_safe_path_segment_validation_rejects_unsafe_names() {
        for value in ["", ".", "..", "a/b", "a\\b", "/tmp", "a\0b"] {
            assert!(SafePathSegment::new(value).is_err(), "应拒绝 {value:?}");
        }
    }

    #[test]
    fn test_safe_path_segment_validation_error_has_stable_reason() {
        let error = SafePathSegment::new("..").expect_err("父目录必须被拒绝");
        assert_eq!(error.reason(), "路径段不能是点目录");
        assert!(!error.to_string().contains("/Users/"));
    }

    #[test]
    fn test_storage_key_invariants_namespace_names_are_stable() {
        let cases = [
            (StorageNamespace::Sessions, "sessions"),
            (StorageNamespace::Memory, "memory"),
            (StorageNamespace::Tasks, "tasks"),
            (StorageNamespace::History, "history"),
            (StorageNamespace::ToolResults, "tool-results"),
            (StorageNamespace::Audit, "audit"),
        ];
        for (namespace, expected) in cases {
            assert_eq!(namespace.as_str(), expected);
        }
    }

    #[test]
    fn test_storage_key_invariants_rejects_empty_segments() {
        assert!(StorageKey::new(StorageNamespace::Sessions, []).is_err());
    }

    #[test]
    fn test_storage_key_invariants_child_uses_validated_segment() {
        let key = StorageKey::new(
            StorageNamespace::Sessions,
            [SafePathSegment::new("project").unwrap()],
        )
        .unwrap();
        let child = key.child(SafePathSegment::new("session.json").unwrap());
        assert_eq!(child.segments().len(), 2);
        assert_eq!(child.segments()[1].as_str(), "session.json");
    }

    #[test]
    fn test_storage_key_invariants_display_is_logical_only() {
        let key = StorageKey::new(
            StorageNamespace::Memory,
            [SafePathSegment::new("project.json").unwrap()],
        )
        .unwrap();
        assert_eq!(key.to_string(), "memory/project.json");
        assert!(!format!("{key:?}").contains("/Users/"));
    }
}
