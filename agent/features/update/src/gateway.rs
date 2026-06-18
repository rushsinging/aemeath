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
        // 1. 强制检查最新版本
        let check = self.force_check().await?;
        if !check.is_update_available {
            return Ok(UpdateResult::UpToDate {
                version: check.current_version,
            });
        }

        // 2. 平台匹配 → 确定 artifact 文件名
        let target = platform_target().ok_or_else(|| {
            SdkError::Internal(format!(
                "当前平台暂不支持自动更新 (os={}, arch={})",
                std::env::consts::OS,
                std::env::consts::ARCH
            ))
        })?;
        let version = &check.latest_version;
        let archive_name = format!("aemeath-{version}-{target}.tar.gz");

        // 3. 下载 checksums.txt 并解析
        let checksums_url = download_url(version, "checksums.txt");
        let checksums_text = self
            .download_text(&checksums_url)
            .await
            .map_err(|e| SdkError::Internal(format!("下载 checksums.txt 失败: {e}")))?;
        let expected_hash = parse_checksums(&checksums_text, &archive_name).ok_or_else(|| {
            SdkError::Internal(format!("checksums.txt 中未找到 {archive_name} 的校验值"))
        })?;

        // 4. 下载 tar.gz
        let archive_url = download_url(version, &archive_name);
        let archive_bytes = self
            .download_bytes(&archive_url)
            .await
            .map_err(|e| SdkError::Internal(format!("下载 {archive_name} 失败: {e}")))?;

        // 5. SHA256 校验
        let actual_hash = sha256_hex(&archive_bytes);
        if actual_hash != expected_hash {
            return Err(SdkError::Internal(format!(
                "SHA256 校验失败：期望 {expected_hash}，实际 {actual_hash}"
            )));
        }
        log::info!(target: LOG_TARGET, "SHA256 校验通过");

        // 6. 解压 tar.gz 提取二进制
        let new_binary = extract_binary_from_tar_gz(&archive_bytes)
            .map_err(|e| SdkError::Internal(format!("解压 {archive_name} 失败: {e}")))?;

        // 7. 原子替换 current_exe
        let current_exe = std::env::current_exe()
            .map_err(|e| SdkError::Internal(format!("获取当前可执行文件路径失败: {e}")))?;

        let temp_path = current_exe.with_extension("new");
        std::fs::write(&temp_path, &new_binary).map_err(|e| {
            SdkError::Internal(format!("写入临时文件失败 {}: {e}", temp_path.display()))
        })?;

        std::fs::rename(&temp_path, &current_exe).map_err(|e| {
            let _ = std::fs::remove_file(&temp_path);
            SdkError::Internal(format!(
                "原子替换失败（权限不足或文件被占用）{}: {e}",
                current_exe.display()
            ))
        })?;

        log::info!(
            target: LOG_TARGET,
            "更新完成: {} → {version}",
            check.current_version
        );

        Ok(UpdateResult::Updated {
            from: check.current_version,
            to: check.latest_version,
        })
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

    /// 下载文本内容（如 checksums.txt）。
    async fn download_text(&self, url: &str) -> Result<String, reqwest::Error> {
        log::debug!(target: LOG_TARGET, "下载: {url}");
        self.http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await
    }

    /// 下载二进制内容（如 tar.gz）。
    async fn download_bytes(&self, url: &str) -> Result<Vec<u8>, reqwest::Error> {
        log::debug!(target: LOG_TARGET, "下载: {url}");
        self.http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await
            .map(|b| b.to_vec())
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

// ── perform_update 辅助函数 ───────────────────────────────────────

/// 返回当前平台的 Rust target triple（用于匹配 artifact 文件名）。
///
/// 支持的平台：
/// - macOS aarch64 → `aarch64-apple-darwin`
/// - macOS x86_64  → `x86_64-apple-darwin`
/// - Linux x86_64  → `x86_64-unknown-linux-gnu`
fn platform_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        _ => None,
    }
}

/// 构造 GitHub Release 下载 URL。
fn download_url(version: &str, filename: &str) -> String {
    format!("https://github.com/rushsinging/aemeath/releases/download/v{version}/{filename}")
}

/// 从 checksums.txt 内容中查找指定文件名的 SHA256 值。
///
/// checksums.txt 格式（sha256sum 输出）：
/// ```text
/// a1b2c3...  aemeath-0.9.0-aarch64-apple-darwin.tar.gz
/// d4e5f6...  aemeath-0.9.0-x86_64-apple-darwin.tar.gz
/// ```
fn parse_checksums(content: &str, archive_name: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // 格式：`<hash>  <filename>`（两个空格分隔）
        let mut parts = line.splitn(2, |c: char| c.is_whitespace());
        let hash = parts.next()?.trim();
        let name = parts.next()?.trim();
        if name == archive_name {
            return Some(hash.to_lowercase());
        }
    }
    None
}

/// 计算字节数组的 SHA256 十六进制摘要。
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    // 手动 hex 编码，避免额外依赖 hex crate
    let mut hex = String::with_capacity(64);
    for byte in result {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// 从 tar.gz 归档中提取 `aemeath` 二进制文件内容。
///
/// 归档结构：`aemeath-{version}-{target}/aemeath`
fn extract_binary_from_tar_gz(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let gz = flate2::read::GzDecoder::new(data);
    let mut archive = tar::Archive::new(gz);

    for entry in archive
        .entries()
        .map_err(|e| format!("读取 tar 条目失败: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("解析 tar 条目失败: {e}"))?;
        let path = entry.path().map_err(|e| format!("读取条目路径失败: {e}"))?;
        // 匹配归档内任意层级的 `aemeath` 文件
        if path.file_name().is_some_and(|f| f == "aemeath") {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("读取二进制内容失败: {e}"))?;
            return Ok(buf);
        }
    }

    Err("归档中未找到 aemeath 二进制文件".into())
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

    // ── perform_update 辅助函数测试 ──────────────────────────────────

    use std::io::Write;

    #[test]
    fn test_platform_target_matches_current() {
        let target = platform_target();
        assert!(
            target.is_some(),
            "当前平台 (os={}, arch={}) 应受支持",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
    }

    #[test]
    fn test_download_url_format() {
        let url = download_url("0.9.0", "aemeath-0.9.0-aarch64-apple-darwin.tar.gz");
        assert_eq!(
            url,
            "https://github.com/rushsinging/aemeath/releases/download/v0.9.0/aemeath-0.9.0-aarch64-apple-darwin.tar.gz"
        );
    }

    #[test]
    fn test_parse_checksums_found() {
        let content = "\
a1b2c3d4e5f6  aemeath-0.9.0-aarch64-apple-darwin.tar.gz\n\
f7e8d9c0b1a2  aemeath-0.9.0-x86_64-apple-darwin.tar.gz\n";
        let hash = parse_checksums(content, "aemeath-0.9.0-aarch64-apple-darwin.tar.gz");
        assert_eq!(hash.as_deref(), Some("a1b2c3d4e5f6"));
    }

    #[test]
    fn test_parse_checksums_not_found() {
        let content = "a1b2c3d4e5f6  other-file.tar.gz\n";
        let hash = parse_checksums(content, "aemeath-0.9.0-aarch64-apple-darwin.tar.gz");
        assert!(hash.is_none());
    }

    #[test]
    fn test_parse_checksums_case_insensitive_hash() {
        let content = "A1B2C3D4E5F6  aemeath-0.9.0-aarch64-apple-darwin.tar.gz\n";
        let hash = parse_checksums(content, "aemeath-0.9.0-aarch64-apple-darwin.tar.gz");
        assert_eq!(hash.as_deref(), Some("a1b2c3d4e5f6"));
    }

    #[test]
    fn test_parse_checksums_empty_and_whitespace() {
        let content = "\n  \n  \n";
        let hash = parse_checksums(content, "any.tar.gz");
        assert!(hash.is_none());
    }

    #[test]
    fn test_sha256_hex_known_value() {
        // SHA256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        let hash = sha256_hex(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_sha256_hex_empty() {
        // SHA256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let hash = sha256_hex(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_extract_binary_from_tar_gz() {
        // 构造一个最小 tar.gz 归档：目录 + 文件 aemeath
        let mut builder = tar::Builder::new(Vec::new());
        let dir_path = "aemeath-0.9.0-x86_64-apple-darwin/";
        let mut dir_header = tar::Header::new_gnu();
        dir_header.set_path(dir_path).unwrap();
        dir_header.set_size(0);
        dir_header.set_entry_type(tar::EntryType::Directory);
        dir_header.set_mode(0o755);
        dir_header.set_cksum();
        builder.append(&dir_header, std::io::empty()).unwrap();

        let bin_data = b"#!/bin/sh\necho fake binary\n";
        let mut file_header = tar::Header::new_gnu();
        file_header.set_path(format!("{dir_path}aemeath")).unwrap();
        file_header.set_size(bin_data.len() as u64);
        file_header.set_entry_type(tar::EntryType::Regular);
        file_header.set_mode(0o755);
        file_header.set_cksum();
        builder.append(&file_header, &bin_data[..]).unwrap();

        let tar_bytes = builder.into_inner().unwrap();
        let mut gz_buf = Vec::new();
        {
            let mut encoder =
                flate2::write::GzEncoder::new(&mut gz_buf, flate2::Compression::default());
            encoder.write_all(&tar_bytes).unwrap();
            encoder.finish().unwrap();
        }

        let extracted = extract_binary_from_tar_gz(&gz_buf).unwrap();
        assert_eq!(extracted, bin_data);
    }

    #[test]
    fn test_extract_binary_from_tar_gz_not_found() {
        let mut builder = tar::Builder::new(Vec::new());
        let data = b"some content";
        let mut header = tar::Header::new_gnu();
        header.set_path("other.txt").unwrap();
        header.set_size(data.len() as u64);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &data[..]).unwrap();

        let tar_bytes = builder.into_inner().unwrap();
        let mut gz_buf = Vec::new();
        {
            let mut encoder =
                flate2::write::GzEncoder::new(&mut gz_buf, flate2::Compression::default());
            encoder.write_all(&tar_bytes).unwrap();
            encoder.finish().unwrap();
        }

        let result = extract_binary_from_tar_gz(&gz_buf);
        assert!(result.is_err());
    }
}
