use sdk::ids::{RunId, RunStepId};
use std::path::PathBuf;
use tools::{AuthorizationContext, ToolCapabilities, ToolCapability, ToolName};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PolicyMode {
    #[default]
    Standard,
    AllowAll,
}

impl From<share::config::PermissionModeConfig> for PolicyMode {
    fn from(value: share::config::PermissionModeConfig) -> Self {
        match value {
            share::config::PermissionModeConfig::AllowAll => Self::AllowAll,
            share::config::PermissionModeConfig::Ask
            | share::config::PermissionModeConfig::AutoRead => Self::Standard,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PolicyRequest {
    run_id: RunId,
    run_step_id: RunStepId,
    tool_name: ToolName,
    required_capabilities: ToolCapabilities,
    workspace_root: PathBuf,
}

impl PolicyRequest {
    pub fn new(
        run_id: RunId,
        run_step_id: RunStepId,
        tool_name: ToolName,
        required_capabilities: ToolCapabilities,
        workspace_root: impl Into<PathBuf>,
    ) -> Result<Self, PolicyRequestError> {
        let workspace_root = workspace_root.into();
        if workspace_root.as_os_str().is_empty() {
            return Err(PolicyRequestError::EmptyWorkspaceRoot);
        }
        Ok(Self {
            run_id,
            run_step_id,
            tool_name,
            required_capabilities,
            workspace_root,
        })
    }

    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }
    pub fn run_step_id(&self) -> &RunStepId {
        &self.run_step_id
    }
    pub fn tool_name(&self) -> &ToolName {
        &self.tool_name
    }
    pub fn required_capabilities(&self) -> ToolCapabilities {
        self.required_capabilities
    }
    pub fn workspace_root(&self) -> &std::path::Path {
        &self.workspace_root
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyRequestError {
    EmptyWorkspaceRoot,
}

impl std::fmt::Display for PolicyRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyWorkspaceRoot => write!(f, "Policy 请求的工作区根不能为空"),
        }
    }
}
impl std::error::Error for PolicyRequestError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyReason {
    CapabilityExceeded { required: ToolCapability },
    RestrictedTool,
    RestrictedWorkspace,
    Custom(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalSubject {
    UserInteraction,
    Delegated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow(AuthorizationContext),
    Deny {
        reason: PolicyReason,
    },
    RequireApproval {
        reason: PolicyReason,
        subject: ApprovalSubject,
    },
}

pub trait PolicyModeSource: Send + Sync {
    fn current_mode(&self) -> PolicyMode;
}

pub trait PolicyPort: Send + Sync {
    fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision;
}
