//! TUI-owned identities for conversation and tool projections.
//!
//! Runtime identities cross the ACL as strings and are reconstructed here as
//! local values. TUI never exposes or stores SDK ID newtypes.

macro_rules! tui_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, PartialEq)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl AsRef<str>) -> Self {
                Self(value.as_ref().to_string())
            }

            pub fn new_v7() -> Self {
                Self(uuid::Uuid::new_v4().to_string())
            }

            pub fn from_legacy_or_new(value: &str) -> Self {
                Self::new(value)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

tui_id!(ChatId);
tui_id!(ChatTurnId);
tui_id!(ToolCallId);

/// Tool stream key for identifying tool call streams.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ToolStreamKey {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
    pub name: String,
    pub index: usize,
}

impl ToolStreamKey {
    pub fn new(
        chat_id: ChatId,
        turn_id: ChatTurnId,
        name: impl Into<String>,
        index: usize,
    ) -> Self {
        Self {
            chat_id,
            turn_id,
            name: name.into(),
            index,
        }
    }
}

#[cfg(test)]
#[path = "ids_tests.rs"]
mod tests;
