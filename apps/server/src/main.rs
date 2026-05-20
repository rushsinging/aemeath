use server::config::ServerConfig;
use server::grpc::agent::AgentRegistryGrpc;
use server::grpc::board::BoardGrpc;
use server::grpc::chat::ChatGrpc;
use server::model::app::AppState;
use server::proto::aemeath::v1::agent_registry_service_server::AgentRegistryServiceServer;
use server::proto::aemeath::v1::board_service_server::BoardServiceServer;
use server::proto::aemeath::v1::chat_service_server::ChatServiceServer;
use std::net::SocketAddr;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::load()?;
    let state = AppState::default();
    let http_addr: SocketAddr = config.http_addr.parse()?;
    let grpc_addr: SocketAddr = config.grpc_addr.parse()?;

    let rest_state = state.clone();
    let grpc_state = state.clone();

    let rest = async {
        axum::serve(
            tokio::net::TcpListener::bind(http_addr).await?,
            server::rest::router(rest_state),
        )
        .await
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    };

    let grpc = async {
        Server::builder()
            .add_service(AgentRegistryServiceServer::new(AgentRegistryGrpc))
            .add_service(ChatServiceServer::new(ChatGrpc::new(grpc_state.clone())))
            .add_service(BoardServiceServer::new(BoardGrpc::new(grpc_state)))
            .serve(grpc_addr)
            .await
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    };

    tokio::try_join!(rest, grpc)?;
    Ok(())
}
