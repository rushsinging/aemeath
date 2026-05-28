#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ChatId(String);

impl ChatId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl AsRef<str> for ChatId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ChatTurnId(String);

impl ChatTurnId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl AsRef<str> for ChatTurnId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ToolCallId(String);

impl ToolCallId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl AsRef<str> for ToolCallId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

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
