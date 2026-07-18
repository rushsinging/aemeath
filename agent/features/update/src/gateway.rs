//! 版本检查与自动更新 Gateway。
//!
//! 对应设计文档：`docs/snapshot/release-update-design.md` 子系统 2（版本检查）。
//! 子系统 3（自动更新）见本文件 `perform_update`。

mod archive;
mod checksum;
mod platform;
mod version;

#[cfg(test)]
mod tests;

use crate::contract::GitHubRelease;
use archive::extract_binary_from_tar_gz;
use async_trait::async_trait;
use checksum::{parse_checksums, sha256_hex};
use platform::{download_url, platform_target};
use sdk::{SdkError, UpdateResult, UpdateService, VersionCheck};
use version::build_version_check;

/// GitHub Releases API endpoint（匿名访问，限速 60 次/小时）。
const GITHUB_API_URL: &str = "https://api.github.com/repos/rushsinging/aemeath/releases/latest";

/// HTTP 请求超时（秒）—— 用于元数据请求（GitHub API JSON / checksums.txt）。
const REQUEST_TIMEOUT_SECS: u64 = 5;

/// 二进制下载超时（秒）—— 用于 tar.gz artifact（可达数 MB）。
/// 5s 对 6.5MB tar.gz 经常不够（含 TLS 握手 + GitHub 302 跳转），
/// 过短会中断 body 流导致 `error decoding response body`。见 issue #350。
const DOWNLOAD_TIMEOUT_SECS: u64 = 120;

// ── public API ───────────────────────────────────────────────────

/// 版本检查与自动更新 Gateway。
///
/// 每次 `check_latest()` 都会直接调 GitHub Releases API，不做本地缓存：
/// GitHub 匿名 API 限速 60 次/小时，普通 dev tool 不会打满；
/// 每次检查都拿到最新数据，避免缓存过期漏报新版本。
pub struct UpdateGateway {
    /// 元数据请求 client（短超时）。
    http: reqwest::Client,
    /// 二进制下载 client（长超时）。
    download: reqwest::Client,
}

impl UpdateGateway {
    /// 创建 Gateway。
    pub fn new() -> Self {
        let ua = format!("aemeath/{}", share::version());
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .user_agent(&ua)
            .build()
            .unwrap_or_default();
        let download = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
            .user_agent(ua)
            .build()
            .unwrap_or_default();
        Self { http, download }
    }
}

impl Default for UpdateGateway {
    fn default() -> Self {
        Self::new()
    }
}

// ── sdk::UpdateService impl ──────────────────────────────────────

#[async_trait]
impl UpdateService for UpdateGateway {
    async fn check_latest(&self) -> Result<VersionCheck, SdkError> {
        // 无缓存策略：每次都直接查 GitHub API。
        self.force_check().await
    }

    async fn force_check(&self) -> Result<VersionCheck, SdkError> {
        let release = self.fetch_latest_release().await?;
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
        log::info!(target: crate::LOG_TARGET, "SHA256 校验通过");

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
            target: crate::LOG_TARGET,
            "更新完成: {} → {version}（安装路径: {}）",
            check.current_version,
            current_exe.display()
        );

        Ok(UpdateResult::Updated {
            from: check.current_version,
            to: check.latest_version,
            installed_path: current_exe.to_string_lossy().into_owned(),
        })
    }
}

// ── 内部方法 ─────────────────────────────────────────────────────

impl UpdateGateway {
    /// 调用 GitHub Releases API 获取最新 release。
    async fn fetch_latest_release(&self) -> Result<GitHubRelease, SdkError> {
        log::debug!(target: crate::LOG_TARGET, "请求 GitHub API: {GITHUB_API_URL}");

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
            target: crate::LOG_TARGET,
            "最新版本: {} (prerelease={})",
            release.tag_name,
            release.prerelease
        );

        Ok(release)
    }

    /// 下载文本内容（如 checksums.txt）。
    async fn download_text(&self, url: &str) -> Result<String, reqwest::Error> {
        log::debug!(target: crate::LOG_TARGET, "下载: {url}");
        self.http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await
    }

    /// 下载二进制内容（如 tar.gz）。使用长超时 client（见 issue #350）。
    async fn download_bytes(&self, url: &str) -> Result<Vec<u8>, reqwest::Error> {
        log::debug!(target: crate::LOG_TARGET, "下载: {url}");
        self.download
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await
            .map(|b| b.to_vec())
    }
}
