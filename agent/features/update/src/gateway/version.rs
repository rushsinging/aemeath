use sdk::{SdkError, VersionCheck};
use semver::Version;

use crate::contract::GitHubRelease;

/// 从 tag_name 去掉 `v` 前缀。
pub(super) fn strip_v_prefix(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// 解析当前版本号。
pub(super) fn current_version() -> Version {
    Version::parse(share::version()).expect("AEMEATH_VERSION / CARGO_PKG_VERSION 必须是合法 semver")
}

/// 从 GitHubRelease 构造 VersionCheck。
pub(super) fn build_version_check(release: &GitHubRelease) -> Result<VersionCheck, SdkError> {
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
