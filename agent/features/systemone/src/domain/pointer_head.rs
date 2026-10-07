//! PointerHead 领域纯数学：`query = Wq*h_decide+bq`、`key = Wk*h_option+bk`、
//! `logit = dot(key, query) / sqrt(pointer_dimension) / temperature`（矩阵行主序）
//! 与减最大值 softmax。
//!
//! 公式与 kev `PointerHead` 对齐（数值口径见 `eval/system-one/harness/parity_gguf.py`）。
//! 纯数值计算：无 IO、不读环境变量、不依赖 llama.cpp；权重维度由调用方声明，
//! 生产维度 1024/256 的锁定由 `domain::model_manifest` 的契约校验单独承担。

/// PointerHead 计算错误：维度、长度、非有限数值与温度非法。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointerHeadError {
    /// 声明维度为 0（`hidden_size` / `pointer_dimension`）。
    NonPositiveDimension { dimension: &'static str },
    /// 权重张量长度与声明维度不符。
    WeightLengthMismatch {
        weight: &'static str,
        expected: usize,
        found: usize,
    },
    /// 权重张量含 NaN / 无穷值。
    NonFiniteWeight { weight: &'static str },
    /// temperature 非有限或非正。
    InvalidTemperature,
    /// decide hidden 长度与 hidden_size 不符。
    HiddenLengthMismatch {
        hidden: &'static str,
        expected: usize,
        found: usize,
    },
    /// 选项集为空。
    EmptyOptions,
    /// 选项 hidden 总长不是 hidden_size 的整数倍。
    OptionsLengthMismatch { found: usize, hidden_size: usize },
    /// decide hidden 含 NaN / 无穷值。
    NonFiniteHidden { hidden: &'static str },
    /// 某个选项 hidden 含 NaN / 无穷值。
    NonFiniteOption { option_index: usize },
    /// q/k 投影溢出为非有限值。
    NonFiniteProjection { projection: &'static str },
    /// 某个选项的 logit 溢出为非有限值。
    NonFiniteLogit { option_index: usize },
}

impl std::fmt::Display for PointerHeadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonPositiveDimension { dimension } => {
                write!(formatter, "维度 {dimension} 必须为正数")
            }
            Self::WeightLengthMismatch {
                weight,
                expected,
                found,
            } => write!(
                formatter,
                "权重 {weight} 长度应为 {expected}，实际为 {found}"
            ),
            Self::NonFiniteWeight { weight } => {
                write!(formatter, "权重 {weight} 含非有限数值（NaN 或无穷）")
            }
            Self::InvalidTemperature => write!(formatter, "temperature 必须为有限正数"),
            Self::HiddenLengthMismatch {
                hidden,
                expected,
                found,
            } => write!(
                formatter,
                "hidden（{hidden}）长度应为 {expected}，实际为 {found}"
            ),
            Self::EmptyOptions => write!(formatter, "选项 hidden 集合为空"),
            Self::OptionsLengthMismatch { found, hidden_size } => write!(
                formatter,
                "选项 hidden 长度 {found} 不是 hidden_size {hidden_size} 的整数倍"
            ),
            Self::NonFiniteHidden { hidden } => {
                write!(formatter, "hidden（{hidden}）含非有限数值（NaN 或无穷）")
            }
            Self::NonFiniteOption { option_index } => {
                write!(formatter, "第 {option_index} 个选项的 hidden 含非有限数值")
            }
            Self::NonFiniteProjection { projection } => {
                write!(formatter, "{projection} 投影结果为非有限数值")
            }
            Self::NonFiniteLogit { option_index } => {
                write!(formatter, "第 {option_index} 个选项的 logit 为非有限数值")
            }
        }
    }
}

impl std::error::Error for PointerHeadError {}

/// PointerHead 权重：行主序 q/k 投影矩阵、偏置与温度。
///
/// `query_weight` / `key_weight` 均为 `[pointer_dimension × hidden_size]` 行主序展开。
#[derive(Debug, Clone, PartialEq)]
pub struct PointerHeadWeights {
    hidden_size: usize,
    pointer_dimension: usize,
    temperature: f32,
    query_weight: Vec<f32>,
    query_bias: Vec<f32>,
    key_weight: Vec<f32>,
    key_bias: Vec<f32>,
}

impl PointerHeadWeights {
    /// 构造权重：校验声明维度、矩阵/偏置长度以及**所有**数值的有限性。
    pub fn new(
        hidden_size: usize,
        pointer_dimension: usize,
        temperature: f32,
        query_weight: &[f32],
        query_bias: &[f32],
        key_weight: &[f32],
        key_bias: &[f32],
    ) -> Result<Self, PointerHeadError> {
        if hidden_size == 0 {
            return Err(PointerHeadError::NonPositiveDimension {
                dimension: "hidden_size",
            });
        }
        if pointer_dimension == 0 {
            return Err(PointerHeadError::NonPositiveDimension {
                dimension: "pointer_dimension",
            });
        }
        if !temperature.is_finite() || temperature <= 0.0 {
            return Err(PointerHeadError::InvalidTemperature);
        }
        let matrix_length = pointer_dimension * hidden_size;
        ensure_length("query_weight", matrix_length, query_weight)?;
        ensure_length("query_bias", pointer_dimension, query_bias)?;
        ensure_length("key_weight", matrix_length, key_weight)?;
        ensure_length("key_bias", pointer_dimension, key_bias)?;
        for (weight, values) in [
            ("query_weight", query_weight),
            ("query_bias", query_bias),
            ("key_weight", key_weight),
            ("key_bias", key_bias),
        ] {
            if values.iter().any(|value| !value.is_finite()) {
                return Err(PointerHeadError::NonFiniteWeight { weight });
            }
        }
        Ok(Self {
            hidden_size,
            pointer_dimension,
            temperature,
            query_weight: query_weight.to_vec(),
            query_bias: query_bias.to_vec(),
            key_weight: key_weight.to_vec(),
            key_bias: key_bias.to_vec(),
        })
    }

    /// 声明的 hidden size（loader 与 manifest 对账用）。
    pub fn hidden_size(&self) -> usize {
        self.hidden_size
    }

    /// 声明的 pointer dimension（loader 与 manifest 对账用）。
    pub fn pointer_dimension(&self) -> usize {
        self.pointer_dimension
    }

    /// 声明的评分温度（loader 与 manifest 对账用）。
    pub fn temperature(&self) -> f32 {
        self.temperature
    }

    /// `query = Wq*h_decide+bq`，结果逐分量校验有限性。
    fn project_query(&self, hidden_decide: &[f32]) -> Result<Vec<f32>, PointerHeadError> {
        project(
            &self.query_weight,
            &self.query_bias,
            hidden_decide,
            self.pointer_dimension,
            "query",
        )
    }

    /// `key = Wk*h_option+bk`，结果逐分量校验有限性。
    fn project_key(&self, hidden_option: &[f32]) -> Result<Vec<f32>, PointerHeadError> {
        project(
            &self.key_weight,
            &self.key_bias,
            hidden_option,
            self.pointer_dimension,
            "key",
        )
    }
}

/// PointerHead：hidden states → q/k 投影 → 温度缩放 logits → softmax 概率。
#[derive(Debug, Clone, PartialEq)]
pub struct PointerHead {
    weights: PointerHeadWeights,
}

impl PointerHead {
    pub fn new(weights: PointerHeadWeights) -> Self {
        Self { weights }
    }

    /// 对一组候选选项打分，返回与选项顺序一致的概率分布（和约为 1、全部有限）。
    ///
    /// `hidden_options` 为 `option_count × hidden_size` 行主序展开；选项至少一个。
    pub fn score_options(
        &self,
        hidden_decide: &[f32],
        hidden_options: &[f32],
    ) -> Result<Vec<f32>, PointerHeadError> {
        let weights = &self.weights;
        if hidden_decide.len() != weights.hidden_size {
            return Err(PointerHeadError::HiddenLengthMismatch {
                hidden: "decide",
                expected: weights.hidden_size,
                found: hidden_decide.len(),
            });
        }
        if hidden_decide.iter().any(|value| !value.is_finite()) {
            return Err(PointerHeadError::NonFiniteHidden { hidden: "decide" });
        }
        if hidden_options.is_empty() {
            return Err(PointerHeadError::EmptyOptions);
        }
        if !hidden_options.len().is_multiple_of(weights.hidden_size) {
            return Err(PointerHeadError::OptionsLengthMismatch {
                found: hidden_options.len(),
                hidden_size: weights.hidden_size,
            });
        }
        for (element_index, value) in hidden_options.iter().enumerate() {
            if !value.is_finite() {
                return Err(PointerHeadError::NonFiniteOption {
                    option_index: element_index / weights.hidden_size,
                });
            }
        }

        let query = weights.project_query(hidden_decide)?;
        let scale = (weights.pointer_dimension as f32).sqrt();
        let mut logits = Vec::with_capacity(hidden_options.len() / weights.hidden_size);
        for (option_index, hidden_option) in
            hidden_options.chunks_exact(weights.hidden_size).enumerate()
        {
            let key = weights.project_key(hidden_option)?;
            let dot: f32 = key
                .iter()
                .zip(query.iter())
                .map(|(key_value, query_value)| key_value * query_value)
                .sum();
            let logit = dot / scale / weights.temperature;
            if !logit.is_finite() {
                return Err(PointerHeadError::NonFiniteLogit { option_index });
            }
            logits.push(logit);
        }
        Ok(softmax(&logits))
    }
}

/// 校验权重张量长度与声明期望一致。
fn ensure_length(
    weight: &'static str,
    expected: usize,
    values: &[f32],
) -> Result<(), PointerHeadError> {
    if values.len() == expected {
        Ok(())
    } else {
        Err(PointerHeadError::WeightLengthMismatch {
            weight,
            expected,
            found: values.len(),
        })
    }
}

/// `output = matrix * hidden + bias`（行主序），溢出即报错。
///
/// `bias.len() == output_len == pointer_dimension` 由 `PointerHeadWeights::new`
/// 的矩阵/偏置长度校验保障，故完整迭代 bias、不做静默截断。
fn project(
    matrix: &[f32],
    bias: &[f32],
    hidden: &[f32],
    output_len: usize,
    projection: &'static str,
) -> Result<Vec<f32>, PointerHeadError> {
    debug_assert_eq!(
        bias.len(),
        output_len,
        "bias 长度应由 PointerHeadWeights::new 的长度校验保障"
    );
    let mut output = Vec::with_capacity(output_len);
    for (row_index, bias_value) in bias.iter().enumerate() {
        let row_start = row_index * hidden.len();
        let row = &matrix[row_start..row_start + hidden.len()];
        let value: f32 = row
            .iter()
            .zip(hidden.iter())
            .map(|(weight, activation)| weight * activation)
            .sum::<f32>()
            + bias_value;
        if !value.is_finite() {
            return Err(PointerHeadError::NonFiniteProjection { projection });
        }
        output.push(value);
    }
    Ok(output)
}

/// 减最大值 softmax：避免大 logits 溢出，结果全部有限且和约为 1。
fn softmax(logits: &[f32]) -> Vec<f32> {
    let peak = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exponentials: Vec<f32> = logits.iter().map(|logit| (logit - peak).exp()).collect();
    let total: f32 = exponentials.iter().sum();
    exponentials
        .iter()
        .map(|exponential| exponential / total)
        .collect()
}

#[cfg(test)]
#[path = "pointer_head_tests.rs"]
mod tests;
