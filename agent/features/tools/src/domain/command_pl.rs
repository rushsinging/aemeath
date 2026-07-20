//! Slash Command Published Language（Issue #913）。
//!
//! Command 与 Tool/Skill 共享 BC，但不共享执行抽象。本模块只定义发现、
//! 参数解析和分类路由所需的纯值；目标 BC 的 Snapshot/Outcome 不进入这里。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CommandName(String);

impl CommandName {
    pub fn new(value: impl Into<String>) -> Result<Self, CommandParseError> {
        let value = value
            .into()
            .trim()
            .trim_start_matches('/')
            .to_ascii_lowercase();
        if value.is_empty()
            || !value
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            return Err(CommandParseError::InvalidName { name: value });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandMechanism {
    PromptInjection,
    SnapshotQuery,
    ApplicationControl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandTarget {
    Runtime,
    ContextManagement,
    Memory,
    Task,
    Project,
    Provider,
    Config,
    Audit,
    ApplicationVersionControl,
    ApplicationShell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapshotQueryTarget {
    Runtime,
    ContextManagement,
    Memory,
    Task,
    Project,
    Provider,
    Config,
    Audit,
    ApplicationShell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplicationControlTarget {
    Runtime,
    ContextManagement,
    Memory,
    Task,
    Project,
    Config,
    ApplicationVersionControl,
    ApplicationShell,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandArgumentSchema {
    None,
    OptionalText,
    RequiredText,
    OptionalPositiveUsize { default: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandDescriptor {
    pub name: CommandName,
    pub aliases: Vec<CommandName>,
    pub description: String,
    pub mechanism: CommandMechanism,
    pub target: CommandTarget,
    pub argument_schema: CommandArgumentSchema,
}

impl CommandDescriptor {
    pub fn new(
        name: &str,
        aliases: &[&str],
        description: &str,
        mechanism: CommandMechanism,
        target: CommandTarget,
        argument_schema: CommandArgumentSchema,
    ) -> Result<Self, CommandParseError> {
        Ok(Self {
            name: CommandName::new(name)?,
            aliases: aliases
                .iter()
                .map(|alias| CommandName::new(*alias))
                .collect::<Result<Vec<_>, _>>()?,
            description: description.to_string(),
            mechanism,
            target,
            argument_schema,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCompletion {
    pub replacement: String,
    pub display: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashInput(String);

impl SlashInput {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedArguments(Vec<String>);

impl ParsedArguments {
    pub fn new(values: Vec<String>) -> Self {
        Self(values)
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    pub fn join(&self) -> String {
        self.0.join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCommand {
    pub command: CommandName,
    pub arguments: ParsedArguments,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotQueryCommand {
    pub command: CommandName,
    pub arguments: ParsedArguments,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationControlCommand {
    pub command: CommandName,
    pub arguments: ParsedArguments,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandRoute {
    PromptInjection(PromptCommand),
    SnapshotQuery {
        target: SnapshotQueryTarget,
        command: SnapshotQueryCommand,
    },
    ApplicationControl {
        target: ApplicationControlTarget,
        command: ApplicationControlCommand,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandParseError {
    #[error("命令名无效：{name}")]
    InvalidName { name: String },
    #[error("未知命令：/{name}")]
    UnknownCommand { name: String },
    #[error("命令 /{command} 不接受参数")]
    UnexpectedArguments { command: String },
    #[error("命令 /{command} 缺少参数")]
    MissingArgument { command: String },
    #[error("命令 /{command} 的参数无效：{value}")]
    InvalidArgument { command: String, value: String },
    #[error("命令目录冲突：{name}")]
    DuplicateName { name: String },
    #[error("命令 /{command} 的目标与机制不匹配")]
    TargetMismatch { command: String },
}
