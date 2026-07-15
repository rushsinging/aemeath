use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const METRICS: [&str; 3] = ["regions", "functions", "lines"];

#[derive(Debug, Deserialize)]
struct CoverageReport {
    data: Vec<CoverageData>,
}
#[derive(Debug, Deserialize)]
struct CoverageData {
    #[serde(default)]
    files: Vec<CoverageFile>,
}
#[derive(Debug, Deserialize)]
struct CoverageFile {
    filename: PathBuf,
    summary: BTreeMap<String, Metric>,
}
#[derive(Debug, Clone, Copy, Deserialize, Default)]
struct Metric {
    count: u64,
    covered: u64,
}
#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
}
#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: String,
    manifest_path: PathBuf,
}

pub fn render_summary(report: &str, metadata: &str, root: &Path) -> Result<String> {
    let report: CoverageReport = serde_json::from_str(report).context("解析覆盖率 JSON 失败")?;
    let metadata: CargoMetadata =
        serde_json::from_str(metadata).context("解析 cargo metadata 失败")?;
    let mut packages = metadata
        .packages
        .into_iter()
        .map(|package| {
            let manifest = absolutize(root, &package.manifest_path);
            let directory = manifest
                .parent()
                .context("Cargo manifest 缺少父目录")?
                .to_path_buf();
            Ok((package.name, directory))
        })
        .collect::<Result<Vec<_>>>()?;
    packages.sort_by(|left, right| left.0.cmp(&right.0));
    let mut per_package: BTreeMap<String, BTreeMap<String, Metric>> = packages
        .iter()
        .map(|(name, _)| (name.clone(), empty_metrics()))
        .collect();

    for data in report.data {
        for file in data.files {
            for metric in METRICS {
                if !file.summary.contains_key(metric) {
                    anyhow::bail!("覆盖率文件 {} 缺少 {metric} 指标", file.filename.display());
                }
            }
            let filename = absolutize(root, &file.filename);
            let owner = packages
                .iter()
                .filter(|(_, dir)| filename.starts_with(dir))
                .max_by_key(|(_, dir)| dir.components().count())
                .map(|(name, _)| name);
            let Some(owner) = owner else { continue };
            let totals = per_package
                .get_mut(owner)
                .context("覆盖率 package 映射缺失")?;
            for metric in METRICS {
                let source = file.summary[metric];
                let target = totals.get_mut(metric).context("覆盖率指标映射缺失")?;
                target.count += source.count;
                target.covered += source.covered;
            }
        }
    }

    let mut workspace = empty_metrics();
    for values in per_package.values() {
        for metric in METRICS {
            workspace.get_mut(metric).expect("known metric").count += values[metric].count;
            workspace.get_mut(metric).expect("known metric").covered += values[metric].covered;
        }
    }

    let mut output = String::from("\nAemeath coverage summary\n");
    output.push_str(&format!(
        "{:<16} {:>24}  {:>24}  {:>24}\n",
        "package", "regions", "functions", "lines"
    ));
    output.push_str(&format!("{}\n", "-".repeat(94)));
    output.push_str(&format_row("workspace", &workspace));
    output.push('\n');
    output.push_str(&format!("{}\n", "-".repeat(94)));
    for (name, _) in packages {
        if name == "xtask" {
            continue;
        }
        let values = &per_package[&name];
        if METRICS.iter().all(|metric| values[*metric].count == 0) {
            output.push_str(&format!(
                "{name:<16} {:>24}  {:>24}  {:>24}\n",
                "n/a", "n/a", "n/a"
            ));
        } else {
            output.push_str(&format_row(&name, values));
            output.push('\n');
        }
    }
    Ok(output)
}

fn empty_metrics() -> BTreeMap<String, Metric> {
    METRICS
        .into_iter()
        .map(|name| (name.to_owned(), Metric::default()))
        .collect()
}

fn format_row(label: &str, metrics: &BTreeMap<String, Metric>) -> String {
    let columns = METRICS.map(|name| {
        let metric = metrics[name];
        let percentage = if metric.count == 0 {
            100.0
        } else {
            metric.covered as f64 / metric.count as f64 * 100.0
        };
        format!("{}/{} ({percentage:.2}%)", metric.covered, metric.count)
    });
    format!(
        "{label:<16} {:>24}  {:>24}  {:>24}",
        columns[0], columns[1], columns[2]
    )
}

fn absolutize(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}
