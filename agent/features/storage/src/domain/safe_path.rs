use std::fmt;
use std::str::FromStr;

use super::{StorageError, StorageErrorKind};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SafePathSegment(String);

impl SafePathSegment {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for SafePathSegment {
    type Err = StorageError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = value.is_empty()
            || value == "."
            || value == ".."
            || value.starts_with('.')
            || value.contains('/')
            || value.contains('\\')
            || value.contains('\0');
        if invalid {
            return Err(StorageError::new(
                StorageErrorKind::InvalidKey,
                "路径段不安全",
            ));
        }
        Ok(Self(value.to_owned()))
    }
}

impl fmt::Display for SafePathSegment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
