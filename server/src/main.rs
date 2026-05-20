use server::config::ServerConfig;
use server::grpc::agent::AgentRegistryGrpc;
use server::proto::aemeath::v1::agent_registry_service_server::AgentRegistryServiceServer;
use std::net::SocketAddr;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::load()?;
    let http_addr: SocketAddr = config.http_addr.parse()?;
    let grpc_addr: SocketAddr = config.grpc_addr.parse()?;

    let rest = async {
        axum::serve(
            tokio::net::TcpListener::bind(http_addr).await?,
            server::rest::router(),
        )
        .await
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    };

    let grpc = async {
        Server::builder()
            .add_service(AgentRegistryServiceServer::new(AgentRegistryGrpc))
            .serve(grpc_addr)
            .await
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    };

    tokio::try_join!(rest, grpc)?;
    Ok(())
}
