//! 版本检查与自动更新 Gateway。
//!
//! 对应设计文档：`docs/design/release-update-design.md` 子系统 2（版本检查）。
//! 子系统 3（自动更新）在 PR 3 中实现 `perform_update`。

use std::path::PathBuf;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sdk::{SdkError, UpdateResult, UpdateService, VersionCheck};
use semver::Version;

use crate::contract::{CacheEntry, GitHubRelease};
use crate::LOG_TARGET;

/// GitHub Releases API endpoint（匿名访问，限速 60 次/小时）。
const GITHUB_API_URL: &str = "https://api.github.com/repos/rushsinging/aemeath/releases/latest";

/// 缓存最大有效期（小时）。
const CACHE_MAX_AGE_HOURS: i64 = 24;

/// HTTP 请求超时（秒）。
const REQUEST_TIMEOUT_SECS: u64 = 5;

// ── public API ───────────────────────────────────────────────────

/// 版本检查与自动更新 Gateway。
pub struct UpdateGateway {
    http: reqwest::Client,
    cache_path: PathBuf,
}

impl UpdateGateway {
    /// 创建 Gateway。`cache_path` 通常为 `~/.agents/update_check.json`。
    pub fn new(cache_path: PathBuf) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .user_agent(format!("aemeath/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self { http, cache_path }
    }
}

// ── sdk::UpdateService impl ──────────────────────────────────────

#[async_trait]
impl UpdateService for UpdateGateway {
    async fn check_latest(&self) -> Result<VersionCheck, SdkError> {
        // 先检查缓存是否新鲜
        if let Some(cache) = self.load_cache() {
            if is_cache_fresh(&cache) {
                log::debug!(
                    target: LOG_TARGET,
                    "使用缓存（last_check={}）",
                    cache.last_check
                );
                return build_check_from_cache(&cache);
            }
        }

        // 缓存过期或不存在 → 调 API
        self.force_check().await
    }

    async fn force_check(&self) -> Result<VersionCheck, SdkError> {
        let release = self.fetch_latest_release().await?;
        self.save_cache(&release);
        build_version_check(&release)
    }

    async fn perform_update(&self) -> Result<UpdateResult, SdkError> {
        // PR 3 实现：下载 → SHA256 校验 → 原子替换
        Err(SdkError::Internal(
            "自动更新尚未实现（将在 PR 3 中完成）".into(),
        ))
    }
}

// ── 内部方法 ─────────────────────────────────────────────────────

impl UpdateGateway {
    /// 调用 GitHub Releases API 获取最新 release。
    async fn fetch_latest_release(&self) -> Result<GitHubRelease, SdkError> {
        log::debug!(target: LOG_TARGET, "请求 GitHub API: {GITHUB_API_URL}");

        let resp = self
            .http
            .get(GITHUB_API_URL)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| SdkError::Internal(format!("GitHub API 请求失败: {e}")))?;

        if !resp.status().is_success() {
            return Err(SdkError::Internal(format!(
                "GitHub API 返回非成功状态码: {}",
                resp.status()
            )));
        }

        let release: GitHubRelease = resp
            .json()
            .await
            .map_err(|e| SdkError::Internal(format!("解析 GitHub API 响应失败: {e}")))?;

        log::info!(
            target: LOG_TARGET,
            "最新版本: {} (prerelease={})",
            release.tag_name,
            release.prerelease
        );

        Ok(release)
    }

    /// 写入缓存文件（失败时静默降级，不影响主流程）。
    fn save_cache(&self, release: &GitHubRelease) {
        let entry = CacheEntry {
            last_check: Utc::now().to_rfc3339(),
            latest_version: strip_v_prefix(&release.tag_name).to_string(),
            latest_url: release.html_url.clone(),
        };

        if let Some(parent) = self.cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        match serde_json::to_string(&entry) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&self.cache_path, json) {
                    log::warn!(target: LOG_TARGET, "写入更新缓存失败 {}: {e}", self.cache_path.display());
                }
            }
            Err(e) => log::warn!(target: LOG_TARGET, "序列化更新缓存失败: {e}"),
        }
    }

    /// 读取缓存文件（不存在或解析失败时返回 None）。
    fn load_cache(&self) -> Option<CacheEntry> {
        let content = std::fs::read_to_string(&self.cache_path).ok()?;
        serde_json::from_str(&content).ok()
    }
}

// ── 纯函数（便于单元测试） ───────────────────────────────────────

/// 从 tag_name 去掉 `v` 前缀。
fn strip_v_prefix(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// 解析当前版本号。
fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("CARGO_PKG_VERSION 必须是合法 semver")
}

/// 判断缓存是否在有效期内。
fn is_cache_fresh(entry: &CacheEntry) -> bool {
    match DateTime::parse_from_rfc3339(&entry.last_check) {
        Ok(last_check) => {
            let elapsed = Utc::now().signed_duration_since(last_check.with_timezone(&Utc));
            elapsed.num_hours() < CACHE_MAX_AGE_HOURS
        }
        Err(_) => false,
    }
}

/// 从 GitHubRelease 构造 VersionCheck。
fn build_version_check(release: &GitHubRelease) -> Result<VersionCheck, SdkError> {
    let current = current_version();
    let latest_str = strip_v_prefix(&release.tag_name);
    let latest = Version::parse(latest_str)
        .map_err(|e| SdkError::Internal(format!("解析版本号 '{latest_str}' 失败: {e}")))?;

    Ok(VersionCheck {
        current_version: current.to_string(),
        latest_version: latest.to_string(),
        is_update_available: latest > current,
        release_url: release.html_url.clone(),
        release_notes: release.body.clone(),
    })
}

/// 从缓存构造 VersionCheck。
fn build_check_from_cache(cache: &CacheEntry) -> Result<VersionCheck, SdkError> {
    let current = current_version();
    let latest = Version::parse(&cache.latest_version)
        .map_err(|e| SdkError::Internal(format!("解析缓存版本号失败: {e}")))?;

    Ok(VersionCheck {
        current_version: current.to_string(),
        latest_version: latest.to_string(),
        is_update_available: latest > current,
        release_url: cache.latest_url.clone(),
        release_notes: None,
    })
}

// ── 单元测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_v_prefix_with_prefix() {
        assert_eq!(strip_v_prefix("v0.9.0"), "0.9.0");
    }

    #[test]
    fn test_strip_v_prefix_without_prefix() {
        assert_eq!(strip_v_prefix("0.9.0"), "0.9.0");
    }

    #[test]
    fn test_build_version_check_newer() {
        // 模拟一个比当前版本新的 release
        let release = GitHubRelease {
            tag_name: format!("v{}.{}.{}", current_version().major + 1, 0, 0),
            html_url: "https://example.com".into(),
            body: Some("notes".into()),
            prerelease: false,
        };
        let check = build_version_check(&release).unwrap();
        assert!(check.is_update_available);
        assert_eq!(check.release_url, "https://example.com");
    }

    #[test]
    fn test_build_version_check_same_version() {
        let release = GitHubRelease {
            tag_name: format!("v{}", current_version()),
            html_url: "https://example.com".into(),
            body: None,
            prerelease: false,
        };
        let check = build_version_check(&release).unwrap();
        assert!(!check.is_update_available);
        assert!(check.release_notes.is_none());
    }

    #[test]
    fn test_build_version_check_older_version() {
        // 模拟一个比当前版本旧的 release
        let release = GitHubRelease {
            tag_name: "v0.0.1".into(),
            html_url: "https://example.com".into(),
            body: None,
            prerelease: false,
        };
        let check = build_version_check(&release).unwrap();
        assert!(!check.is_update_available);
    }

    #[test]
    fn test_is_cache_fresh_recent() {
        let entry = CacheEntry {
            last_check: Utc::now().to_rfc3339(),
            latest_version: "0.9.0".into(),
            latest_url: "https://example.com".into(),
        };
        assert!(is_cache_fresh(&entry));
    }

    #[test]
    fn test_is_cache_fresh_expired() {
        let old = Utc::now() - chrono::Duration::hours(CACHE_MAX_AGE_HOURS + 1);
        let entry = CacheEntry {
            last_check: old.to_rfc3339(),
            latest_version: "0.9.0".into(),
            latest_url: "https://example.com".into(),
        };
        assert!(!is_cache_fresh(&entry));
    }

    #[test]
    fn test_is_cache_fresh_invalid_date() {
        let entry = CacheEntry {
            last_check: "not-a-date".into(),
            latest_version: "0.9.0".into(),
            latest_url: "https://example.com".into(),
        };
        assert!(!is_cache_fresh(&entry));
    }

    #[test]
    fn test_build_check_from_cache() {
        let cache = CacheEntry {
            last_check: Utc::now().to_rfc3339(),
            latest_version: format!("{}.{}.{}", current_version().major + 1, 0, 0),
            latest_url: "https://example.com".into(),
        };
        let check = build_check_from_cache(&cache).unwrap();
        assert!(check.is_update_available);
        assert!(check.release_notes.is_none());
    }
}
