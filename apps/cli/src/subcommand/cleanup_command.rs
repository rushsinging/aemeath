//! `aemeath cleanup`：统一落盘清理入口。
//!
//! 两类判据（specs/3.10-storage.md）：
//! - 无引用：Workflow 孤儿实体（`~/.agents/workflows`）；
//! - 过期：session 三件套联动、audit、tmp、`~/.agents` 旧布局遗留，
//!   按 `cleanup.retention_days`（全局默认 30，分项覆盖，`0` 禁用）。
//!
//! 约束：
//! - 先 dry-run 预览，确认后执行（`--yes` 跳过确认）；`--dry-run` 止步预览；
//! - `--days` 覆盖全局默认，分项覆盖仍优先生效；
//! - 幂等；单条目删除失败记入 errors 不中止整体；
//! - `state.bak-*` 备份目录只报告体积，永不自动删除；
//! - 不新增 cleanup Tool——清理是用户显式运维动作，不是 LLM 语义操作。

use std::io::{self, Write};

use composition::disk_cleanup::{self, DiskCleanupReport};
use composition::workflow_cleanup;

use crate::constants::REMOVED_PREVIEW_LINES;

/// 处理 `aemeath cleanup` 子命令。
pub(crate) async fn run_cleanup_command(dry_run: bool, yes: bool, days: Option<u64>) {
    let mut retention = composition::app::configured_cleanup_retention(Default::default())
        .await
        .unwrap_or_else(|error| {
            eprintln!("读取配置失败，使用默认保留天数（30 天）：{error}");
            Default::default()
        });
    if let Some(days) = days.filter(|days| *days > 0) {
        retention.default = days;
    }

    // 预览阶段：无引用 + 过期两面同扫描口径。
    let workflow_preview = match workflow_cleanup::cleanup_orphan_workflows(true) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("cleanup 失败（未删除任何数据）：{error}");
            std::process::exit(1);
        }
    };
    let disk_preview = match disk_cleanup::run_disk_cleanup(Some(retention.clone()), true) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("cleanup 失败（未删除任何数据）：{error}");
            std::process::exit(1);
        }
    };

    if dry_run {
        print_workflow_section(&workflow_preview, true);
        print_disk_sections(&disk_preview);
        print_advisories(&disk_preview);
        return;
    }

    let workflow_pending = !workflow_preview.removed.is_empty();
    if !workflow_pending && !disk_preview.has_pending() {
        println!("没有可清理的数据（无引用 Workflow 实体与过期条目均为空）。");
        print_advisories(&disk_preview);
        return;
    }

    println!("以下数据将被删除（不可恢复）：");
    print_workflow_section(&workflow_preview, true);
    print_disk_sections(&disk_preview);
    println!(
        "合计回收约 {}。",
        human_bytes(disk_preview.total_reclaimed_bytes())
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

    if workflow_pending {
        match workflow_cleanup::cleanup_orphan_workflows(false) {
            Ok(report) => print_workflow_section(&report, false),
            Err(error) => eprintln!("Workflow 实体清理失败：{error}"),
        }
    }
    match disk_cleanup::run_disk_cleanup(Some(retention), false) {
        Ok(report) => {
            print_disk_sections(&report);
            println!(
                "完成：过期清理回收约 {}。",
                human_bytes(report.total_reclaimed_bytes())
            );
        }
        Err(error) => eprintln!("过期清理失败：{error}"),
    }
    print_advisories(&disk_preview);
}

fn print_workflow_section(report: &workflow_cleanup::CleanupReport, preview: bool) {
    let prefix = if preview { "[dry-run] " } else { "" };
    println!(
        "{prefix}[workflows] 扫描 {} 个实体，保留 {} 个被引用，{} {} 个。",
        report.scanned,
        report.referenced,
        if preview { "将删除" } else { "已删除" },
        report.removed.len()
    );
    for workflow_id in &report.removed {
        println!("  - {workflow_id}");
    }
}

fn print_disk_sections(report: &DiskCleanupReport) {
    for face in &report.faces {
        if face.disabled {
            println!(
                "[{}] 已禁用（cleanup.retention_days.{} = 0），跳过。",
                face.face, face.face
            );
            continue;
        }
        let prefix = if report.dry_run { "[dry-run] " } else { "" };
        let verb = if report.dry_run {
            "将回收"
        } else {
            "已回收"
        };
        println!(
            "{prefix}[{}] 扫描 {} 项，命中 {} 项，{}约 {}。",
            face.face,
            face.scanned,
            face.removed.len(),
            verb,
            human_bytes(face.reclaimed_bytes)
        );
        for item in face.removed.iter().take(REMOVED_PREVIEW_LINES) {
            println!("  - {item}");
        }
        let hidden = face.removed.len().saturating_sub(REMOVED_PREVIEW_LINES);
        if hidden > 0 {
            println!("  ……及其它 {hidden} 项（省略展示，清理时同样处理）。");
        }
        for error in &face.errors {
            println!("  ! {error}");
        }
    }
}

fn print_advisories(report: &DiskCleanupReport) {
    for advisory in &report.advisories {
        println!(
            "[提示] 检测到备份目录 {}（约 {}）——cleanup 不会自动删除，确无需要请手动清理。",
            advisory.label,
            human_bytes(advisory.bytes)
        );
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
