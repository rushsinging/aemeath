//! `ArtifactFetcherPort` 的 HTTP 实现：固定 manifest 来源策略 + https-only +
//! 流式增量写入 + 目标路径安全。
//!
//! 安全与性能边界（fail-closed）：
//! - **流式**：`reqwest::Response::bytes_stream()` 分块 + `tokio::fs::File` 增量写入，
//!   **NEVER** `.bytes()` / `read_to_end`（GGUF 约 775MB，绝不整份进内存）。
//! - **长度**：Content-Length 有值且与 manifest 不符立即失败（建目标文件之前）；
//!   流累计超过 `byte_length` 立即失败；结束时 MUST 恰好等于 `byte_length`；
//!   成功路径最终 `flush` + `sync_all`，显式关闭文件后才返回。
//! - **来源（SSRF 边界）**：System One 下载的是固定可信 manifest，fetcher
//!   **不是任意 HTTPS 客户端**。构造时从注入的 [`ModelManifest`] assets 提取
//!   「path → 精确规范化 URL」白名单：每次初始请求的 path+URL MUST 完全匹配；
//!   重定向除 https 外每一跳 host 还必须命中 host 白名单（manifest URL hosts
//!   ∪ 构造器注入的 `allowed_redirect_hosts`，为 CDN 兼容预留）。
//!   host 策略在**策略构造期**执行：canonical lowercase、拒 userinfo、拒全部
//!   IP literal（loopback / private / link-local / multicast / unspecified 等
//!   一律按字面量拒绝）、拒 `localhost`/`.localhost`、拒单标签主机、拒控制 /
//!   空白字符。重定向到未允许 host typed [`ArtifactFetchErrorKind::UnsafeSource`]。
//!   设计边界声明：本策略是**固定 URL + host allowlist**，不做 DNS 解析、
//!   不声称防御全部 DNS rebinding；信任根是注入的固定 manifest。
//! - **超时**：注入值只作用于 `connect_timeout` + `read_timeout`（连接超时与
//!   单次读取空闲超时），**NEVER** 作为请求总预算 —— 775MB 资产的完整下载
//!   时长不受该值限制。
//! - **重定向**：`Policy::none()` 后逐跳手动跟随
//!   （最多 [`MAX_REDIRECT_HOPS`](crate::constants::MAX_REDIRECT_HOPS) 跳，
//!   每一跳都过同一 scheme + host 校验，禁止降级 http）。非 2xx 状态 typed 失败。
//! - **目标**：路径只能由 staging 根 + 已校验 `asset.path` 推导；父目录逐组件
//!   创建并拒绝符号链接；目标文件 `create_new` + unix `O_NOFOLLOW` + mode `0600`，
//!   已存在（含末段符号链接）一律拒绝，**NEVER 覆盖**。父组件探测与打开之间
//!   存在 TOCTOU 竞态窗口，完整性最终由安装端逐资产长度 + SHA-256 锁定。
//! - **配置**：超时、User-Agent 与 reqwest client 全部由构造器注入；
//!   不读环境变量（`no_proxy` 显式禁用代理环境），不硬编码 API key / base URL。
//!
//! 职责拆分：本文件 = client 构造 / 来源与重定向闸门 / 流式写入编排；
//! 同级 `fetch_http_destination.rs` = 相对路径复核、父目录安全创建与
//! `create_new` + `O_NOFOLLOW` 打开。

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;

use crate::constants::MAX_REDIRECT_HOPS;
use crate::domain::{ModelAsset, ModelManifest};
use crate::ports::{ArtifactFetchError, ArtifactFetchErrorKind, ArtifactFetcherPort};

#[path = "fetch_http_destination.rs"]
mod destination;
use destination::{open_destination_file, prepare_destination, validate_relative_asset_path};

#[cfg(all(test, unix))]
#[path = "fetch_http_tests.rs"]
mod tests;

/// 可信来源策略（固定 manifest 派生，fail-closed）。
///
/// - `allowed_assets`：`asset.path` → 精确规范化 URL；初始请求 MUST path+URL
///   完全匹配（`Url` 相等性按同一解析器的规范化结果比较）。
/// - `allowed_redirect_hosts`：允许出现在重定向任一跳的 host（canonical
///   lowercase）：manifest 全部资产 URL 的 host ∪ 构造器注入的 CDN host。
///
/// 生产形态下两个集合都在构造期经过 [`canonical_trusted_host`] 的 host 策略
/// 校验；构造期拒绝 localhost / IP literal / 单标签 / 控制空白等不可信 host，
/// 请求期只做集合成员判定（拒绝即 typed [`ArtifactFetchErrorKind::UnsafeSource`]）。
#[derive(Debug, Clone)]
struct SourcePolicy {
    /// asset path → 精确规范化 URL（初始请求完全匹配用）。
    allowed_assets: BTreeMap<String, reqwest::Url>,
    /// 允许的重定向 host（canonical lowercase）。
    allowed_redirect_hosts: BTreeSet<String>,
}

impl SourcePolicy {
    /// 生产构造：从固定 manifest 的 assets 提取精确 (path, URL) 集合，
    /// 并合并注入的额外重定向 host（CDN 兼容）；全部 host 过策略校验。
    fn from_manifest(
        manifest: &ModelManifest,
        allowed_redirect_hosts: Vec<String>,
    ) -> Result<Self, ArtifactFetchError> {
        let mut allowed_assets = BTreeMap::new();
        let mut allowed_redirect_hosts = allowed_redirect_hosts
            .into_iter()
            .map(|host| canonical_trusted_host(&host))
            .collect::<Result<BTreeSet<String>, ArtifactFetchError>>()?;
        for asset in &manifest.assets {
            let url = parse_trusted_https_url(&asset.url)?;
            if let Some(host) = url.host_str() {
                allowed_redirect_hosts.insert(host.to_ascii_lowercase());
            }
            allowed_assets.insert(asset.path.clone(), url);
        }
        Ok(Self {
            allowed_assets,
            allowed_redirect_hosts,
        })
    }

    /// 测试专用构造（`cfg(test)`，loopback 明文 HTTP wire 夹具）：
    /// 由测试显式给定 (path, URL) 白名单与重定向 host —— **绕过生产 host 策略**
    /// （127.0.0.1 是 IP literal，生产策略必然拒绝）；生产 source policy 另有
    /// 独立单测（见 `production_source_policy_*` 用例）。
    #[cfg(all(test, unix))]
    fn for_local_http_tests(
        allowed_assets: Vec<(String, String)>,
        allowed_redirect_hosts: Vec<String>,
    ) -> Result<Self, ArtifactFetchError> {
        let mut assets = BTreeMap::new();
        let mut hosts: BTreeSet<String> = allowed_redirect_hosts
            .into_iter()
            .map(|host| host.to_ascii_lowercase())
            .collect();
        for (path, url) in allowed_assets {
            let parsed = reqwest::Url::parse(&url).map_err(|error| ArtifactFetchError {
                kind: ArtifactFetchErrorKind::UnsafeSource,
                detail: format!("测试来源 URL 非法：{error}"),
            })?;
            if let Some(host) = parsed.host_str() {
                hosts.insert(host.to_ascii_lowercase());
            }
            assets.insert(path, parsed);
        }
        Ok(Self {
            allowed_assets: assets,
            allowed_redirect_hosts: hosts,
        })
    }

    /// 初始请求准入：path+URL MUST 与固定 manifest 派生的允许集合完全匹配
    /// （调用方已完成 URL 解析与 scheme / userinfo 校验）。
    fn initial_request_url(
        &self,
        asset_path: &str,
        requested: reqwest::Url,
    ) -> Result<reqwest::Url, ArtifactFetchError> {
        match self.allowed_assets.get(asset_path) {
            Some(allowed) if *allowed == requested => Ok(requested),
            _ => Err(ArtifactFetchError {
                kind: ArtifactFetchErrorKind::UnsafeSource,
                detail: format!(
                    "资产 {asset_path} 的初始请求 URL 不在固定 manifest 允许列表：{requested}"
                ),
            }),
        }
    }

    /// 重定向 host 准入：host MUST 命中允许集合（canonical lowercase 成员判定）。
    fn allows_redirect_host(&self, url: &reqwest::Url) -> bool {
        match url.host_str() {
            Some(host) => self
                .allowed_redirect_hosts
                .contains(&host.to_ascii_lowercase()),
            None => false,
        }
    }
}

/// 生产 host 策略（策略构造期执行）：canonical lowercase + 逐条拒绝规则。
///
/// 拒绝：空 host、userinfo 残留字符（`@`）、全部 IP literal（loopback /
/// private / link-local / multicast / unspecified 等，括号 IPv6 同样拒绝）、
/// `localhost` 与 `.localhost` 后缀、单标签主机、尾点 / 空标签、
/// 控制 / 空白 / 非 ASCII 字符。返回 canonical lowercase host。
fn canonical_trusted_host(host: &str) -> Result<String, ArtifactFetchError> {
    fn reject(detail: String) -> Result<String, ArtifactFetchError> {
        Err(ArtifactFetchError {
            kind: ArtifactFetchErrorKind::UnsafeSource,
            detail,
        })
    }
    // URL host_str 对 IPv6 带方括号；先剥壳再按字面量判定。
    let bare = host
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(host);
    if bare.parse::<IpAddr>().is_ok() {
        return reject(format!("来源策略拒绝 IP 字面量主机：{host}"));
    }
    let canonical = host.to_ascii_lowercase();
    if canonical.is_empty() {
        return reject("来源策略拒绝空主机".to_owned());
    }
    if canonical == "localhost" || canonical.ends_with(".localhost") {
        return reject(format!("来源策略拒绝 localhost 主机：{host}"));
    }
    if !canonical.contains('.') {
        return reject(format!("来源策略拒绝单标签主机：{host}"));
    }
    if canonical.ends_with('.') || canonical.contains("..") || canonical.contains('@') {
        return reject(format!("来源策略拒绝形态异常的主机：{host}"));
    }
    if !canonical
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '.'))
    {
        return reject(format!(
            "来源策略拒绝含控制 / 空白 / 非 ASCII 字符的主机：{host}"
        ));
    }
    Ok(canonical)
}

/// 解析生产资产 URL：MUST 为 https、无 userinfo，host 过 [`canonical_trusted_host`]。
fn parse_trusted_https_url(url: &str) -> Result<reqwest::Url, ArtifactFetchError> {
    let parsed = reqwest::Url::parse(url).map_err(|error| ArtifactFetchError {
        kind: ArtifactFetchErrorKind::UnsafeSource,
        detail: format!("manifest 资产 URL 非法：{error}"),
    })?;
    validate_request_target(&parsed, false)?;
    if let Some(host) = parsed.host_str() {
        canonical_trusted_host(host)?;
    } else {
        return Err(ArtifactFetchError {
            kind: ArtifactFetchErrorKind::UnsafeSource,
            detail: format!("manifest 资产 URL 缺少主机：{url}"),
        });
    }
    Ok(parsed)
}

/// 请求目标的基础校验：https-only（测试形态额外接受 loopback 明文 HTTP）
/// 且 **NEVER** 接受带 userinfo（`user@host`）的地址。
fn validate_request_target(
    url: &reqwest::Url,
    allow_loopback_http_for_tests: bool,
) -> Result<(), ArtifactFetchError> {
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ArtifactFetchError {
            kind: ArtifactFetchErrorKind::UnsafeSource,
            detail: format!("拒绝带 userinfo 凭证的来源地址：{url}"),
        });
    }
    match url.scheme() {
        "https" => Ok(()),
        "http" if allow_loopback_http_for_tests && is_loopback_host(url) => Ok(()),
        scheme => Err(ArtifactFetchError {
            kind: ArtifactFetchErrorKind::UnsafeSource,
            detail: format!("仅允许 https 来源，拒绝 {scheme} 地址：{url}"),
        }),
    }
}

/// loopback 主机判定（仅测试形态的明文 HTTP 放行条件）。
fn is_loopback_host(url: &reqwest::Url) -> bool {
    matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"))
}

/// HTTP 工件抓取 adapter：构造器注入 User-Agent、连接/单次读取空闲超时与
/// 固定 manifest 来源策略；生产形态仅接受 https 且只下载 manifest 声明的资产。
#[derive(Debug)]
pub struct HttpArtifactFetcher {
    /// 构造器注入的 reqwest client（重定向关闭，逐跳手动校验）。
    client: reqwest::Client,
    /// 固定 manifest 派生的可信来源策略（初始 path+URL 完全匹配 + 重定向 host 白名单）。
    source_policy: SourcePolicy,
    /// 仅 `cfg(test)` 构造可达的开关：允许 loopback 明文 HTTP（本地最小 server 夹具）；
    /// 生产构造器恒为 `false`，https-only 语义不受影响。
    allow_loopback_http_for_tests: bool,
}

impl HttpArtifactFetcher {
    /// 生产构造：注入 User-Agent、连接 / 单次读取空闲超时与**固定 manifest 来源策略**。
    ///
    /// - `connect_read_timeout`：同时作用于 `connect_timeout` 与 `read_timeout`
    ///   （连接超时 + 单次读取空闲超时），**不是**请求总预算 —— 775MB 资产的
    ///   下载总时长不受该值限制。
    /// - `manifest`：下载的信任根；初始请求 path+URL 必须与其 assets 完全匹配。
    /// - `allowed_redirect_hosts`：额外允许的重定向 host（CDN 兼容，默认只允许
    ///   manifest URL hosts）；每个 host 同样过生产 host 策略，非法 host
    ///   （localhost / IP literal / 单标签 / 控制空白等）在**构造期**即拒绝。
    pub fn new(
        user_agent: &str,
        connect_read_timeout: Duration,
        manifest: &ModelManifest,
        allowed_redirect_hosts: Vec<String>,
    ) -> Result<Self, ArtifactFetchError> {
        let source_policy = SourcePolicy::from_manifest(manifest, allowed_redirect_hosts)?;
        Self::build_client(user_agent, connect_read_timeout, true).map(|client| Self {
            client,
            source_policy,
            allow_loopback_http_for_tests: false,
        })
    }

    /// 测试专用构造（`cfg(test)`）：接受 loopback 明文 HTTP，供本地最小 server
    /// 夹具使用；来源策略由测试显式注入的 (path, URL) 白名单提供（绕过生产
    /// host 策略以测试 wire，生产 source policy 有独立单测）。
    /// 与测试模块同一 `unix` 门控（夹具依赖 `std::os::unix::fs::symlink`）。
    #[cfg(all(test, unix))]
    pub(crate) fn for_local_http_tests(
        user_agent: &str,
        connect_read_timeout: Duration,
        allowed_assets: Vec<(String, String)>,
        allowed_redirect_hosts: Vec<String>,
    ) -> Result<Self, ArtifactFetchError> {
        let source_policy =
            SourcePolicy::for_local_http_tests(allowed_assets, allowed_redirect_hosts)?;
        Self::build_client(user_agent, connect_read_timeout, false).map(|client| Self {
            client,
            source_policy,
            allow_loopback_http_for_tests: true,
        })
    }

    /// 构建注入式 client：**连接超时 + 单次读取空闲超时**
    /// （`connect_timeout` + `read_timeout`，NEVER 是总预算，不限制 775MB
    /// 资产的完整下载时长）、重定向关闭（逐跳手动校验）、`no_proxy`
    /// （不读代理环境变量）、生产形态附加 `https_only` 双闸。
    fn build_client(
        user_agent: &str,
        connect_read_timeout: Duration,
        https_only: bool,
    ) -> Result<reqwest::Client, ArtifactFetchError> {
        let mut builder = reqwest::Client::builder()
            .user_agent(user_agent)
            .connect_timeout(connect_read_timeout)
            .read_timeout(connect_read_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy();
        if https_only {
            builder = builder.https_only(true);
        }
        builder.build().map_err(|error| ArtifactFetchError {
            kind: ArtifactFetchErrorKind::Network,
            detail: format!("HTTP 客户端构建失败：{error}"),
        })
    }

    /// 初始请求目标校验：https（测试形态 + loopback 明文）且无 userinfo。
    fn validate_initial_target(&self, url: &reqwest::Url) -> Result<(), ArtifactFetchError> {
        validate_request_target(url, self.allow_loopback_http_for_tests)
    }

    /// 重定向目标校验：除 https（测试形态 + loopback 明文）与无 userinfo 外，
    /// host 还必须命中允许集合；未允许 host typed [`ArtifactFetchErrorKind::UnsafeSource`]。
    fn validate_redirect_target(&self, url: &reqwest::Url) -> Result<(), ArtifactFetchError> {
        validate_request_target(url, self.allow_loopback_http_for_tests)?;
        if !self.source_policy.allows_redirect_host(url) {
            return Err(ArtifactFetchError {
                kind: ArtifactFetchErrorKind::UnsafeSource,
                detail: format!(
                    "重定向目标 host 不在允许列表（固定 manifest URL hosts + 注入 CDN hosts）：{url}"
                ),
            });
        }
        Ok(())
    }
}

#[async_trait]
impl ArtifactFetcherPort for HttpArtifactFetcher {
    async fn fetch_asset_into_staging(
        &self,
        staging_root: &Path,
        asset: &ModelAsset,
    ) -> Result<(), ArtifactFetchError> {
        // 目标只能由 staging 根 + 已校验 asset.path 推导（防御性复核相对路径形态）。
        validate_relative_asset_path(&asset.path)?;
        // 初始请求：先过 scheme / userinfo 闸门，再要求 path+URL 与固定 manifest
        // 派生的允许集合完全匹配；任一不符在发出任何请求之前即拒绝。
        let requested_url =
            reqwest::Url::parse(&asset.url).map_err(|error| ArtifactFetchError {
                kind: ArtifactFetchErrorKind::UnsafeSource,
                detail: format!("资产 {} 的 URL 非法：{error}", asset.path),
            })?;
        self.validate_initial_target(&requested_url)?;
        let mut current_url = self
            .source_policy
            .initial_request_url(&asset.path, requested_url)?;
        // 手动逐跳重定向：每一跳都过同一 scheme + host 校验，跳数有限，禁止降级。
        let mut redirect_hops = 0_usize;
        let response = loop {
            let response = self
                .client
                .get(current_url.clone())
                .send()
                .await
                .map_err(|error| ArtifactFetchError {
                    kind: ArtifactFetchErrorKind::Network,
                    detail: format!("请求 {} 失败：{error}", asset.path),
                })?;
            let status = response.status();
            if status.is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok());
                let Some(location) = location else {
                    return Err(ArtifactFetchError {
                        kind: ArtifactFetchErrorKind::HttpStatus,
                        detail: format!("重定向响应缺少 Location 头：{status}"),
                    });
                };
                let redirect_url =
                    current_url
                        .join(location)
                        .map_err(|error| ArtifactFetchError {
                            kind: ArtifactFetchErrorKind::UnsafeSource,
                            detail: format!("重定向目标非法：{error}"),
                        })?;
                self.validate_redirect_target(&redirect_url)?;
                redirect_hops += 1;
                if redirect_hops > MAX_REDIRECT_HOPS {
                    return Err(ArtifactFetchError {
                        kind: ArtifactFetchErrorKind::UnsafeSource,
                        detail: format!("重定向跳数超过上限 {MAX_REDIRECT_HOPS}"),
                    });
                }
                current_url = redirect_url;
                continue;
            }
            if !status.is_success() {
                return Err(ArtifactFetchError {
                    kind: ArtifactFetchErrorKind::HttpStatus,
                    detail: format!("服务端返回非成功状态：{status}"),
                });
            }
            break response;
        };
        self.write_streamed_asset(response, staging_root, asset)
            .await
    }
}

impl HttpArtifactFetcher {
    /// 校验 Content-Length → 准备目标（父目录安全创建 + create_new）→ 流式写入 → flush/sync/close。
    async fn write_streamed_asset(
        &self,
        response: reqwest::Response,
        staging_root: &Path,
        asset: &ModelAsset,
    ) -> Result<(), ArtifactFetchError> {
        // Content-Length 预检：有值且与 manifest 不符立即失败（尚未创建目标文件）。
        if let Some(declared_length) = response.content_length() {
            if declared_length != asset.byte_length {
                return Err(ArtifactFetchError {
                    kind: ArtifactFetchErrorKind::LengthMismatch,
                    detail: format!(
                        "Content-Length {declared_length} 与 manifest 字节数 {} 不符",
                        asset.byte_length
                    ),
                });
            }
        }
        let destination = prepare_destination(staging_root, &asset.path)?;
        let mut destination_file = open_destination_file(&destination)?;
        let mut response_stream = response.bytes_stream();
        let mut accumulated: u64 = 0;
        let mut stream_failure: Option<ArtifactFetchError> = None;
        while let Some(chunk_result) = response_stream.next().await {
            let chunk = match chunk_result {
                Ok(chunk) => chunk,
                Err(error) => {
                    stream_failure = Some(ArtifactFetchError {
                        kind: ArtifactFetchErrorKind::Network,
                        detail: format!("响应流读取失败：{error}"),
                    });
                    break;
                }
            };
            accumulated += chunk.len() as u64;
            if accumulated > asset.byte_length {
                stream_failure = Some(ArtifactFetchError {
                    kind: ArtifactFetchErrorKind::LengthMismatch,
                    detail: format!("响应流累计字节超过 manifest 期望 {}", asset.byte_length),
                });
                break;
            }
            if let Err(error) =
                tokio::io::AsyncWriteExt::write_all(&mut destination_file, &chunk).await
            {
                stream_failure = Some(ArtifactFetchError {
                    kind: ArtifactFetchErrorKind::DestinationRejected,
                    detail: format!("目标文件写入失败：{error}"),
                });
                break;
            }
        }
        if stream_failure.is_none() && accumulated != asset.byte_length {
            stream_failure = Some(ArtifactFetchError {
                kind: ArtifactFetchErrorKind::LengthMismatch,
                detail: format!(
                    "响应结束时字节数不足：期望 {}，实际 {accumulated}",
                    asset.byte_length
                ),
            });
        }
        if stream_failure.is_none() {
            // 成功路径：最终 flush + sync_all，显式关闭后才返回。
            if let Err(error) = tokio::io::AsyncWriteExt::flush(&mut destination_file).await {
                stream_failure = Some(ArtifactFetchError {
                    kind: ArtifactFetchErrorKind::DestinationRejected,
                    detail: format!("目标文件 flush 失败：{error}"),
                });
            } else if let Err(error) = destination_file.sync_all().await {
                stream_failure = Some(ArtifactFetchError {
                    kind: ArtifactFetchErrorKind::DestinationRejected,
                    detail: format!("目标文件 sync 失败：{error}"),
                });
            }
        }
        // 显式关闭：任何返回路径前先 drop 文件句柄。
        drop(destination_file);
        match stream_failure {
            Some(failure) => {
                // 失败：尽力删除半成品目标文件；删除失败仅记录 LOG_TARGET warn
                //（不掩盖原始抓取失败）。其父目录与整个 staging 由安装端
                // `discard_staging` 整体清理（见 `ArtifactFetcherPort` 端口契约），
                // application NEVER 逐路径删除。
                if let Err(cleanup_error) = tokio::fs::remove_file(&destination).await {
                    if cleanup_error.kind() != std::io::ErrorKind::NotFound {
                        log::warn!(
                            target: crate::LOG_TARGET,
                            "model_fetch_partial_cleanup_failed path={} error={cleanup_error}",
                            destination.display()
                        );
                    }
                }
                Err(failure)
            }
            None => Ok(()),
        }
    }
}
