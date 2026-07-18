use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

pub struct TimedStep {
    pub name: &'static str,
    pub duration: Duration,
}

pub fn run(root: &Path) -> Result<Vec<TimedStep>> {
    let steps = [
        ("cargo check --workspace", vec!["check", "--workspace"]),
        (
            "cargo clippy production targets",
            vec![
                "clippy",
                "--workspace",
                "--bins",
                "--lib",
                "--",
                "-D",
                "warnings",
            ],
        ),
    ];
    let mut timings = Vec::new();
    for (name, args) in steps {
        let started = Instant::now();
        let status = Command::new("cargo")
            .args(args)
            .current_dir(root)
            .status()
            .with_context(|| format!("执行 {name} 失败"))?;
        if !status.success() {
            anyhow::bail!("{name} 未通过");
        }
        timings.push(TimedStep {
            name,
            duration: started.elapsed(),
        });
    }
    Ok(timings)
}
