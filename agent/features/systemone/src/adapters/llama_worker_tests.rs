//! embedded worker 协议契约：owned row → owned hidden vectors、失败降级与初始化失败。

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::adapters::llama_worker::{
    spawn_worker_thread, CausalRow, EmbeddedWorkerClient, RowEmbeddingEngine, RowHiddenVectors,
    WorkerFailure, WorkerInitError,
};
use crate::domain::UnavailableKind;

/// 确定性 fake engine：把 readout 位置的 token id 广播为 hidden 向量。
struct EchoReadoutEngine {
    hidden_size: usize,
}

impl RowEmbeddingEngine for EchoReadoutEngine {
    fn embed_rows(&mut self, rows: Vec<CausalRow>) -> Result<Vec<RowHiddenVectors>, WorkerFailure> {
        Ok(rows
            .iter()
            .map(|row| {
                RowHiddenVectors::new(
                    vec![row.token_ids[row.decide_offset] as f32; self.hidden_size],
                    row.option_offsets
                        .iter()
                        .map(|offset| vec![row.token_ids[*offset] as f32; self.hidden_size])
                        .collect(),
                )
            })
            .collect())
    }
}

/// 固定失败的 fake engine。
struct RejectingEngine {
    detail: String,
}

impl RowEmbeddingEngine for RejectingEngine {
    fn embed_rows(
        &mut self,
        _rows: Vec<CausalRow>,
    ) -> Result<Vec<RowHiddenVectors>, WorkerFailure> {
        Err(WorkerFailure::new(self.detail.clone()))
    }
}

/// 请求到达后使 worker 线程崩溃的 fake engine。
struct PanickingEngine;

impl RowEmbeddingEngine for PanickingEngine {
    fn embed_rows(
        &mut self,
        _rows: Vec<CausalRow>,
    ) -> Result<Vec<RowHiddenVectors>, WorkerFailure> {
        panic!("模拟 worker 线程崩溃");
    }
}

fn sample_row() -> CausalRow {
    CausalRow::new(vec![7, 8, 9, 10], 1, vec![0, 3])
}

async fn spawn_ok<E, F>(init_engine: F) -> EmbeddedWorkerClient
where
    E: RowEmbeddingEngine + 'static,
    F: FnOnce() -> Result<E, WorkerInitError> + Send + 'static,
{
    let init_receiver = spawn_worker_thread(init_engine);
    init_receiver
        .await
        .expect("worker 初始化通道不被丢弃")
        .expect("fake engine 初始化成功")
}

#[tokio::test]
async fn spawn_returns_client_and_roundtrips_owned_rows() {
    let client = spawn_ok(|| Ok(EchoReadoutEngine { hidden_size: 4 })).await;
    let vectors = client.run_rows(vec![sample_row()]).await.expect("推理成功");
    assert_eq!(vectors.len(), 1);
    assert_eq!(vectors[0].decide, vec![8.0; 4], "decide readout 取 token 8");
    assert_eq!(
        vectors[0].options,
        vec![vec![7.0; 4], vec![10.0; 4]],
        "选项 readout 按 option_offsets 顺序返回 owned 向量"
    );
}

#[tokio::test]
async fn spawn_when_engine_init_fails_returns_typed_error_and_no_client() {
    let init_receiver = spawn_worker_thread(|| {
        Err::<EchoReadoutEngine, _>(WorkerInitError::UnsupportedPlatform {
            platform: "linux-x86_64".to_owned(),
        })
    });
    let error = init_receiver
        .await
        .expect("worker 初始化通道不被丢弃")
        .expect_err("初始化失败必须返回错误");
    assert_eq!(
        error,
        WorkerInitError::UnsupportedPlatform {
            platform: "linux-x86_64".to_owned()
        }
    );
    let detail = error.to_string();
    assert!(
        detail.contains("不支持") && detail.contains("linux-x86_64"),
        "错误消息为中文并携带平台标识：{detail}"
    );
}

#[tokio::test]
async fn run_rows_when_engine_rejects_maps_to_server_unavailable() {
    let client = spawn_ok(|| {
        Ok(RejectingEngine {
            detail: "causal row 解码失败：测试".to_owned(),
        })
    })
    .await;
    let error = client
        .run_rows(vec![sample_row()])
        .await
        .expect_err("失败必须降级");
    assert_eq!(error.kind(), UnavailableKind::Server, "{error}");
    assert!(
        error.to_string().contains("causal row 解码失败：测试"),
        "明细保留 worker 的中文原因：{error}"
    );
}

#[tokio::test]
async fn run_rows_when_worker_thread_dies_maps_to_connect_unavailable() {
    let client = spawn_ok(|| Ok(PanickingEngine)).await;
    let first_error = client
        .run_rows(vec![sample_row()])
        .await
        .expect_err("worker 崩溃必须降级");
    assert_eq!(
        first_error.kind(),
        UnavailableKind::Connect,
        "{first_error}"
    );
    let second_error = client
        .run_rows(vec![sample_row()])
        .await
        .expect_err("worker 已退出，后续请求同样降级");
    assert_eq!(
        second_error.kind(),
        UnavailableKind::Connect,
        "{second_error}"
    );
}

#[tokio::test]
async fn concurrent_run_rows_keep_request_and_response_pairing() {
    let client = std::sync::Arc::new(spawn_ok(|| Ok(EchoReadoutEngine { hidden_size: 2 })).await);
    let first_task = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .run_rows(vec![CausalRow::new(vec![101, 102], 0, vec![1])])
                .await
        }
    });
    let second_task = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .run_rows(vec![CausalRow::new(vec![201, 202], 1, vec![0])])
                .await
        }
    });
    let first_vectors = first_task.await.unwrap().expect("首请求成功");
    let second_vectors = second_task.await.unwrap().expect("次请求成功");
    assert_eq!(first_vectors[0].decide, vec![101.0; 2]);
    assert_eq!(
        second_vectors[0].decide,
        vec![202.0; 2],
        "响应与请求逐条配对，不串号"
    );
}

#[tokio::test]
async fn worker_loop_survives_single_failure_and_serves_next_request() {
    let served_requests = std::sync::Arc::new(AtomicUsize::new(0));
    let client = spawn_ok({
        let served_requests = served_requests.clone();
        move || {
            Ok(CountingEngine {
                hidden_size: 2,
                served_requests,
            })
        }
    })
    .await;

    let first_error = client
        .run_rows(vec![sample_row()])
        .await
        .expect_err("首次降级");
    assert_eq!(first_error.kind(), UnavailableKind::Server, "{first_error}");
    let vectors = client
        .run_rows(vec![sample_row()])
        .await
        .expect("单次失败不熔断，后续请求仍可服务");
    assert_eq!(vectors[0].decide, vec![8.0; 2]);
    assert_eq!(served_requests.load(Ordering::SeqCst), 2);
}

/// 仅首个请求失败的 fake engine：证明 worker 单次失败不会终止线程。
struct CountingEngine {
    hidden_size: usize,
    served_requests: std::sync::Arc<AtomicUsize>,
}

impl RowEmbeddingEngine for CountingEngine {
    fn embed_rows(&mut self, rows: Vec<CausalRow>) -> Result<Vec<RowHiddenVectors>, WorkerFailure> {
        let attempt = self.served_requests.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            return Err(WorkerFailure::new("首个请求失败：测试".to_owned()));
        }
        Ok(rows
            .iter()
            .map(|row| {
                RowHiddenVectors::new(
                    vec![row.token_ids[row.decide_offset] as f32; self.hidden_size],
                    vec![vec![
                        row.token_ids[row.decide_offset] as f32;
                        self.hidden_size
                    ]],
                )
            })
            .collect())
    }
}
