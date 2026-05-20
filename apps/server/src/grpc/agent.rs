use crate::proto::aemeath::v1::agent_registry_service_server::AgentRegistryService;
use crate::proto::aemeath::v1::{
    DeregisterAgentRequest, Empty, HeartbeatRequest, HeartbeatResponse, RegisterAgentRequest,
    RegisterAgentResponse,
};
use tonic::{Request, Response, Status};

#[derive(Debug, Default)]
pub struct AgentRegistryGrpc;

#[tonic::async_trait]
impl AgentRegistryService for AgentRegistryGrpc {
    async fn register(
        &self,
        _request: Request<RegisterAgentRequest>,
    ) -> Result<Response<RegisterAgentResponse>, Status> {
        Err(Status::unimplemented(
            "AgentRegistryService.Register 尚未实现",
        ))
    }

    async fn heartbeat(
        &self,
        _request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        Err(Status::unimplemented(
            "AgentRegistryService.Heartbeat 尚未实现",
        ))
    }

    async fn deregister(
        &self,
        _request: Request<DeregisterAgentRequest>,
    ) -> Result<Response<Empty>, Status> {
        Err(Status::unimplemented(
            "AgentRegistryService.Deregister 尚未实现",
        ))
    }
}
