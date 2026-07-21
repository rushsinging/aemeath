use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn write(output: &Path) -> Result<()> {
    let rendered = render()?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建 SDK Wire Schema 目录 {} 失败", parent.display()))?;
    }
    fs::write(output, rendered)
        .with_context(|| format!("写入 SDK Wire Schema {} 失败", output.display()))
}

pub fn check(output: &Path) -> Result<()> {
    let expected = render()?;
    let actual = fs::read_to_string(output)
        .with_context(|| format!("读取 SDK Wire Schema {} 失败", output.display()))?;
    if actual != expected {
        anyhow::bail!(
            "SDK Wire Schema 已过期: 请运行 `cargo run -p xtask -- sdk-wire-schema write {}`",
            output.display()
        );
    }
    Ok(())
}

fn render() -> Result<String> {
    let document = sdk::wire::components_document();
    let rendered =
        serde_json::to_string_pretty(&document).context("序列化 SDK Wire Schema 失败")?;
    Ok(format!("{rendered}\n"))
}
