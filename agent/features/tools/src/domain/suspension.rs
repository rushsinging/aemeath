//! Typed, value-only tool suspension published language.

use serde::{Deserialize, Serialize};

/// A tool invocation that needs Runtime-owned user interaction before it can
/// produce a final result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolSuspension {
    UserInteraction(UserInteractionSpec),
}

/// Questions that Runtime must present for one suspended tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserInteractionSpec {
    pub questions: Vec<UserQuestion>,
}

impl UserInteractionSpec {
    pub fn new(questions: Vec<UserQuestion>) -> Self {
        Self { questions }
    }
}

/// An answer choice is a pure value and preserves the SDK's user-visible
/// title/description pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserOption {
    pub title: String,
    pub description: Option<String>,
}

impl UserOption {
    pub fn new(title: impl Into<String>, description: Option<String>) -> Self {
        Self {
            title: title.into(),
            description,
        }
    }

    pub fn title_only(title: impl Into<String>) -> Self {
        Self::new(title, None)
    }
}

/// A question is a pure value. Runtime supplies request/tool-call identity and
/// owns all waiting, reply, and cancellation state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserQuestion {
    pub prompt: String,
    pub options: Vec<UserOption>,
    pub allow_multi: bool,
    pub allow_free_input: bool,
    pub default: Option<String>,
}

impl UserQuestion {
    pub fn new(
        prompt: impl Into<String>,
        options: Vec<UserOption>,
        allow_multi: bool,
        allow_free_input: bool,
        default: Option<String>,
    ) -> Self {
        Self {
            prompt: prompt.into(),
            options,
            allow_multi,
            allow_free_input,
            default,
        }
    }
}

#[cfg(test)]
#[path = "suspension_tests.rs"]
mod tests;
