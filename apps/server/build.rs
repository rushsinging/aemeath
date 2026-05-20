fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure().build_server(true).compile_protos(
        &[
            "../../packages/proto/common.proto",
            "../../packages/proto/workspace.proto",
            "../../packages/proto/chat.proto",
            "../../packages/proto/board.proto",
            "../../packages/proto/agent.proto",
        ],
        &["../../packages/proto"],
    )?;
    Ok(())
}
