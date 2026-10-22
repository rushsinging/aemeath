//! RSI daemon 控制客户端：连接 sock、发指令、读应答。

use crate::domain::daemon_protocol::{
    encode_control_frame, try_decode_frame, RsiControl, RsiControlAck,
};
use crate::domain::errors::HarnessError;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// 连接 daemon 并执行一次指令-应答往返。
pub async fn send_control(
    sock: &Path,
    control: &RsiControl,
) -> Result<RsiControlAck, HarnessError> {
    let mut stream = UnixStream::connect(sock).await.map_err(|error| {
        HarnessError::unavailable(
            crate::constants::ERROR_DOMAIN,
            format!("连接 rsi daemon 失败 {}: {error}", sock.display()),
        )
    })?;
    stream
        .write_all(&encode_control_frame(control))
        .await
        .map_err(|error| {
            HarnessError::unavailable(
                crate::constants::ERROR_DOMAIN,
                format!("发送控制指令失败：{error}"),
            )
        })?;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream.read(&mut chunk).await.map_err(|error| {
            HarnessError::unavailable(
                crate::constants::ERROR_DOMAIN,
                format!("读取控制应答失败：{error}"),
            )
        })?;
        if read == 0 {
            return Err(HarnessError::unavailable(
                crate::constants::ERROR_DOMAIN,
                "daemon 在应答前断开连接",
            ));
        }
        buffer.extend_from_slice(chunk.get(..read).expect("read 不超过 chunk 长度"));
        match try_decode_frame::<RsiControlAck>(&buffer) {
            Ok(Some((ack, _consumed))) => return Ok(ack),
            Ok(None) => continue,
            Err(error) => return Err(error),
        }
    }
}
