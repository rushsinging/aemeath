//! Scoring daemon UDS accept loop：单 worker 串行处理 answer 请求。
//!
//! 设计依据：`docs/design/02-modules/systemone/04-scoring-daemon.md` §4–§5。

use std::io;
use std::sync::Arc;
use std::time::Instant;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;

use crate::adapters::daemon_instance::{prepare_socket_path, DaemonEndpoints, DaemonLockGuard};
use crate::domain::daemon_protocol::{
    decode_answer_request, encode_json_frame, try_decode_frame, AnswerResult, DaemonErrorBody,
    DaemonErrorKind, DaemonRequest, DaemonResponse, ProtocolError,
};
use crate::domain::UnavailableKind;
use crate::ports::ScoringPort;

/// 处理一帧请求并返回响应帧（含 length prefix）。
pub async fn handle_framed_request(
    port: &dyn ScoringPort,
    engine_revision: &str,
    buffer: &[u8],
) -> Result<Vec<u8>, ProtocolError> {
    let (request, _) = decode_answer_request(buffer)?;
    let response = match request {
        DaemonRequest::Answer { id, params, .. } => {
            let started = Instant::now();
            match port.answer(&params.state, &params.questions).await {
                Ok(answers) => DaemonResponse::ok(
                    id,
                    AnswerResult {
                        answers,
                        engine_revision: engine_revision.to_string(),
                        latency_ms: started.elapsed().as_millis() as u64,
                    },
                ),
                Err(unavailable) => DaemonResponse::error(
                    id,
                    DaemonErrorBody {
                        kind: map_unavailable_kind(unavailable.kind()),
                        message: unavailable.to_string(),
                    },
                ),
            }
        }
    };
    encode_json_frame(&response)
}

fn map_unavailable_kind(kind: UnavailableKind) -> DaemonErrorKind {
    match kind {
        UnavailableKind::Timeout => DaemonErrorKind::Timeout,
        UnavailableKind::Schema => DaemonErrorKind::Protocol,
        UnavailableKind::Disabled
        | UnavailableKind::Connect
        | UnavailableKind::Server
        | UnavailableKind::Client => DaemonErrorKind::Unavailable,
    }
}

/// 绑定 sock 并循环 accept；`shutdown` 触发后优雅退出（不再接新连接）。
pub async fn run_daemon_accept_loop(
    endpoints: DaemonEndpoints,
    lock: DaemonLockGuard,
    port: Arc<dyn ScoringPort>,
    engine_revision: String,
    mut shutdown: oneshot::Receiver<()>,
) -> io::Result<()> {
    prepare_socket_path(&endpoints.sock, &lock)?;
    let listener = UnixListener::bind(&endpoints.sock)?;
    // 持有 lock 直到循环结束，防止第二实例抢 sock。
    let _lock = lock;

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                break;
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                // 连接期内也响应 shutdown，避免阻塞在长连接 read。
                tokio::select! {
                    _ = &mut shutdown => {
                        break;
                    }
                    result = serve_connection(stream, port.as_ref(), &engine_revision) => {
                        if let Err(error) = result {
                            log::warn!(
                                target: crate::LOG_TARGET,
                                "scoring daemon 连接处理失败：{error}"
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

async fn serve_connection(
    mut stream: UnixStream,
    port: &dyn ScoringPort,
    engine_revision: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        // chunk 为原始 socket 字节缓冲，非 UTF-8 文本。
        let (received, _) = chunk.split_at(n); // allow unsafe_text_op
        buffer.extend_from_slice(received);
        while let Ok((_, consumed)) = try_decode_frame(&buffer) {
            let frame: Vec<u8> = buffer.drain(..consumed).collect();
            let response = handle_framed_request(port, engine_revision, &frame)
                .await
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
            stream.write_all(&response).await?;
            stream.flush().await?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "daemon_serve_tests.rs"]
mod tests;
