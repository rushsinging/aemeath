//! 控制协议帧编解码契约测试。

use crate::domain::daemon_protocol::{
    encode_ack_frame, encode_control_frame, try_decode_frame, RsiControl, RsiControlAck,
    RsiRunState,
};

/// 指令/应答帧往返：编码后可完整解码回等价值。
#[test]
fn control_frames_round_trip() {
    let samples: Vec<RsiControl> = vec![
        RsiControl::Trigger {
            request_id: "req-1".to_string(),
        },
        RsiControl::Status,
        RsiControl::Stop,
    ];
    for control in &samples {
        let frame = encode_control_frame(control);
        let (decoded, consumed) = try_decode_frame::<RsiControl>(&frame)
            .expect("解码不应失败")
            .expect("完整帧必须可解析");
        assert_eq!(&decoded, control);
        assert_eq!(consumed, frame.len());
    }

    let ack = RsiControlAck::Status {
        state: RsiRunState {
            cycle_running: false,
            last_cycle: Some("采纳 2 / 作废 0".to_string()),
        },
    };
    let frame = encode_ack_frame(&ack);
    let (decoded, _) = try_decode_frame::<RsiControlAck>(&frame)
        .expect("解码不应失败")
        .expect("完整帧必须可解析");
    assert_eq!(decoded, ack);
}

/// 半帧返回 None（等待更多数据），NEVER 报错。
#[test]
fn partial_frame_waits_for_more_bytes() {
    let frame = encode_control_frame(&RsiControl::Stop);
    for cut in 1..frame.len() {
        let partial = &frame[..cut];
        let decoded = try_decode_frame::<RsiControl>(partial).expect("半帧 NEVER 报错");
        assert!(decoded.is_none(), "截断至 {cut} 字节应等待更多数据");
    }
}

/// 超长帧拒绝（协议破坏防护）。
#[test]
fn oversized_frame_rejected() {
    let mut buffer = Vec::new();
    buffer
        .extend_from_slice(&((crate::constants::CONTROL_FRAME_MAX_BYTES + 1) as u32).to_be_bytes());
    buffer.extend_from_slice(b"{}");
    let error = try_decode_frame::<RsiControl>(&buffer).expect_err("超长帧必须拒绝");
    assert!(error.message().contains("超长"));
}
