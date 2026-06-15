//! TUI 可展示视图：进度 / Hook / Workspace / 选项。

use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;

/// AskUserQuestion 选项项：简要 title + 详细 description。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Hash)]
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
    Message {
        text: String,
    },
    ToolCalls {
        calls: Vec<AgentToolCallProgressView>,
    },
}

impl std::fmt::Display for AgentProgressKindView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message { text } => write!(f, "{text}"),
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceStackEntryView {
    pub path_base: PathBuf,
    pub working_root: PathBuf,
}

/// TUI 可展示的 workspace 上下文视图。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceContextView {
    pub path_base: PathBuf,
    pub working_root: PathBuf,
    pub context_stack: Vec<WorkspaceStackEntryView>,
}

/// Hook 执行状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookEventStatus {
    Running,
    Succeeded,
    Blocked,
    Failed,
}

/// Hook 执行结果视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookExecutionResultView {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub decision: Option<String>,
    pub reason: Option<String>,
    pub additional_context: Option<String>,
}

/// Hook 事件视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookEventView {
    pub hook_name: String,
    pub status: HookEventStatus,
    pub matcher: Option<String>,
    pub command: Option<String>,
    pub result: Option<HookExecutionResultView>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_progress_view_supports_message_and_tool_calls() {
        let message = AgentProgressEventView {
            sequence: 1,
            kind: AgentProgressKindView::Message {
                text: "working".to_string(),
            },
        };
        let tools = AgentProgressEventView {
            sequence: 2,
            kind: AgentProgressKindView::ToolCalls {
                calls: vec![AgentToolCallProgressView {
                    id: crate::ids::ToolCallId::new_v7(),
                    name: "Read".to_string(),
                    input: serde_json::json!({"file_path":"a.rs"}),
                }],
            },
        };

        assert_eq!(message.sequence, 1);
        match message.kind {
            AgentProgressKindView::Message { text } => assert_eq!(text, "working"),
            other => panic!("unexpected kind: {other:?}"),
        }
        match tools.kind {
            AgentProgressKindView::ToolCalls { calls } => {
                assert_eq!(calls[0].name, "Read");
                // summary 已移除
            }
            other => panic!("unexpected kind: {other:?}"),
        }
    }

    #[test]
    fn test_agent_progress_display_tool_calls() {
        let event = AgentProgressEventView {
            sequence: 1,
            kind: AgentProgressKindView::ToolCalls {
                calls: vec![
                    AgentToolCallProgressView {
                        id: crate::ids::ToolCallId::new_v7(),
                        name: "Bash".to_string(),
                        input: serde_json::json!({"command": "ls"}),
                    },
                    AgentToolCallProgressView {
                        id: crate::ids::ToolCallId::new_v7(),
                        name: "Read".to_string(),
                        input: serde_json::json!({"file_path": "TODO.md"}),
                    },
                ],
            },
        };
        assert_eq!(format!("{event}"), "Bash, Read");
    }

    #[test]
    fn test_agent_progress_display_message() {
        let event = AgentProgressEventView {
            sequence: 2,
            kind: AgentProgressKindView::Message {
                text: "分析完成".to_string(),
            },
        };
        assert_eq!(format!("{event}"), "分析完成");
    }

    #[test]
    fn test_workspace_context_view_keeps_paths() {
        let view = WorkspaceContextView {
            path_base: "/repo/sub".into(),
            working_root: "/repo".into(),
            context_stack: vec![WorkspaceStackEntryView {
                path_base: "/repo".into(),
                working_root: "/repo".into(),
            }],
        };

        assert_eq!(view.path_base.to_string_lossy(), "/repo/sub");
        assert_eq!(view.working_root.to_string_lossy(), "/repo");
        assert_eq!(view.context_stack.len(), 1);
    }

    #[test]
    fn test_option_item_title_only() {
        let item = OptionItem::title_only("Yes".to_string());
        assert_eq!(item.title, "Yes");
        assert!(item.description.is_none());
    }

    #[test]
    fn test_option_item_with_description() {
        let item = OptionItem::new("Deploy", "Push to production");
        assert_eq!(item.title, "Deploy");
        assert_eq!(item.description.as_deref(), Some("Push to production"));
    }

    #[test]
    fn test_option_item_serialize_deserialize_string_compat() {
        // 向后兼容：纯字符串应反序列化为 title_only
        let json = serde_json::json!("Simple option");
        let item: OptionItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.title, "Simple option");
        assert!(item.description.is_none());
    }

    #[test]
    fn test_option_item_serialize_deserialize_object() {
        let json = serde_json::json!({"title": "Go", "description": "Proceed"});
        let item: OptionItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.title, "Go");
        assert_eq!(item.description, Some("Proceed".to_string()));
    }

    #[test]
    fn test_option_item_serialize_outputs_object() {
        let item = OptionItem::new("Test", "Desc");
        let val = serde_json::to_value(&item).unwrap();
        assert_eq!(val["title"], "Test");
        assert_eq!(val["description"], "Desc");
    }
}
