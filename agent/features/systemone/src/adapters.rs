pub mod audited;
pub mod calibrated;
pub mod calibration_store;
#[cfg(feature = "embedded")]
pub mod embedded;
pub mod fetch_http;
#[cfg(any(test, feature = "http-adapter"))]
pub mod jev_http;
#[cfg(any(test, feature = "http-adapter"))]
pub mod jev_wire;
#[cfg(feature = "embedded")]
pub mod kev_answer_mapping;
#[cfg(feature = "embedded")]
pub mod kev_causal_row;
#[cfg(feature = "embedded")]
pub mod kev_hf_tokenizer;
#[cfg(feature = "embedded")]
pub mod kev_question_plan;
// 原生 llama.cpp worker 仅在首批平台（macOS arm64）编译；
// 其余平台经 `llama_worker::start_llama_worker` 的 cfg 分支报告不支持。
#[cfg(all(feature = "embedded", target_os = "macos", target_arch = "aarch64"))]
pub mod llama_native_engine;
#[cfg(feature = "embedded")]
pub mod llama_worker;
pub mod model_assets;
pub mod null;
#[cfg(feature = "embedded")]
pub mod pointer_head_loader;
