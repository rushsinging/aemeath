//! `aemeath cleanup`：统一清理无引用落盘数据（#2089）。
//!
//! 范围（MVP）：
//! - Workflow 实体：未被任何 Session 索引引用的 `~/.agents/workflows/entities/*.json`；
//!
//! 约束：
//! - 只清理无引用数据；仍被索引（含 active 指针）引用的实体一律保留；
//! - 幂等：重复执行不再删除；
//! - 清理前二次确认（`--yes` 跳过）；`--dry-run` 只列出将删除项；
//! - 不新增 workflow_* Tool——清理是用户显式的运维动作，不是 LLM 语义操作。

use std::io::{self, Write};

use composition::workflow_cleanup;

/// 处理 `aemeath cleanup` 子命令。
pub(crate) async fn run_cleanup_command(dry_run: bool, yes: bool) {
    if dry_run {
        match workflow_cleanup::cleanup_orphan_workflows(true) {
            Ok(report) => print_report(&report, true),
            Err(error) => {
                eprintln!("cleanup 失败（未删除任何数据）：{error}");
                std::process::exit(1);
            }
        }
        return;
    }

    // 先 dry-run 预览，确认后执行；避免「先删再问」。
    let preview = match workflow_cleanup::cleanup_orphan_workflows(true) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("cleanup 失败（未删除任何数据）：{error}");
            std::process::exit(1);
        }
    };

    if preview.removed.is_empty() {
        println!("没有可清理的无引用 Workflow 实体。");
        return;
    }

    println!("将删除以下无引用 Workflow 实体（实体文件不可恢复）：");
    for workflow_id in &preview.removed {
        println!("  - {workflow_id}");
    }
    println!(
        "共 {} 个；保留 {} 个仍被引用的实体（扫描 {} 个）。",
        preview.removed.len(),
        preview.referenced,
        preview.scanned
    );

    if !yes {
        print!("确认清理？此操作不可恢复 [y/N]: ");
        let _ = io::stdout().flush();
        let mut answer = String::new();
        if io::stdin().read_line(&mut answer).is_err() {
            eprintln!("读取确认失败，已取消。");
            std::process::exit(1);
        }
        let answer = answer.trim().to_ascii_lowercase();
        if answer != "y" && answer != "yes" {
            println!("已取消，未删除任何数据。");
            return;
        }
    }

    match workflow_cleanup::cleanup_orphan_workflows(false) {
        Ok(report) => print_report(&report, false),
        Err(error) => {
            eprintln!("cleanup 失败：{error}");
            std::process::exit(1);
        }
    }
}

fn print_report(report: &workflow_cleanup::CleanupReport, dry_run: bool) {
    if dry_run {
        println!(
            "[dry-run] 扫描 {} 个实体，保留 {} 个被引用，将删除 {} 个：",
            report.scanned,
            report.referenced,
            report.removed.len()
        );
    } else {
        println!(
            "完成：扫描 {} 个实体，保留 {} 个被引用，删除 {} 个。",
            report.scanned,
            report.referenced,
            report.removed.len()
        );
    }
    for workflow_id in &report.removed {
        println!("  - {workflow_id}");
    }
}
