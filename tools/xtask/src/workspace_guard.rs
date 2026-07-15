use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct DeadCodeBaseline {
    max_production_allow_dead_code: usize,
}

pub struct SourceReport {
    pub test_only_violations: Vec<String>,
    pub dead_code_allow_count: usize,
    pub public_surface: Vec<String>,
}

pub fn scan_workspace(root: &Path) -> Result<SourceReport> {
    let mut rust_files = Vec::new();
    for directory in ["agent", "apps", "packages"] {
        collect_rust_files(&root.join(directory), &mut rust_files)?;
    }
    rust_files.sort();
    let mut report = SourceReport {
        test_only_violations: Vec::new(),
        dead_code_allow_count: 0,
        public_surface: Vec::new(),
    };
    for file in rust_files {
        let source =
            fs::read_to_string(&file).with_context(|| format!("读取 {} 失败", file.display()))?;
        let relative = file.strip_prefix(root).unwrap_or(&file);
        report
            .test_only_violations
            .extend(crate::source_guard::find_test_only_api_violations(
                relative, &source,
            ));
        report.dead_code_allow_count +=
            crate::source_guard::production_dead_code_allow_count(&source);
        report
            .public_surface
            .extend(crate::source_guard::public_surface(relative, &source));
    }
    report.public_surface.sort();
    Ok(report)
}

pub fn enforce(root: &Path, output: Option<&Path>) -> Result<SourceReport> {
    let report = scan_workspace(root)?;
    if !report.test_only_violations.is_empty() {
        anyhow::bail!(
            "发现生产 test-only API:\n{}",
            report.test_only_violations.join("\n")
        );
    }
    let baseline: DeadCodeBaseline = serde_json::from_str(&fs::read_to_string(
        root.join(".agents/dead-code-baseline.json"),
    )?)?;
    if report.dead_code_allow_count > baseline.max_production_allow_dead_code {
        anyhow::bail!(
            "生产 allow(dead_code) 数量 {} 超过 baseline {}",
            report.dead_code_allow_count,
            baseline.max_production_allow_dead_code
        );
    }
    if let Some(output) = output {
        fs::write(output, report.public_surface.join("\n") + "\n")?;
    }
    Ok(report)
}

fn collect_rust_files(directory: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            collect_rust_files(&path, output)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
    Ok(())
}
