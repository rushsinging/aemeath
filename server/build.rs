fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure().build_server(true).compile_protos(
        &[
            "../share/proto/common.proto",
            "../share/proto/workspace.proto",
            "../share/proto/agent.proto",
        ],
        &["../share/proto"],
    )?;
    Ok(())
}
