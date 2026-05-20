use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("packages/sdk/openapi/aemeath.json"));
    let json = server::rest::export_openapi_json()?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, json)?;
    println!("exported OpenAPI to {}", output.display());
    Ok(())
}
