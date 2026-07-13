use crate::business::agent::ToolCall;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInvocation {
    request: String,
    response: String,
}

impl ModelInvocation {
    pub fn new(request: impl Into<String>, response: impl Into<String>) -> Self {
        Self {
            request: request.into(),
            response: response.into(),
        }
    }

    pub fn request(&self) -> &str {
        &self.request
    }

    pub fn response(&self) -> &str {
        &self.response
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallStatus {
    Pending,
    Ready,
    AwaitingApproval,
    Running,
    Success,
    Error,
    Cancelled,
}

impl ToolCallStatus {
    pub(super) fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::Ready | Self::Cancelled)
                | (
                    Self::Ready,
                    Self::AwaitingApproval | Self::Running | Self::Cancelled
                )
                | (Self::AwaitingApproval, Self::Running | Self::Cancelled)
                | (Self::Running, Self::Success | Self::Error | Self::Cancelled)
        )
    }
}

#[derive(Clone)]
pub struct RunToolCall {
    call: ToolCall,
    status: ToolCallStatus,
}

impl std::fmt::Debug for RunToolCall {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RunToolCall")
            .field("id", &self.call.id)
            .field("provider_id", &self.call.provider_id)
            .field("name", &self.call.name)
            .field("status", &self.status)
            .finish()
    }
}

impl RunToolCall {
    pub(super) fn new(call: ToolCall) -> Self {
        Self {
            call,
            status: ToolCallStatus::Pending,
        }
    }

    pub fn id(&self) -> &sdk::ids::ToolCallId {
        &self.call.id
    }

    pub fn call(&self) -> &ToolCall {
        &self.call
    }

    pub fn status(&self) -> ToolCallStatus {
        self.status
    }

    pub(super) fn advance(&mut self, next: ToolCallStatus) -> bool {
        if !self.status.can_transition_to(next) {
            return false;
        }
        self.status = next;
        true
    }
}
