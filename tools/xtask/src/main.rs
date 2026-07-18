use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("changed-lines") => {
            let coverage = fs::read_to_string(args.next().context("缺少 coverage JSON")?)?;
            let diff = fs::read_to_string(args.next().context("缺少 diff patch")?)?;
            let changed = xtask::changed_lines::parse_diff(&diff)?;
            let report = xtask::changed_lines::report(&coverage, &changed, &env::current_dir()?)?;
            let percent = if report.changed == 0 { 100.0 } else { report.covered as f64 / report.changed as f64 * 100.0 };
            println!("changed lines: {}/{} ({percent:.2}%)", report.covered, report.changed);
            for missing in report.missing { println!("uncovered: {missing}"); }
            Ok(())
        }
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
        Some("run-test") => {
            let retries: usize = args.next().unwrap_or_else(|| "1".into()).parse()?;
            let command: Vec<String> = args.collect();
            if command.is_empty() { anyhow::bail!("run-test 缺少命令"); }
            let report = xtask::flaky::run_with_retry(&command.join(" "), retries, || {
                Command::new(&command[0]).args(&command[1..]).status().map(|status| status.code().unwrap_or(1)).unwrap_or(1)
            });
            println!("first_exit={} retries={:?} classification={}", report.first_exit, report.retry_exits, report.classification);
            if !report.passed { anyhow::bail!("首次测试失败，分类={}", report.classification); }
            Ok(())
        }
        Some("guard-registry") => {
            let action = args.next().unwrap_or_else(|| "check".to_owned());
            let root = PathBuf::from(args.next().unwrap_or_else(|| ".".to_owned()));
            match action.as_str() {
                "check" => {
                    let report = xtask::guard_registry::check_workspace(&root, None)?;
                    print!("{}", report.render());
                    Ok(())
                }
                "report" => {
                    let output = args.next().map(PathBuf::from);
                    let report =
                        xtask::guard_registry::check_workspace(&root, output.as_deref())?;
                    print!("{}", report.render());
                    Ok(())
                }
                _ => anyhow::bail!("guard-registry 仅支持 check 或 report"),
            }
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
            "用法: cargo run -p xtask -- <coverage-summary <report.json> <root>|production-reachability [root]|guard-registry <check|report> [root] [output]|source-guard [root] [public-surface-output]>"
        ),
    }
}
