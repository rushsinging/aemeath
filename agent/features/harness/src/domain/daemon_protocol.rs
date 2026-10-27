//! RSI daemon 控制协议：length-prefix JSON 帧与控制指令编解码。
//!
//! 仅控制面（Trigger/Status/Stop）；数据面（材料、变更、经验）全走落盘文件，
//! 与主进程零数据耦合（见 `docs/design/02-modules/harness/03-rsi-daemon.md`）。

use crate::constants::ERROR_DOMAIN;
use crate::domain::errors::HarnessError;
use serde::{Deserialize, Serialize};

/// 控制指令（控制方 → daemon）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RsiControl {
    /// Manual 显式触发一轮改进周期。
    Trigger { request_id: String },
    /// 查询运行状态。
    Status,
    /// 显式停止（daemon 常驻，NEVER 空闲自动退出）。
    Stop,
}

/// daemon 运行状态（Status 应答载荷）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RsiRunState {
    /// 是否有改进周期正在执行。
    pub cycle_running: bool,
    /// 上一次周期的结果摘要（None = 尚未跑过）。
    pub last_cycle: Option<String>,
}

/// 控制应答（daemon → 控制方）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RsiControlAck {
    /// 周期已开始（结果异步，经日志与经验库可查）。
    Started {
        request_id: String,
    },
    Status {
        state: RsiRunState,
    },
    Stopped,
    /// 已有周期在跑，Manual 让位（串行化，NEVER 并行周期）。
    Busy {
        request_id: String,
    },
}

/// 序列化控制指令为完整帧（u32 大端长度前缀 + JSON）。
pub fn encode_control_frame(control: &RsiControl) -> Vec<u8> {
    encode_json_frame(&control)
}

/// 序列化应答为完整帧。
pub fn encode_ack_frame(ack: &RsiControlAck) -> Vec<u8> {
    encode_json_frame(&ack)
}

fn encode_json_frame<T: Serialize>(value: &T) -> Vec<u8> {
    let json = serde_json::to_vec(value).expect("控制协议序列化不可失败");
    let mut frame = Vec::with_capacity(4 + json.len());
    frame.extend_from_slice(&(json.len() as u32).to_be_bytes());
    frame.extend_from_slice(&json);
    frame
}

/// 尝试从缓冲解析一帧（值 + 帧总字节数）。数据不足时返回 Ok(None)。
pub fn try_decode_frame<T: for<'de> Deserialize<'de>>(
    buffer: &[u8],
) -> Result<Option<(T, usize)>, HarnessError> {
    if buffer.len() < 4 {
        return Ok(None);
    }
    let payload_len = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    if payload_len > crate::constants::CONTROL_FRAME_MAX_BYTES {
        return Err(HarnessError::invalid(
            ERROR_DOMAIN,
            format!(
                "控制帧超长：{payload_len} > {}",
                crate::constants::CONTROL_FRAME_MAX_BYTES
            ),
        ));
    }
    if buffer.len() < 4 + payload_len {
        return Ok(None);
    }
    let payload = buffer
        .get(4..4 + payload_len)
        .ok_or_else(|| HarnessError::invalid(ERROR_DOMAIN, "控制帧载荷切分越界"))?;
    let value = serde_json::from_slice(payload).map_err(|error| {
        HarnessError::invalid(ERROR_DOMAIN, format!("控制帧 JSON 解析失败：{error}"))
    })?;
    Ok(Some((value, 4 + payload_len)))
}
