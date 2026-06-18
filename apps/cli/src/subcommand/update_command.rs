//! `aemeath update` 子命令 — 版本检查与自动更新。
//!
//! - `aemeath update` — 检查并尝试自动更新（PR 3 实现自动下载）
//! - `aemeath update --check` — 仅检查，不更新

use sdk::UpdateResult;

pub(crate) async fn run_update_command(check: bool) {
    let service = composition::update::wire_update();

    // --check 模式忽略缓存，强制查 API
    match service.force_check().await {
        Ok(result) => {
            if result.is_update_available {
                println!(
                    "Update available: {} → {}",
                    result.current_version, result.latest_version
                );
                if let Some(notes) = &result.release_notes {
                    let truncated = if notes.chars().count() > 200 {
                        let short: String = notes.chars().take(200).collect();
                        format!("{short}\n...(truncated)")
                    } else {
                        notes.clone()
                    };
                    println!("\n{truncated}");
                }
                println!("\nRelease: {}", result.release_url);

                if !check {
                    match service.perform_update().await {
                        Ok(UpdateResult::Updated { from, to }) => {
                            println!("\n✓ Updated {from} → {to}");
                            println!("Please restart aemeath to use the new version.");
                        }
                        Ok(UpdateResult::UpToDate { version }) => {
                            println!("\nAlready up to date ({version}).");
                        }
                        Ok(UpdateResult::CheckOnly(_)) => {}
                        Err(e) => {
                            eprintln!("\nAuto-update not available: {e}");
                            eprintln!("Please update manually from: {}", result.release_url);
                        }
                    }
                }
            } else {
                println!("Already up to date ({})", result.current_version);
            }
        }
        Err(e) => {
            eprintln!("Failed to check for updates: {e}");
            std::process::exit(1);
        }
    }
}
