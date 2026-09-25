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
        Some("sdk-wire-schema") => {
            let action = args.next().unwrap_or_else(|| "check".to_owned());
            let output = PathBuf::from(
                args.next()
                    .unwrap_or_else(|| "packages/sdk/schema/wire-components.schema.json".to_owned()),
            );
            match action.as_str() {
                "write" => xtask::sdk_wire_schema::write(&output),
                "check" => xtask::sdk_wire_schema::check(&output),
                _ => anyhow::bail!("sdk-wire-schema 仅支持 write 或 check"),
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
        Some("facade") => {
            // xtask facade trim [--apply]：死导出计算与原子下架。
            let apply = args.any(|flag| flag == "--apply");
            let root = PathBuf::from(env::var("AEMEATH_PROJECT_DIR").unwrap_or_else(|_| ".".into()));
            let mut total_dead = 0usize;
            let mut total_trimmed = 0usize;
            for crate_name in xtask::guards_facade_trim::CRATES {
                let report = xtask::guards_facade_trim::analyze_crate(&root, crate_name)?;
                let dead = xtask::guards_facade_trim::dead_exports(&report);
                if apply && !dead.is_empty() {
                    xtask::guards_facade_trim::rewrite_internal_root_consumption(
                        &root, crate_name, &report,
                    )?;
                    // 折返改写后重算死集（折返符号转为死）。
                    let report2 = xtask::guards_facade_trim::analyze_crate(&root, crate_name)?;
                    let dead2 = xtask::guards_facade_trim::dead_exports(&report2);
                    let removed =
                        xtask::guards_facade_trim::apply_trim(&root, crate_name, &dead2)?;
                    println!(
                        "{crate_name}: 死 {}，下架 {}（折返改写 {} 处）",
                        dead2.len(),
                        removed,
                        report.internal_root_consumed.len()
                    );
                    total_trimmed += removed;
                } else {
                    println!(
                        "{crate_name}: 导出 {}，跨消费 {}，内部折返 {}，死 {}",
                        report.exports.len(),
                        report.cross_consumed.len(),
                        report.internal_root_consumed.len(),
                        dead.len()
                    );
                }
                total_dead += dead.len();
            }
            if apply {
                println!("合计下架 {total_trimmed}");
            } else {
                println!("合计死导出 {total_dead}（--apply 执行下架）");
            }
            Ok(())
        }
        Some("guard") => {
            let mut profile = xtask::guards::Profile::Full;
            let mut rule_filter: Option<String> = None;
            let mut rest = args;
            while let Some(flag) = rest.next() {
                match flag.as_str() {
                    "--fast" => profile = xtask::guards::Profile::Fast,
                    "--full" => profile = xtask::guards::Profile::Full,
                    "--rule" => {
                        rule_filter =
                            Some(rest.next().context("--rule 需要规则 id 参数")?);
                    }
                    other => anyhow::bail!("未知 guard 参数：{other}"),
                }
            }
            let root = PathBuf::from(env::var("AEMEATH_PROJECT_DIR").unwrap_or_else(|_| ".".into()));
            let report = xtask::guards::run(&root, profile, rule_filter.as_deref())?;
            if !report.violations.is_empty() {
                eprint!("{}", report.render());
                eprintln!();
                std::process::exit(2);
            }
            println!(
                "guard: {} rules checked, 0 violations, {:?}",
                report.rules_run, report.duration
            );
            Ok(())
        }
        _ => anyhow::bail!(
            "用法: cargo run -p xtask -- <coverage-summary <report.json> <root>|production-reachability [root]|guard-registry <check|report> [root] [output]|sdk-wire-schema <write|check> [output]|source-guard [root] [public-surface-output]|guard [--fast|--full|--rule <id>]>"
        ),
    }
}
