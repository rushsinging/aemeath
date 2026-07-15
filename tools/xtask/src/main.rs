use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("coverage-summary") => {
            let report = PathBuf::from(args.next().context("缺少 coverage report 路径")?);
            let root = PathBuf::from(args.next().context("缺少 workspace root 路径")?);
            let report_json = fs::read_to_string(&report)
                .with_context(|| format!("读取 {} 失败", report.display()))?;
            let output = Command::new("cargo")
                .args(["metadata", "--no-deps", "--format-version", "1"])
                .current_dir(&root)
                .output()
                .context("执行 cargo metadata 失败")?;
            if !output.status.success() {
                anyhow::bail!(
                    "cargo metadata 失败: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            let rendered = xtask::coverage::render_summary(
                &report_json,
                &String::from_utf8(output.stdout).context("cargo metadata 不是 UTF-8")?,
                &root,
            )?;
            print!("{rendered}");
            Ok(())
        }
        Some("production-reachability") => {
            let root = PathBuf::from(args.next().unwrap_or_else(|| ".".to_owned()));
            for step in xtask::reachability::run(&root)? {
                println!("{}: {:.2?}", step.name, step.duration);
            }
            Ok(())
        }
        Some("source-guard") => {
            let root = PathBuf::from(args.next().unwrap_or_else(|| ".".to_owned()));
            let output = args.next().map(PathBuf::from);
            let started = std::time::Instant::now();
            let report = xtask::workspace_guard::enforce(&root, output.as_deref())?;
            println!(
                "source guard: {:?}; allow(dead_code)={}; public items={}",
                started.elapsed(),
                report.dead_code_allow_count,
                report.public_surface.len()
            );
            Ok(())
        }
        _ => anyhow::bail!(
            "用法: cargo run -p xtask -- <coverage-summary <report.json> <root>|production-reachability [root]|source-guard [root] [public-surface-output]>"
        ),
    }
}
