//! Scoring daemon UDS 客户端：实现 `ScoringPort`，供会话进程调用。
//!
//! 设计依据：`docs/design/02-modules/systemone/04-scoring-daemon.md` §3 / §5。

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::timeout;

use crate::domain::daemon_protocol::{
    decode_response, encode_json_frame, try_decode_frame, AnswerParams, DaemonErrorKind,
    DaemonRequest, DaemonResponse,
};
use crate::domain::{
    ScoringAnswer, ScoringQuestion, ScoringState, ScoringUnavailable, UnavailableKind,
};
use crate::ports::ScoringPort;

/// 本机 scoring daemon 的轻量客户端。
#[derive(Debug, Clone)]
pub struct DaemonScoringClient {
    sock: PathBuf,
    connect_timeout: Duration,
    infer_timeout: Duration,
}

impl DaemonScoringClient {
    pub fn new(sock: PathBuf, connect_timeout: Duration, infer_timeout: Duration) -> Self {
        Self {
            sock,
            connect_timeout,
            infer_timeout,
        }
    }

    pub fn sock_path(&self) -> &std::path::Path {
        &self.sock
    }
}

#[async_trait]
impl ScoringPort for DaemonScoringClient {
    async fn answer(
        &self,
        state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        let mut stream = timeout(self.connect_timeout, UnixStream::connect(&self.sock))
            .await
            .map_err(|_| {
                ScoringUnavailable::new(UnavailableKind::Timeout, "连接 scoring daemon 超时")
            })?
            .map_err(|error| {
                ScoringUnavailable::new(
                    UnavailableKind::Connect,
                    format!("连接 scoring daemon 失败：{error}"),
                )
            })?;

        let request = DaemonRequest::answer(
            None,
            AnswerParams {
                scenario: None,
                state: state.clone(),
                questions: questions.to_vec(),
            },
        );
        let frame = encode_json_frame(&request).map_err(|error| {
            ScoringUnavailable::new(
                UnavailableKind::Schema,
                format!("编码评分请求失败：{error}"),
            )
        })?;

        timeout(self.infer_timeout, async {
            stream.write_all(&frame).await?;
            stream.flush().await?;
            read_one_frame(&mut stream).await
        })
        .await
        .map_err(|_| ScoringUnavailable::new(UnavailableKind::Timeout, "scoring daemon 推理超时"))?
        .map_err(|error| {
            ScoringUnavailable::new(
                UnavailableKind::Connect,
                format!("读写 scoring daemon 失败：{error}"),
            )
        })
        .and_then(|response_frame| map_daemon_response(&response_frame))
    }
}

async fn read_one_frame(stream: &mut UnixStream) -> std::io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "scoring daemon 提前关闭连接",
            ));
        }
        // chunk 为原始 socket 字节缓冲，非 UTF-8 文本。
        let (received, _) = chunk.split_at(n); // allow unsafe_text_op
        buffer.extend_from_slice(received);
        match try_decode_frame(&buffer) {
            Ok((_, consumed)) => {
                let frame: Vec<u8> = buffer.drain(..consumed).collect();
                return Ok(frame);
            }
            Err(crate::domain::daemon_protocol::FrameError::Incomplete) => continue,
        }
    }
}

fn map_daemon_response(frame: &[u8]) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
    let (response, _) = decode_response(frame).map_err(|error| {
        ScoringUnavailable::new(
            UnavailableKind::Schema,
            format!("解码评分响应失败：{error}"),
        )
    })?;
    match response {
        DaemonResponse::Ok { result, .. } => Ok(result.answers),
        DaemonResponse::Err { error, .. } => Err(ScoringUnavailable::new(
            map_daemon_error_kind(error.kind),
            error.message,
        )),
    }
}

fn map_daemon_error_kind(kind: DaemonErrorKind) -> UnavailableKind {
    match kind {
        DaemonErrorKind::Timeout => UnavailableKind::Timeout,
        DaemonErrorKind::Protocol => UnavailableKind::Schema,
        DaemonErrorKind::Unavailable | DaemonErrorKind::Internal => UnavailableKind::Server,
    }
}

/// 惰性拉起：sock 可连则 Ok；否则 spawn 当前可执行文件的 `scoring serve` 并等待就绪。
///
/// 会话退出 **NEVER** 杀掉 daemon。构造外部进程时配对
/// [`utils::configure_std_noninteractive`]。
pub fn ensure_scoring_daemon_ready(
    sock: &std::path::Path,
    connect_timeout: Duration,
) -> Result<(), String> {
    if scoring_sock_connectable(sock) {
        return Ok(());
    }
    let executable = std::env::current_exe()
        .map_err(|error| format!("定位当前可执行文件失败，无法惰性拉起 scoring serve：{error}"))?;
    let mut command = std::process::Command::new(&executable);
    command
        .args(["scoring", "serve"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    utils::configure_std_noninteractive(&mut command)
        .map_err(|error| format!("配置 scoring serve 非交互隔离失败：{error}"))?;
    command
        .spawn()
        .map_err(|error| format!("惰性拉起 `scoring serve` 失败：{error}"))?;

    let started = std::time::Instant::now();
    while started.elapsed() < connect_timeout {
        if scoring_sock_connectable(sock) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(format!(
        "等待 scoring daemon sock 就绪超时（{}s）；可手动执行 `aemeath scoring serve`，         模型缺失时先 `aemeath systemone download`",
        connect_timeout.as_secs().max(1)
    ))
}

fn scoring_sock_connectable(sock: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::net::UnixStream::connect(sock).is_ok()
    }
    #[cfg(not(unix))]
    {
        let _ = sock;
        false
    }
}

#[cfg(test)]
#[path = "daemon_client_tests.rs"]
mod tests;
