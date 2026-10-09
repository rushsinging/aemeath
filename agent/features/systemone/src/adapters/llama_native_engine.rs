//! 原生 llama.cpp 行前向引擎：backend / model / context / batch 驻留 worker 线程。
//!
//! 生命周期（设计 §4.1）：
//! - [`init_llama_row_engine`] 在 [`crate::adapters::llama_worker::spawn_worker_thread`]
//!   的线程闭包内调用，native 类型 NEVER 离开该线程；
//! - context 以 `embeddings=true` + `pooling=none` 创建，容量取 REPORT 基线；
//! - 每个需要读取的 token 以 `logits=true` 入 batch，`decode` 后立即复制
//!   `embeddings_ith` 为 owned `Vec<f32>`，随后清 KV 与 batch；
//! - 模型文件缺失、加载失败、维度不符 → typed `WorkerInitError`（不构造引擎）。
//!
//! unsafe 边界：`LlamaContext<'a>` 借用 `LlamaModel`，两者需同生共死地驻留
//! worker 线程。模型以 `Box` 置于稳定堆地址、context 借用其 `'static` 引用，
//! [`LlamaRowEngine::drop`] 保证 context 先于模型释放——unsafe 仅存在于
//! 本文件的构造与析构，NEVER 外溢。

use std::num::NonZeroU32;
use std::path::Path;

use llama_cpp_2::context::params::{LlamaContextParams, LlamaPoolingType};
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::token::LlamaToken;

use crate::adapters::llama_worker::{
    CausalRow, LlamaWorkerConfig, RowEmbeddingEngine, RowHiddenVectors, WorkerFailure,
    WorkerInitError,
};

/// 模型文件存在性预检：`LlamaModel::load_from_file` 在 debug 下对缺失路径
/// 直接断言 panic，此处先行 fail-closed 并给出中文原因。
fn ensure_model_file_exists(model_path: &Path) -> Result<(), WorkerInitError> {
    if model_path.is_file() {
        Ok(())
    } else {
        Err(WorkerInitError::ModelLoadFailed {
            detail: format!("模型文件不存在：{}", model_path.display()),
        })
    }
}

/// 持有 model/context/batch 的行前向引擎（仅存在于 worker 线程内）。
pub(crate) struct LlamaRowEngine {
    /// 借用 `model_ptr` 的 context；`ManuallyDrop` 由 `drop` 显式控制释放顺序。
    context: std::mem::ManuallyDrop<llama_cpp_2::context::LlamaContext<'static>>,
    /// 复用的解码 batch（容量 = 上下文容量，逐行 clear）。
    batch: LlamaBatch<'static>,
    /// 稳定堆地址上的模型（`Box::into_raw`，`drop` 时 `Box::from_raw` 回收）。
    model_ptr: *mut LlamaModel,
    /// backend 生命周期证据：最后释放（字段声明顺序）。
    _backend: LlamaBackend,
    /// 期望 hidden 宽度（init 期已与模型对账，回读时逐次校验）。
    hidden_size: usize,
    /// 上下文容量（同时是单 row 的 token 上限）。
    context_tokens: u32,
}

/// 在 worker 线程内初始化 native 引擎：文件 → backend → model → 维度对账 → context → batch。
pub(crate) fn init_llama_row_engine(
    config: &LlamaWorkerConfig,
) -> Result<LlamaRowEngine, WorkerInitError> {
    ensure_model_file_exists(&config.model_path)?;
    // llama.cpp 0.1.159 已知缺陷（ggml-metal-device.m:1025）：residency sets
    // 未释放即 device_free 会 SIGABRT（worker 退出 / 进程收尾时触发用户可见
    // 崩溃）。在 backend init（device 唯一读取点）前关闭该特性规避；用户
    // 已显式配置该变量时 NEVER 覆盖。上游修复后移除（ggml-org/llama.cpp#17869）。
    if std::env::var_os("GGML_METAL_NO_RESIDENCY").is_none() {
        std::env::set_var("GGML_METAL_NO_RESIDENCY", "1");
    }
    let backend = LlamaBackend::init().map_err(|error| WorkerInitError::BackendInitFailed {
        detail: error.to_string(),
    })?;
    let model =
        LlamaModel::load_from_file(&backend, &config.model_path, &LlamaModelParams::default())
            .map_err(|error| WorkerInitError::ModelLoadFailed {
                detail: error.to_string(),
            })?;
    let output_width = usize::try_from(model.n_embd_out()).unwrap_or(usize::MAX);
    if output_width != config.hidden_size {
        return Err(WorkerInitError::HiddenSizeMismatch {
            expected: config.hidden_size,
            found: output_width,
        });
    }
    let context_params = LlamaContextParams::default()
        .with_embeddings(true)
        .with_pooling_type(LlamaPoolingType::None)
        .with_n_ctx(NonZeroU32::new(config.context_tokens))
        .with_n_batch(config.context_tokens)
        .with_n_ubatch(config.ubatch_tokens)
        .with_n_seq_max(1);
    let model_ptr = Box::into_raw(Box::new(model));
    // SAFETY: `model_ptr` 为 Box 置入的稳定堆地址，仅在 `LlamaRowEngine::drop`
    // 中经 `Box::from_raw` 回收；`LlamaRowEngine::drop` 先释放 context 再回收模型，
    // 且引擎整体只在 worker 线程内构造与析构，故该 `'static` 借用恒被上下文覆盖。
    let model_reference: &'static LlamaModel = unsafe { &*model_ptr };
    let context = match model_reference.new_context(&backend, context_params) {
        Ok(context) => std::mem::ManuallyDrop::new(context),
        Err(error) => {
            // SAFETY: 上面刚取出的裸指针尚未移出，此处立即回收，无别名存活。
            unsafe {
                drop(Box::from_raw(model_ptr));
            }
            return Err(WorkerInitError::ContextCreateFailed {
                detail: error.to_string(),
            });
        }
    };
    Ok(LlamaRowEngine {
        context,
        batch: LlamaBatch::new(config.context_tokens as usize, 1),
        model_ptr,
        _backend: backend,
        hidden_size: config.hidden_size,
        context_tokens: config.context_tokens,
    })
}

impl LlamaRowEngine {
    /// 前向单行：入 batch → decode → 复制 readout → 清 KV/batch（含失败路径）。
    fn embed_one_row(&mut self, row: &CausalRow) -> Result<RowHiddenVectors, WorkerFailure> {
        let readout_flags = self.readout_flags(row)?;
        self.batch.clear();
        for (offset, token_id) in row.token_ids.iter().enumerate() {
            let add_result = self.batch.add(
                LlamaToken::new(*token_id),
                offset as i32,
                &[0],
                readout_flags[offset],
            );
            if let Err(error) = add_result {
                self.batch.clear();
                return Err(WorkerFailure::new(format!("causal row 入批失败：{error}")));
            }
        }
        if let Err(error) = self.context.decode(&mut self.batch) {
            self.context.clear_kv_cache();
            self.batch.clear();
            return Err(WorkerFailure::new(format!("causal row 解码失败：{error}")));
        }
        let readouts = self.copy_row_readouts(row);
        self.context.clear_kv_cache();
        self.batch.clear();
        readouts
    }

    /// 校验 readout 偏移并生成 logits 标记（仅 decide / 各 option 位置为 true）。
    fn readout_flags(&self, row: &CausalRow) -> Result<Vec<bool>, WorkerFailure> {
        let token_count = row.token_ids.len();
        if token_count == 0 {
            return Err(WorkerFailure::new("空 causal row 无 token 可解码"));
        }
        if u32::try_from(token_count).unwrap_or(u32::MAX) > self.context_tokens {
            return Err(WorkerFailure::new(format!(
                "causal row 长度 {token_count} 超出上下文容量 {} tokens",
                self.context_tokens
            )));
        }
        if row.decide_offset >= token_count {
            return Err(WorkerFailure::new(format!(
                "decide 偏移 {} 超出 row 长度 {token_count}",
                row.decide_offset
            )));
        }
        let mut flags = vec![false; token_count];
        flags[row.decide_offset] = true;
        for option_offset in &row.option_offsets {
            if *option_offset >= token_count {
                return Err(WorkerFailure::new(format!(
                    "选项 readout 偏移 {option_offset} 超出 row 长度 {token_count}"
                )));
            }
            flags[*option_offset] = true;
        }
        Ok(flags)
    }

    /// 立即复制 decide / 各选项的 `embeddings_ith` 为 owned 向量并校验宽度与有限性。
    fn copy_row_readouts(&self, row: &CausalRow) -> Result<RowHiddenVectors, WorkerFailure> {
        let decide = self.copy_embedding_at(row.decide_offset)?;
        let mut options = Vec::with_capacity(row.option_offsets.len());
        for option_offset in &row.option_offsets {
            options.push(self.copy_embedding_at(*option_offset)?);
        }
        Ok(RowHiddenVectors::new(decide, options))
    }

    /// 读取 batch 内第 `offset` 个 token 的 hidden 并复制为 owned 向量。
    fn copy_embedding_at(&self, offset: usize) -> Result<Vec<f32>, WorkerFailure> {
        let embedding = self
            .context
            .embeddings_ith(offset as i32)
            .map_err(|error| {
                WorkerFailure::new(format!("读取第 {offset} 个 token 的 hidden 失败：{error}"))
            })?;
        if embedding.len() != self.hidden_size {
            return Err(WorkerFailure::new(format!(
                "hidden 长度不符：期望 {}，实际 {}",
                self.hidden_size,
                embedding.len()
            )));
        }
        if embedding.iter().any(|value| !value.is_finite()) {
            return Err(WorkerFailure::new(format!(
                "第 {offset} 个 token 的 hidden 含非有限数值"
            )));
        }
        Ok(embedding.to_vec())
    }
}

impl RowEmbeddingEngine for LlamaRowEngine {
    fn embed_rows(&mut self, rows: Vec<CausalRow>) -> Result<Vec<RowHiddenVectors>, WorkerFailure> {
        let mut results = Vec::with_capacity(rows.len());
        for row in &rows {
            results.push(self.embed_one_row(row)?);
        }
        Ok(results)
    }
}

impl Drop for LlamaRowEngine {
    fn drop(&mut self) {
        // 释放顺序不变量：context 借用 model_ptr，必须先释放 context 再回收 Box；
        // 随后字段按声明顺序析构，`_backend` 最后释放（llama_backend_free 收尾）。
        unsafe {
            std::mem::ManuallyDrop::drop(&mut self.context);
            drop(Box::from_raw(self.model_ptr));
        }
    }
}

#[cfg(test)]
#[path = "llama_native_engine_tests.rs"]
mod tests;
