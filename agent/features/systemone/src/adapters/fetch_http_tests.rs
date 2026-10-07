//! `HttpArtifactFetcher` L2 测试：本地最小 HTTP server（Tokio TcpListener）
//! 覆盖流式写入、状态/长度校验、重定向闸门与目标路径安全；NEVER 访问公网、不用 sleep
//! （server 与 client 同一 runtime，以 write + `yield_now` 推进握手）。
//!
//! 来源策略两层覆盖：
//! - **wire 层**（loopback 明文 HTTP 夹具，仅 `cfg(test)` 构造可达）：测试以
//!   `(path, URL)` 白名单驱动，验证「不在白名单零请求」「重定向未允许 host
//!   只发一次初始请求」等请求期行为；
//! - **生产 source policy 层**（纯单测，零网络）：localhost / IP literal /
//!   单标签 host 在**策略构造期**即拒绝，同 host https 重定向放行、
//!   未允许 host 与 http 降级拒绝。

use super::*;

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

/// 测试用 engine_revision（canonical segment）。
const TEST_ENGINE_REVISION: &str = "e83f5c1a9d2b4f6a7c0e1d2b3a4f5c6d7e8f9a0b";

/// 构造测试用 fetcher：来源策略由给定资产精确派生（path+URL 完全匹配 +
/// 该 URL 的 host 允许重定向），允许 loopback 明文 HTTP（仅 `cfg(test)` 构造可达）。
fn local_fetcher_for(asset: &ModelAsset) -> HttpArtifactFetcher {
    HttpArtifactFetcher::for_local_http_tests(
        "systemone-fetch-tests/1.0",
        Duration::from_secs(10),
        vec![(asset.path.clone(), asset.url.clone())],
        Vec::new(),
    )
    .expect("测试 fetcher 构造")
}

/// 按 payload 构造契约资产（长度与 SHA-256 来自真实字节）。
fn test_asset(path: &str, url: &str, payload: &[u8]) -> ModelAsset {
    ModelAsset {
        path: path.to_owned(),
        url: url.to_owned(),
        byte_length: payload.len() as u64,
        sha256: utils::sha256_hex(payload),
    }
}

/// 构造生产形态契约 manifest：`model_url` 替换 model.gguf 资产 URL
/// （host 可控，用于生产 source policy 单测；`HttpArtifactFetcher::new`
/// 只从 assets 提取来源策略）。
fn production_manifest(model_url: &str) -> ModelManifest {
    let sha256 = "0123456789abcdef".repeat(4);
    ModelManifest {
        schema_version: 1,
        engine_revision: TEST_ENGINE_REVISION.to_owned(),
        hidden_size: 1024,
        pointer_dimension: 256,
        temperature: 0.07,
        supported_platforms: vec![crate::domain::required_platform().to_owned()],
        assets: vec![
            ModelAsset {
                path: "model.gguf".to_owned(),
                url: model_url.to_owned(),
                byte_length: 4096,
                sha256: sha256.clone(),
            },
            ModelAsset {
                path: "pointer_head.safetensors".to_owned(),
                url: "https://models.example.com/pointer_head.safetensors".to_owned(),
                byte_length: 2048,
                sha256: sha256.clone(),
            },
            ModelAsset {
                path: "tokenizer/merges.txt".to_owned(),
                url: "https://models.example.com/tokenizer/merges.txt".to_owned(),
                byte_length: 1024,
                sha256,
            },
        ],
    }
}

/// 脚本化响应。
#[derive(Debug, Clone)]
struct MockResponse {
    /// 状态行（如 `200 OK`）。
    status_line: String,
    /// 附加响应头。
    headers: Vec<(String, String)>,
    /// 声明的 Content-Length；`None` = 不带该头（EOF 定界）。
    content_length: Option<u64>,
    /// 响应体。
    body: Vec<u8>,
    /// body 分片写入数（每片后 `yield_now`，促使 client 增量消费）。
    body_fragments: usize,
}

impl MockResponse {
    /// 200 响应（声明与 body 等长的 Content-Length）。
    fn ok(body: Vec<u8>) -> Self {
        Self {
            status_line: "200 OK".to_owned(),
            headers: Vec::new(),
            content_length: Some(body.len() as u64),
            body,
            body_fragments: 1,
        }
    }

    /// 非 2xx 响应。
    fn error_status(status: u16, reason: &str) -> Self {
        Self {
            status_line: format!("{status} {reason}"),
            headers: Vec::new(),
            content_length: Some(10),
            body: b"error body".to_vec(),
            body_fragments: 1,
        }
    }

    /// 302 重定向响应。
    fn redirect(location: &str) -> Self {
        Self {
            status_line: "302 Found".to_owned(),
            headers: vec![("location".to_owned(), location.to_owned())],
            content_length: Some(0),
            body: Vec::new(),
            body_fragments: 1,
        }
    }

    /// 响应头块（不含 body）。
    fn header_block(&self) -> Vec<u8> {
        let mut block = format!("HTTP/1.1 {}\r\n", self.status_line).into_bytes();
        if let Some(content_length) = self.content_length {
            block.extend_from_slice(format!("content-length: {content_length}\r\n").as_bytes());
        }
        for (name, value) in &self.headers {
            block.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
        }
        block.extend_from_slice(b"connection: close\r\n\r\n");
        block
    }
}

/// server 记录的一次请求（请求行 + User-Agent）。
#[derive(Debug, Clone)]
struct RecordedRequest {
    target: String,
    user_agent: Option<String>,
}

/// 最小脚本化 HTTP server：按序响应（末项重复），记录请求。
struct MockArtifactServer {
    base_url: String,
    requests: mpsc::UnboundedReceiver<RecordedRequest>,
}

/// 启动本地脚本 server（`127.0.0.1:0` 随机端口）。
async fn start_mock_server(script: Vec<MockResponse>) -> MockArtifactServer {
    assert!(!script.is_empty(), "响应脚本不得为空");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("绑定端口");
    let address = listener.local_addr().expect("本地地址");
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut step_index = 0_usize;
        while let Ok((mut socket, _)) = listener.accept().await {
            // 读到请求头结束（GET 无 body）。
            let mut request_bytes = Vec::new();
            let mut buffer = vec![0_u8; 4096];
            loop {
                let read = socket.read(&mut buffer).await.unwrap_or(0);
                if read == 0 {
                    break;
                }
                request_bytes.extend_from_slice(&buffer[..read]);
                if request_bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            if request_bytes.is_empty() {
                continue;
            }
            let request_text = String::from_utf8_lossy(&request_bytes).to_string();
            let target = request_text.lines().next().unwrap_or_default().to_owned();
            let user_agent = request_text.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.trim()
                    .eq_ignore_ascii_case("user-agent")
                    .then(|| value.trim().to_owned())
            });
            let _ = sender.send(RecordedRequest { target, user_agent });
            let response = script
                .get(step_index)
                .or_else(|| script.last())
                .expect("脚本非空");
            step_index += 1;
            let _ = socket.write_all(&response.header_block()).await;
            tokio::task::yield_now().await;
            let fragment_size = response.body.len().div_ceil(response.body_fragments);
            for fragment in response.body.chunks(fragment_size.max(1)) {
                let _ = socket.write_all(fragment).await;
                let _ = socket.flush().await;
                tokio::task::yield_now().await;
            }
            let _ = socket.shutdown().await;
        }
    });
    MockArtifactServer {
        base_url: format!("http://{address}"),
        requests: receiver,
    }
}

/// 断言 server 恰好收到一次请求（fetch 已完成，接收端为非阻塞 drain）。
fn assert_single_request(server: &mut MockArtifactServer) -> RecordedRequest {
    let first = server.requests.try_recv().expect("应记录一次请求");
    assert!(
        server.requests.try_recv().is_err(),
        "MUST 只发出一次请求（无后续重定向/重试）"
    );
    first
}

/// 断言 server 零请求（拒绝发生在任何请求发出之前）。
fn assert_zero_requests(server: &mut MockArtifactServer) {
    assert!(
        server.requests.try_recv().is_err(),
        "MUST 零请求（拒绝 MUST 发生在发出任何请求之前）"
    );
}

#[tokio::test]
async fn streams_multi_fragment_response_into_staging_destination() {
    let body: Vec<u8> = (0..10_000).map(|index| (index % 251) as u8).collect();
    let mut response = MockResponse::ok(body.clone());
    response.body_fragments = 4;
    let mut server = start_mock_server(vec![response]).await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    let asset = test_asset(
        "model.gguf",
        &format!("{}/model.gguf", server.base_url),
        &body,
    );
    let fetcher = local_fetcher_for(&asset);

    fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect("多分片响应应流式写入成功");

    let written = std::fs::read(staging_root.path().join("model.gguf")).expect("目标文件可读");
    assert_eq!(written, body, "目标内容 MUST 等于响应体");
    let request = server.requests.try_recv().expect("应记录一次请求");
    assert!(
        request.target.contains("/model.gguf"),
        "请求行：{}",
        request.target
    );
    assert_eq!(
        request.user_agent.as_deref(),
        Some("systemone-fetch-tests/1.0"),
        "User-Agent MUST 来自构造器注入"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn destination_file_is_created_owner_only_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let body = b"owner-only-bytes".to_vec();
    let server = start_mock_server(vec![MockResponse::ok(body.clone())]).await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    let asset = test_asset(
        "model.gguf",
        &format!("{}/model.gguf", server.base_url),
        &body,
    );
    let fetcher = local_fetcher_for(&asset);

    fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect("写入成功");

    let mode = std::fs::metadata(staging_root.path().join("model.gguf"))
        .expect("目标文件元数据")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "目标文件 MUST 以 mode 0600 创建");
}

#[tokio::test]
async fn creates_missing_tokenizer_parent_directory_for_relative_destination() {
    let body = b"tokenizer-merges-bytes".to_vec();
    let server = start_mock_server(vec![MockResponse::ok(body.clone())]).await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    let asset = test_asset(
        "tokenizer/merges.txt",
        &format!("{}/merges.txt", server.base_url),
        &body,
    );
    let fetcher = local_fetcher_for(&asset);

    fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect("应安全创建父目录并写入");

    let written =
        std::fs::read(staging_root.path().join("tokenizer/merges.txt")).expect("目标文件可读");
    assert_eq!(written, body);
    assert!(
        !staging_root.path().join("tokenizer").is_symlink(),
        "父目录 MUST 由 fetcher 创建为真实目录"
    );
}

#[tokio::test]
async fn rejects_non_success_status_with_typed_error_and_no_destination() {
    let server = start_mock_server(vec![MockResponse::error_status(404, "Not Found")]).await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    let asset = test_asset(
        "model.gguf",
        &format!("{}/model.gguf", server.base_url),
        b"irrelevant",
    );
    let fetcher = local_fetcher_for(&asset);

    let error = fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect_err("非 2xx MUST typed 失败");

    assert_eq!(error.kind, ArtifactFetchErrorKind::HttpStatus);
    assert!(
        error.detail.contains("404"),
        "detail 应含状态码：{}",
        error.detail
    );
    assert!(
        !staging_root.path().join("model.gguf").exists(),
        "失败 MUST 不出现目标文件"
    );
}

#[tokio::test]
async fn rejects_content_length_deviating_from_manifest_before_creating_destination() {
    let mut response = MockResponse::ok(vec![0_u8; 8]);
    response.content_length = Some(999);
    let server = start_mock_server(vec![response]).await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    let payload = [0_u8; 8];
    let asset = test_asset(
        "model.gguf",
        &format!("{}/model.gguf", server.base_url),
        &payload,
    );
    let fetcher = local_fetcher_for(&asset);

    let error = fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect_err("Content-Length 与 manifest 不符 MUST 立即失败");

    assert_eq!(error.kind, ArtifactFetchErrorKind::LengthMismatch);
    assert!(
        error.detail.contains("Content-Length"),
        "detail：{}",
        error.detail
    );
    assert!(
        !staging_root.path().join("model.gguf").exists(),
        "长度预检失败 MUST 在建目标文件之前返回"
    );
}

#[tokio::test]
async fn fails_when_stream_ends_short_of_manifest_byte_length() {
    // 不声明 Content-Length（EOF 定界），body 短于 manifest 字节数。
    let mut response = MockResponse::ok(vec![0_u8; 4]);
    response.content_length = None;
    let server = start_mock_server(vec![response]).await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    let payload = [0_u8; 8];
    let asset = test_asset(
        "model.gguf",
        &format!("{}/model.gguf", server.base_url),
        &payload,
    );
    let fetcher = local_fetcher_for(&asset);

    let error = fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect_err("流提前结束 MUST 失败");

    assert_eq!(error.kind, ArtifactFetchErrorKind::LengthMismatch);
    assert!(
        !staging_root.path().join("model.gguf").exists(),
        "失败 MUST 清理半成品文件"
    );
}

#[tokio::test]
async fn fails_as_soon_as_stream_exceeds_manifest_byte_length() {
    // 不声明 Content-Length，body 长于 manifest 字节数：累计越界立即失败。
    let mut response = MockResponse::ok(vec![0_u8; 12]);
    response.content_length = None;
    let server = start_mock_server(vec![response]).await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    let payload = [0_u8; 8];
    let asset = test_asset(
        "model.gguf",
        &format!("{}/model.gguf", server.base_url),
        &payload,
    );
    let fetcher = local_fetcher_for(&asset);

    let error = fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect_err("流累计越界 MUST 失败");

    assert_eq!(error.kind, ArtifactFetchErrorKind::LengthMismatch);
    assert!(
        !staging_root.path().join("model.gguf").exists(),
        "失败 MUST 清理半成品文件"
    );
}

#[tokio::test]
async fn asset_url_absent_from_fixed_manifest_is_rejected_with_zero_requests() {
    // 来源策略只允许 `allowed.bin`；同一 path 换 URL 即不在固定 manifest 内。
    let mut server = start_mock_server(vec![MockResponse::ok(b"data".to_vec())]).await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    let allowed = test_asset(
        "model.gguf",
        &format!("{}/allowed.bin", server.base_url),
        b"data",
    );
    let fetcher = local_fetcher_for(&allowed);
    let disallowed = test_asset(
        "model.gguf",
        &format!("{}/elsewhere.bin", server.base_url),
        b"data",
    );

    let error = fetcher
        .fetch_asset_into_staging(staging_root.path(), &disallowed)
        .await
        .expect_err("不在固定 manifest 的初始 URL MUST 拒绝");

    assert_eq!(error.kind, ArtifactFetchErrorKind::UnsafeSource);
    assert!(
        error.detail.contains("允许列表"),
        "detail 应说明不在允许列表：{}",
        error.detail
    );
    assert_zero_requests(&mut server);
    assert!(
        !staging_root.path().join("model.gguf").exists(),
        "拒绝 MUST 不出现目标文件"
    );
}

#[tokio::test]
async fn https_redirect_to_host_outside_allowlist_is_rejected_after_single_request() {
    // 初始 URL 合法（同 host loopback），重定向到未允许 host 的 https 地址。
    let mut server = start_mock_server(vec![MockResponse::redirect(
        "https://cdn.untrusted.example/artifact.bin",
    )])
    .await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    let asset = test_asset(
        "model.gguf",
        &format!("{}/artifact.bin", server.base_url),
        b"irrelevant",
    );
    let fetcher = local_fetcher_for(&asset);

    let error = fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect_err("重定向到未允许 host MUST 拒绝");

    assert_eq!(error.kind, ArtifactFetchErrorKind::UnsafeSource);
    assert!(
        error.detail.contains("重定向"),
        "detail 应说明重定向 host 被拒：{}",
        error.detail
    );
    let request = assert_single_request(&mut server);
    assert!(request.target.contains("/artifact.bin"));
    assert!(
        !staging_root.path().join("model.gguf").exists(),
        "拒绝 MUST 不出现目标文件"
    );
}

#[tokio::test]
async fn rejects_redirect_downgrade_to_plain_http_without_following() {
    let mut server = start_mock_server(vec![MockResponse::redirect(
        "http://example.invalid/artifact.bin",
    )])
    .await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    let asset = test_asset(
        "model.gguf",
        &format!("{}/artifact.bin", server.base_url),
        b"irrelevant",
    );
    let fetcher = local_fetcher_for(&asset);

    let error = fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect_err("降级到非 https/loopback 的重定向 MUST 拒绝");

    assert_eq!(error.kind, ArtifactFetchErrorKind::UnsafeSource);
    assert!(error.detail.contains("https"), "detail：{}", error.detail);
    let request = assert_single_request(&mut server);
    assert!(request.target.contains("/artifact.bin"));
    assert!(
        !staging_root.path().join("model.gguf").exists(),
        "拒绝重定向 MUST 不出现目标文件"
    );
}

#[tokio::test]
async fn follows_validated_relative_redirect_and_succeeds() {
    let body = b"redirected-content".to_vec();
    let mut server = start_mock_server(vec![
        MockResponse::redirect("/final.bin"),
        MockResponse::ok(body.clone()),
    ])
    .await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    let asset = test_asset(
        "model.gguf",
        &format!("{}/artifact.bin", server.base_url),
        &body,
    );
    let fetcher = local_fetcher_for(&asset);

    fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect("逐跳校验通过的重定向应跟随并成功");

    let written = std::fs::read(staging_root.path().join("model.gguf")).expect("目标文件可读");
    assert_eq!(written, body);
    let request_count = std::iter::from_fn(|| server.requests.try_recv().ok()).count();
    assert_eq!(request_count, 2, "MUST 恰好请求两次（重定向一次）");
}

#[tokio::test]
async fn fails_when_redirect_hops_exceed_limit() {
    // 末项重复：server 永远返回 302。
    let mut server = start_mock_server(vec![MockResponse::redirect("/again.bin")]).await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    let asset = test_asset(
        "model.gguf",
        &format!("{}/start.bin", server.base_url),
        b"irrelevant",
    );
    let fetcher = local_fetcher_for(&asset);

    let error = fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect_err("跳数超限 MUST 失败");

    assert_eq!(error.kind, ArtifactFetchErrorKind::UnsafeSource);
    assert!(error.detail.contains("跳数"), "detail：{}", error.detail);
    let request_count = std::iter::from_fn(|| server.requests.try_recv().ok()).count();
    assert_eq!(
        request_count,
        MAX_REDIRECT_HOPS + 1,
        "MUST 在第 {MAX_REDIRECT_HOPS} 跳后停止（初始请求 + {} 次跟随）",
        MAX_REDIRECT_HOPS
    );
}

#[tokio::test]
async fn refuses_existing_destination_without_overwriting() {
    let server = start_mock_server(vec![MockResponse::ok(b"new-bytes".to_vec())]).await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    std::fs::write(staging_root.path().join("model.gguf"), b"original").expect("既有文件写入");
    let asset = test_asset(
        "model.gguf",
        &format!("{}/model.gguf", server.base_url),
        b"new-bytes",
    );
    let fetcher = local_fetcher_for(&asset);

    let error = fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect_err("已存在的目标 MUST 拒绝写入");

    assert_eq!(error.kind, ArtifactFetchErrorKind::DestinationRejected);
    assert!(error.detail.contains("已存在"), "detail：{}", error.detail);
    assert_eq!(
        std::fs::read(staging_root.path().join("model.gguf")).expect("既有文件可读"),
        b"original",
        "既有文件 MUST NOT 被覆盖"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn refuses_parent_directory_symlink_and_leaves_target_untouched() {
    let server = start_mock_server(vec![MockResponse::ok(b"payload".to_vec())]).await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    let external_dir = tempfile::tempdir().expect("外部目录");
    std::os::unix::fs::symlink(external_dir.path(), staging_root.path().join("tokenizer"))
        .expect("创建父目录符号链接");
    let asset = test_asset(
        "tokenizer/merges.txt",
        &format!("{}/merges.txt", server.base_url),
        b"payload",
    );
    let fetcher = local_fetcher_for(&asset);

    let error = fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect_err("父目录为符号链接 MUST 拒绝");

    assert_eq!(error.kind, ArtifactFetchErrorKind::DestinationRejected);
    assert!(
        std::fs::read_dir(external_dir.path())
            .expect("外部目录可读")
            .next()
            .is_none(),
        "链接目标 MUST 不被写入"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn refuses_last_segment_symlink_destination_and_leaves_target_untouched() {
    let server = start_mock_server(vec![MockResponse::ok(b"payload".to_vec())]).await;
    let staging_root = tempfile::tempdir().expect("临时目录");
    let external_dir = tempfile::tempdir().expect("外部目录");
    let external_file = external_dir.path().join("precious.txt");
    std::fs::write(&external_file, b"keep").expect("外部文件写入");
    std::os::unix::fs::symlink(&external_file, staging_root.path().join("model.gguf"))
        .expect("创建末段符号链接");
    let asset = test_asset(
        "model.gguf",
        &format!("{}/model.gguf", server.base_url),
        b"payload",
    );
    let fetcher = local_fetcher_for(&asset);

    let error = fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect_err("末段符号链接 MUST 拒绝");

    assert_eq!(error.kind, ArtifactFetchErrorKind::DestinationRejected);
    assert_eq!(
        std::fs::read(&external_file).expect("外部文件可读"),
        b"keep",
        "链接目标 MUST 不被覆盖"
    );
}

#[tokio::test]
async fn production_constructor_rejects_plain_http_source_before_any_request() {
    // 生产构造器：仅允许 https（本用例不启动任何 server，证明请求发出前即拒绝）。
    let production_fetcher = HttpArtifactFetcher::new(
        "systemone/1.0",
        Duration::from_secs(5),
        &production_manifest("https://models.example.com/model.gguf"),
        Vec::new(),
    )
    .expect("合法来源策略应通过构造");
    let staging_root = tempfile::tempdir().expect("临时目录");
    let asset = test_asset(
        "model.gguf",
        "http://example.invalid/model.gguf",
        b"irrelevant",
    );

    let error = production_fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect_err("非 https 来源 MUST 在发请求前拒绝");

    assert_eq!(error.kind, ArtifactFetchErrorKind::UnsafeSource);
    assert!(error.detail.contains("https"), "detail：{}", error.detail);
    assert!(!staging_root.path().join("model.gguf").exists());
}

#[tokio::test]
async fn production_fetcher_rejects_asset_url_outside_fixed_manifest_before_any_request() {
    // 生产构造器 + 固定 manifest；请求 URL 不在 manifest assets 内 → 发请求前拒绝。
    let production_fetcher = HttpArtifactFetcher::new(
        "systemone/1.0",
        Duration::from_secs(5),
        &production_manifest("https://models.example.com/model.gguf"),
        Vec::new(),
    )
    .expect("合法来源策略应通过构造");
    let staging_root = tempfile::tempdir().expect("临时目录");
    let asset = test_asset(
        "model.gguf",
        "https://elsewhere.example.net/model.gguf",
        b"irrelevant",
    );

    let error = production_fetcher
        .fetch_asset_into_staging(staging_root.path(), &asset)
        .await
        .expect_err("不在固定 manifest 的 URL MUST 在发请求前拒绝");

    assert_eq!(error.kind, ArtifactFetchErrorKind::UnsafeSource);
    assert!(
        error.detail.contains("允许列表"),
        "detail 应说明不在允许列表：{}",
        error.detail
    );
    assert!(!staging_root.path().join("model.gguf").exists());
}

#[test]
fn production_source_policy_rejects_localhost_ip_and_single_label_hosts_at_construction() {
    // host 策略在策略构造期执行：这些 host 不会进入任何允许集合。
    for hostile_url in [
        "https://localhost/model.gguf",
        "https://sub.localhost/model.gguf",
        "https://127.0.0.1/model.gguf",
        "https://10.0.0.8/model.gguf",
        "https://[::1]/model.gguf",
        "https://intranet/model.gguf",
    ] {
        let error = HttpArtifactFetcher::new(
            "systemone/1.0",
            Duration::from_secs(5),
            &production_manifest(hostile_url),
            Vec::new(),
        )
        .expect_err("hostile host MUST 在策略构造期被拒绝");
        assert_eq!(
            error.kind,
            ArtifactFetchErrorKind::UnsafeSource,
            "应为来源策略拒绝（{hostile_url}）：{error}"
        );
    }

    // 控制 / 空白字符的主机在 URL 解析或 host 策略两道闸门之一被拒。
    let whitespace_error = HttpArtifactFetcher::new(
        "systemone/1.0",
        Duration::from_secs(5),
        &production_manifest("https://models.example.com/model.gguf"),
        vec!["bad host.example".to_owned()],
    )
    .expect_err("含空白的重定向 host MUST 在构造期被拒绝");
    assert_eq!(whitespace_error.kind, ArtifactFetchErrorKind::UnsafeSource);

    // 注入的重定向 host（CDN 兼容入口）同样过生产 host 策略。
    for hostile_host in ["localhost", "169.254.169.254", "singlelabel", "::1"] {
        let error = HttpArtifactFetcher::new(
            "systemone/1.0",
            Duration::from_secs(5),
            &production_manifest("https://models.example.com/model.gguf"),
            vec![hostile_host.to_owned()],
        )
        .expect_err("hostile 重定向 host MUST 在构造期被拒绝");
        assert_eq!(
            error.kind,
            ArtifactFetchErrorKind::UnsafeSource,
            "应为来源策略拒绝（{hostile_host}）：{error}"
        );
    }
}

#[test]
fn production_source_policy_permits_same_host_https_redirect_but_rejects_unknown_hosts() {
    // 同 host https 重定向放行（含构造器注入的 CDN host）；
    // 未允许 host / http 降级 / userinfo 地址一律拒绝 —— 纯策略单测，零网络。
    let manifest = production_manifest("https://models.example.com/model.gguf");
    let fetcher = HttpArtifactFetcher::new(
        "systemone/1.0",
        Duration::from_secs(5),
        &manifest,
        vec!["cdn.example.net".to_owned()],
    )
    .expect("合法来源策略应通过构造");
    let policy = &fetcher.source_policy;

    let same_host =
        reqwest::Url::parse("https://models.example.com/cdn/model.gguf").expect("URL 解析");
    fetcher
        .validate_redirect_target(&same_host)
        .expect("允许同 host 的 https 重定向");
    assert!(policy.allows_redirect_host(&same_host));

    let injected_cdn = reqwest::Url::parse("https://cdn.example.net/model.gguf").expect("URL 解析");
    fetcher
        .validate_redirect_target(&injected_cdn)
        .expect("允许注入的 CDN host 重定向");

    for rejected in [
        "https://untrusted.example.org/model.gguf",
        "https://models.example.com.evil.example/model.gguf",
        "http://models.example.com/model.gguf",
        "https://user@models.example.com/model.gguf",
        "https://localhost/model.gguf",
        "https://127.0.0.1/model.gguf",
    ] {
        let url = reqwest::Url::parse(rejected).expect("URL 解析");
        let error = fetcher
            .validate_redirect_target(&url)
            .expect_err("重定向目标 MUST 被拒绝");
        assert_eq!(
            error.kind,
            ArtifactFetchErrorKind::UnsafeSource,
            "应为来源策略拒绝（{rejected}）：{error}"
        );
    }
}

#[test]
fn constructors_apply_connect_and_read_idle_timeouts_without_total_budget() {
    // 超时注入只构造（connect_timeout + read_timeout 空闲口径），
    // NEVER 是总预算 —— 不用 sleep，仅证明两个构造器可正常装配。
    let manifest = production_manifest("https://models.example.com/model.gguf");
    HttpArtifactFetcher::new(
        "systemone/1.0",
        Duration::from_secs(5),
        &manifest,
        Vec::new(),
    )
    .expect("生产构造器应可用注入的连接/读取空闲超时构造");
    HttpArtifactFetcher::for_local_http_tests(
        "systemone-fetch-tests/1.0",
        Duration::from_secs(5),
        vec![(
            "model.gguf".to_owned(),
            "http://127.0.0.1:9/model.gguf".to_owned(),
        )],
        Vec::new(),
    )
    .expect("测试构造器应可用注入的连接/读取空闲超时构造");
}
