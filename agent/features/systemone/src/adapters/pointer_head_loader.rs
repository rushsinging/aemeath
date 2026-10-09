//! pointer_head.safetensors 加载：字节 → `PointerHead`（维度 / dtype / 有限性 fail-closed）。
//!
//! 文件契约（发行资产唯一形态）：header JSON 中四个 F32 张量
//! `q.weight [pointer_dimension, hidden_size]`、`q.bias [pointer_dimension]`、
//! `k.weight [pointer_dimension, hidden_size]`、`k.bias [pointer_dimension]`，
//! 行主序与 `PointerHeadWeights` 的矩阵口径一致；评分温度取 manifest（非文件）。
//! 加载层按设计 §3 放在 adapters，domain 保持零 IO。

use std::path::Path;

use crate::constants::POINTER_HEAD_TENSORS;
use crate::domain::{PointerHead, PointerHeadWeights};

/// PointerHead 加载失败（全部映射为启动期禁用，不构造 port）。
#[derive(Debug, Clone, PartialEq)]
pub enum PointerHeadLoadError {
    /// 文件不可读或不存在。
    ReadFailed { detail: String },
    /// header 长度 / JSON / 结构非法。
    MalformedHeader { detail: String },
    /// 数据段长度与 header 声明不符。
    DataTruncated { detail: String },
    /// 缺少契约张量。
    MissingTensor { tensor: String },
    /// 张量 dtype 不是 F32。
    UnsupportedDtype { tensor: String, dtype: String },
    /// 张量 shape 与声明维度不符。
    InvalidShape {
        tensor: String,
        found: Vec<u64>,
        expected: Vec<u64>,
    },
    /// 权重未通过 `PointerHeadWeights` 的长度 / 维度 / 有限性校验。
    WeightsRejected { detail: String },
}

impl std::fmt::Display for PointerHeadLoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadFailed { detail } => write!(formatter, "读取 pointer head 失败：{detail}"),
            Self::MalformedHeader { detail } => {
                write!(formatter, "pointer head safetensors header 非法：{detail}")
            }
            Self::DataTruncated { detail } => {
                write!(formatter, "pointer head 数据段与 header 不符：{detail}")
            }
            Self::MissingTensor { tensor } => {
                write!(formatter, "pointer head 缺少张量：{tensor}")
            }
            Self::UnsupportedDtype { tensor, dtype } => {
                write!(
                    formatter,
                    "pointer head 张量 {tensor} 不支持 dtype：{dtype}（仅 F32）"
                )
            }
            Self::InvalidShape {
                tensor,
                found,
                expected,
            } => write!(
                formatter,
                "pointer head 张量 {tensor} 形状非法：实际 {found:?}，期望 {expected:?}"
            ),
            Self::WeightsRejected { detail } => {
                write!(formatter, "pointer head 权重校验失败：{detail}")
            }
        }
    }
}

impl std::error::Error for PointerHeadLoadError {}

/// 从已安装文件加载 PointerHead。
pub(crate) fn load_pointer_head(
    path: &Path,
    hidden_size: usize,
    pointer_dimension: usize,
    temperature: f32,
) -> Result<PointerHead, PointerHeadLoadError> {
    let bytes = std::fs::read(path).map_err(|error| PointerHeadLoadError::ReadFailed {
        detail: format!("{}（{}）", error, path.display()),
    })?;
    parse_pointer_head(&bytes, hidden_size, pointer_dimension, temperature)
}

/// 从 safetensors 字节解析 PointerHead（纯函数，测试直接注入字节）。
pub(crate) fn parse_pointer_head(
    source: &[u8],
    hidden_size: usize,
    pointer_dimension: usize,
    temperature: f32,
) -> Result<PointerHead, PointerHeadLoadError> {
    let header_end = header_end(source)?;
    let header = std::str::from_utf8(source.get(8..header_end).ok_or_else(|| {
        PointerHeadLoadError::MalformedHeader {
            detail: "header 边界超出源字节范围".to_owned(),
        }
    })?)
    .map_err(|error| PointerHeadLoadError::MalformedHeader {
        detail: error.to_string(),
    })?;
    let entries: serde_json::Map<String, serde_json::Value> = serde_json::from_str(header)
        .map_err(|error| PointerHeadLoadError::MalformedHeader {
            detail: error.to_string(),
        })?;
    let data_section =
        source
            .get(header_end..)
            .ok_or_else(|| PointerHeadLoadError::MalformedHeader {
                detail: "数据段边界超出源字节范围".to_owned(),
            })?;

    let mut tensors: Vec<Vec<f32>> = Vec::with_capacity(POINTER_HEAD_TENSORS.len());
    for (tensor_name, expected_shape) in POINTER_HEAD_TENSORS {
        let entry =
            entries
                .get(tensor_name)
                .ok_or_else(|| PointerHeadLoadError::MissingTensor {
                    tensor: tensor_name.to_owned(),
                })?;
        let expected_shape = expected_shape(pointer_dimension, hidden_size);
        let shape = read_shape(entry, tensor_name)?;
        if shape != expected_shape {
            return Err(PointerHeadLoadError::InvalidShape {
                tensor: tensor_name.to_owned(),
                found: shape,
                expected: expected_shape,
            });
        }
        let dtype = entry
            .get("dtype")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if dtype != "F32" {
            return Err(PointerHeadLoadError::UnsupportedDtype {
                tensor: tensor_name.to_owned(),
                dtype: dtype.to_owned(),
            });
        }
        tensors.push(read_f32_tensor(entry, data_section, tensor_name)?);
    }

    let [query_weight, query_bias, key_weight, key_bias] = [
        tensors[0].as_slice(),
        tensors[1].as_slice(),
        tensors[2].as_slice(),
        tensors[3].as_slice(),
    ];
    let weights = PointerHeadWeights::new(
        hidden_size,
        pointer_dimension,
        temperature,
        query_weight,
        query_bias,
        key_weight,
        key_bias,
    )
    .map_err(|error| PointerHeadLoadError::WeightsRejected {
        detail: error.to_string(),
    })?;
    Ok(PointerHead::new(weights))
}

/// header 长度前缀边界（8 字节 LE u64 + header 字节）。
fn header_end(source: &[u8]) -> Result<usize, PointerHeadLoadError> {
    if source.len() < 8 {
        return Err(PointerHeadLoadError::MalformedHeader {
            detail: "字节长度不足 8 字节长度前缀".to_owned(),
        });
    }
    let header_length = u64::from_le_bytes(
        source[0..8]
            .try_into()
            .expect("切片长度 8 已由上界校验保障"),
    );
    let header_end = (8_usize)
        .checked_add(usize::try_from(header_length).map_err(|_| {
            PointerHeadLoadError::MalformedHeader {
                detail: "header 长度超出地址空间".to_owned(),
            }
        })?)
        .ok_or_else(|| PointerHeadLoadError::MalformedHeader {
            detail: "header 长度溢出".to_owned(),
        })?;
    if header_end > source.len() {
        return Err(PointerHeadLoadError::MalformedHeader {
            detail: format!(
                "header 声明 {header_end} 字节，实际文件仅 {} 字节",
                source.len()
            ),
        });
    }
    Ok(header_end)
}

/// 读取张量 shape（缺失 / 非数组 → header 非法）。
fn read_shape(
    entry: &serde_json::Value,
    tensor_name: &str,
) -> Result<Vec<u64>, PointerHeadLoadError> {
    entry
        .get("shape")
        .and_then(serde_json::Value::as_array)
        .map(|dimensions| {
            dimensions
                .iter()
                .map(|dimension| dimension.as_u64().unwrap_or(u64::MAX))
                .collect()
        })
        .ok_or_else(|| PointerHeadLoadError::MalformedHeader {
            detail: format!("张量 {tensor_name} 缺少 shape 数组"),
        })
}

/// 按 data_offsets 读取 F32 LE 张量数据。
fn read_f32_tensor(
    entry: &serde_json::Value,
    data_section: &[u8],
    tensor_name: &str,
) -> Result<Vec<f32>, PointerHeadLoadError> {
    let offsets: Vec<u64> = entry
        .get("data_offsets")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(|value| value.as_u64().unwrap_or(u64::MAX))
                .collect()
        })
        .ok_or_else(|| PointerHeadLoadError::MalformedHeader {
            detail: format!("张量 {tensor_name} 缺少 data_offsets"),
        })?;
    let [begin, end] = offsets.as_slice() else {
        return Err(PointerHeadLoadError::MalformedHeader {
            detail: format!("张量 {tensor_name} 的 data_offsets 必须为 [begin, end]"),
        });
    };
    let (begin, end) = (
        usize::try_from(*begin).unwrap_or(usize::MAX),
        usize::try_from(*end).unwrap_or(usize::MAX),
    );
    if end < begin || end > data_section.len() {
        return Err(PointerHeadLoadError::DataTruncated {
            detail: format!(
                "张量 {tensor_name} 声明区间 [{begin}, {end}) 超出数据段 {} 字节",
                data_section.len()
            ),
        });
    }
    let payload =
        data_section
            .get(begin..end)
            .ok_or_else(|| PointerHeadLoadError::DataTruncated {
                detail: format!(
                    "张量 {tensor_name} 声明区间 [{begin}, {end}) 超出数据段 {} 字节",
                    data_section.len()
                ),
            })?;
    if payload.len() % 4 != 0 {
        return Err(PointerHeadLoadError::DataTruncated {
            detail: format!(
                "张量 {tensor_name} 的 F32 数据长度 {} 非 4 的倍数",
                payload.len()
            ),
        });
    }
    Ok(payload
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().expect("chunks_exact(4) 保证 4 字节")))
        .collect())
}

#[cfg(test)]
#[path = "pointer_head_loader_tests.rs"]
mod tests;
