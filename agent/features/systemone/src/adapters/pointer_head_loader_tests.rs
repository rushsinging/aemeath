//! pointer_head.safetensors 加载：字节 → PointerHeadWeights（维度/端序/有限性 fail-closed）。

use crate::adapters::pointer_head_loader::{
    load_pointer_head, parse_pointer_head, PointerHeadLoadError,
};
use crate::domain::PointerHead;

/// 合成 safetensors：8 字节 header 长度 + JSON header + F32 LE 数据段。
fn synthetic_safetensors(tensors: &[(&str, Vec<f32>, Vec<u64>)]) -> Vec<u8> {
    let mut header_entries = Vec::new();
    let mut data_section = Vec::new();
    for (name, values, shape) in tensors {
        let begin = data_section.len();
        for value in values {
            data_section.extend_from_slice(&value.to_le_bytes());
        }
        let end = data_section.len();
        header_entries.push(format!(
            "\"{name}\":{{\"dtype\":\"F32\",\"shape\":{shape:?},\"data_offsets\":[{begin},{end}]}}"
        ));
    }
    let header = format!("{{{}}}", header_entries.join(","));
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(header.len() as u64).to_le_bytes());
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(&data_section);
    bytes
}

/// 2×2 恒等投影 + 零偏置的完整 head 字节。
fn identity_head_safetensors() -> Vec<u8> {
    synthetic_safetensors(&[
        ("q.weight", vec![1.0, 0.0, 0.0, 1.0], vec![2, 2]),
        ("q.bias", vec![0.0, 0.0], vec![2]),
        ("k.weight", vec![1.0, 0.0, 0.0, 1.0], vec![2, 2]),
        ("k.bias", vec![0.0, 0.0], vec![2]),
    ])
}

#[test]
fn parse_identity_head_scores_expected_softmax() {
    let head =
        parse_pointer_head(&identity_head_safetensors(), 2, 2, 1.0).expect("恒等 head 解析成功");
    // query = [1,0]；option0 = [1,0] → logit 1；option1 = [0,1] → logit 0；再除 sqrt(2)。
    let probabilities = head
        .score_options(&[1.0, 0.0], &[1.0, 0.0, 0.0, 1.0])
        .expect("概率计算成功");
    let scaled_logit = 1.0_f32 / 2.0_f32.sqrt();
    let expected_first = 1.0 / (1.0 + (-scaled_logit).exp());
    assert!(
        (probabilities[0] - expected_first).abs() < 1e-6,
        "实际 {probabilities:?}"
    );
    assert!((probabilities.iter().sum::<f32>() - 1.0).abs() < 1e-6);
}

#[test]
fn parse_reports_missing_tensor() {
    let bytes = synthetic_safetensors(&[
        ("q.weight", vec![1.0, 0.0, 0.0, 1.0], vec![2, 2]),
        ("q.bias", vec![0.0, 0.0], vec![2]),
        ("k.weight", vec![1.0, 0.0, 0.0, 1.0], vec![2, 2]),
    ]);
    let error = parse_pointer_head(&bytes, 2, 2, 1.0).expect_err("缺 k.bias 必须失败");
    match &error {
        PointerHeadLoadError::MissingTensor { tensor } => assert_eq!(*tensor, "k.bias"),
        other => panic!("错误类型不符：{other:?}"),
    }
    assert!(
        error.to_string().contains("缺少"),
        "错误消息为中文：{}",
        error
    );
}

#[test]
fn parse_reports_unsupported_dtype() {
    let header = "{\"q.weight\":{\"dtype\":\"F16\",\"shape\":[2,2],\"data_offsets\":[0,8]},\
                  \"q.bias\":{\"dtype\":\"F32\",\"shape\":[2],\"data_offsets\":[8,16]},\
                  \"k.weight\":{\"dtype\":\"F32\",\"shape\":[2,2],\"data_offsets\":[16,32]},\
                  \"k.bias\":{\"dtype\":\"F32\",\"shape\":[2],\"data_offsets\":[32,40]}}";
    let header = header.replace(' ', "");
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(header.len() as u64).to_le_bytes());
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(&[0_u8; 40]);
    let error = parse_pointer_head(&bytes, 2, 2, 1.0).expect_err("F16 dtype 必须失败");
    match &error {
        PointerHeadLoadError::UnsupportedDtype { tensor, dtype } => {
            assert_eq!(tensor, "q.weight");
            assert_eq!(dtype, "F16");
        }
        other => panic!("错误类型不符：{other:?}"),
    }
    assert!(
        error.to_string().contains("不支持"),
        "错误消息为中文：{}",
        error
    );
}

#[test]
fn parse_reports_malformed_header() {
    let error =
        parse_pointer_head(b"not-a-safetensors-file", 2, 2, 1.0).expect_err("非法字节必须失败");
    assert!(
        matches!(error, PointerHeadLoadError::MalformedHeader { .. }),
        "错误类型不符：{error:?}"
    );
    assert!(
        error.to_string().contains("非法"),
        "错误消息为中文：{}",
        error
    );
}

#[test]
fn parse_reports_shape_mismatch() {
    let bytes = synthetic_safetensors(&[
        ("q.weight", vec![0.0; 8], vec![4, 2]),
        ("q.bias", vec![0.0; 2], vec![2]),
        ("k.weight", vec![1.0, 0.0, 0.0, 1.0], vec![2, 2]),
        ("k.bias", vec![0.0; 2], vec![2]),
    ]);
    let error = parse_pointer_head(&bytes, 2, 2, 1.0).expect_err("形状不符必须失败");
    match &error {
        PointerHeadLoadError::InvalidShape { tensor, .. } => assert_eq!(tensor, "q.weight"),
        other => panic!("错误类型不符：{other:?}"),
    }
    assert!(
        error.to_string().contains("形状"),
        "错误消息为中文：{}",
        error
    );
}

#[test]
fn parse_reports_non_finite_weights() {
    let bytes = synthetic_safetensors(&[
        ("q.weight", vec![f32::NAN, 0.0, 0.0, 1.0], vec![2, 2]),
        ("q.bias", vec![0.0; 2], vec![2]),
        ("k.weight", vec![1.0, 0.0, 0.0, 1.0], vec![2, 2]),
        ("k.bias", vec![0.0; 2], vec![2]),
    ]);
    let error = parse_pointer_head(&bytes, 2, 2, 1.0).expect_err("NaN 权重必须失败");
    assert!(
        matches!(error, PointerHeadLoadError::WeightsRejected { .. }),
        "错误类型不符：{error:?}"
    );
    assert!(
        error.to_string().contains("非有限"),
        "错误消息为中文：{}",
        error
    );
}

#[test]
fn parse_reports_invalid_temperature() {
    let error = parse_pointer_head(&identity_head_safetensors(), 2, 2, 0.0)
        .expect_err("temperature 0 必须失败");
    assert!(
        matches!(error, PointerHeadLoadError::WeightsRejected { .. }),
        "错误类型不符：{error:?}"
    );
}

#[test]
fn load_reads_head_from_installed_file() {
    let directory = tempfile::tempdir().expect("临时目录创建成功");
    let path = directory.path().join("pointer_head.safetensors");
    std::fs::write(&path, identity_head_safetensors()).expect("写入测试资产成功");
    let head: PointerHead = load_pointer_head(&path, 2, 2, 1.0).expect("文件加载成功");
    let probabilities = head
        .score_options(&[1.0, 0.0], &[1.0, 0.0, 0.0, 1.0])
        .expect("概率计算成功");
    assert_eq!(probabilities.len(), 2);
}

#[test]
fn load_reports_missing_file() {
    let path = std::path::Path::new("/nonexistent/pointer_head.safetensors");
    let error = load_pointer_head(path, 2, 2, 1.0).expect_err("缺失文件必须失败");
    assert!(
        matches!(error, PointerHeadLoadError::ReadFailed { .. }),
        "错误类型不符：{error:?}"
    );
    assert!(
        error.to_string().contains("读取"),
        "错误消息为中文：{}",
        error
    );
}
