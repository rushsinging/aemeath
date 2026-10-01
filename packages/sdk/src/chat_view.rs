//! TUI 可展示视图：进度 / Hook / Workspace / 选项。

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;

/// AskUserQuestion 选项项：简要 title + 详细 description。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Hash, JsonSchema)]
pub struct OptionItem {
    /// 简要标题（必填）。
    pub title: String,
    /// 详细描述（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl<'de> Deserialize<'de> for OptionItem {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        use serde::de;

        #[derive(Deserialize)]
        struct Obj {
            title: String,
            #[serde(default)]
            description: Option<String>,
        }

        // 先尝试按对象反序列化
        let value = serde_json::Value::deserialize(de)?;
        if value.is_string() {
            Ok(OptionItem::title_only(value.as_str().unwrap().to_string()))
        } else if value.is_object() {
            let obj: Obj =
                serde_json::from_value(value).map_err(|e| de::Error::custom(e.to_string()))?;
            Ok(OptionItem {
                title: obj.title,
                description: obj.description,
            })
        } else {
            Err(de::Error::custom(
                "expected string or object { title, description }",
            ))
        }
    }
}

impl OptionItem {
    pub fn title_only(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: None,
        }
    }

    pub fn new(title: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: Some(description.into()),
        }
    }
}

/// Sub Run identity published with every structured activity event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SubRunIdentityView {
    pub agent_id: crate::AgentId,
    pub run_id: crate::RunId,
    pub parent_chat_id: crate::ChatId,
    pub parent_run_id: crate::RunId,
    pub spawned_by_tool_call_id: crate::ToolCallId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SubRunActivityKindView {
    Text {
        text: String,
    },
    Thinking {
        text: String,
    },
    ToolCall {
        id: crate::ToolCallId,
        name: String,
        input: serde_json::Value,
    },
    ToolOutput {
        tool_name: String,
        text: String,
    },
    ToolResult {
        tool_call_id: crate::ToolCallId,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
    },
    Terminal {
        outcome: SubRunTerminalOutcomeView,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SubRunTerminalOutcomeView {
    Completed,
    Failed { error: String },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SubRunStartedEventView {
    pub identity: SubRunIdentityView,
    pub sequence: u64,
    pub role: Option<String>,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubRunActivityEventView {
    pub identity: SubRunIdentityView,
    pub sequence: u64,
    pub kind: SubRunActivityKindView,
}

/// Sub-agent 工具调用进度。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentToolCallProgressView {
    pub id: crate::ids::ToolCallId,
    pub name: String,
    pub input: serde_json::Value,
}

impl std::fmt::Display for AgentToolCallProgressView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// Sub-agent 进度类型。
#[derive(Debug, Clone, PartialEq)]
pub enum AgentProgressKindView {
    /// Sub-agent 启动，携带 role / model 元数据。
    Started {
        role: Option<String>,
        model: String,
    },
    Message {
        text: String,
    },
    ToolCalls {
        calls: Vec<AgentToolCallProgressView>,
    },
    /// Output streamed by a tool running inside a sub-agent.
    ///
    /// This is distinct from human-readable sub-agent progress and should not
    /// be rendered as a normal activity line by default.
    ToolOutput {
        tool_name: String,
        text: String,
    },
}

/// 工具 stdout 流式输出事件 view（与 [`AgentProgressEventView`] 平级但语义独立）。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolProgressEventView {
    /// stdout 文本片段。
    pub text: String,
}

impl std::fmt::Display for AgentProgressKindView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Started { role, model } => match role {
                Some(r) => write!(f, "{r} · {model}"),
                None => write!(f, "{model}"),
            },
            Self::Message { text } => write!(f, "{text}"),
            Self::ToolOutput { tool_name, text } => write!(f, "{tool_name}: {text}"),
            Self::ToolCalls { calls } => {
                for (i, call) in calls.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{call}")?;
                }
                Ok(())
            }
        }
    }
}

/// Sub-agent 进度事件。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentProgressEventView {
    pub sequence: usize,
    pub kind: AgentProgressKindView,
}

impl std::fmt::Display for AgentProgressEventView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind)
    }
}

/// workspace 栈条目视图。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceStackEntryView {
    pub path_base: PathBuf,
    #[serde(alias = "working_root")]
    pub workspace_root: PathBuf,
}

/// TUI 可展示的 workspace 上下文视图。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceContextView {
    pub path_base: PathBuf,
    #[serde(alias = "working_root")]
    pub workspace_root: PathBuf,
    pub context_stack: Vec<WorkspaceStackEntryView>,
}

#[cfg(test)]
#[path = "chat_view_tests.rs"]
mod tests;
