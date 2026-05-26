use crate::agent::{Agent, ToolCall};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::{Tool, ToolContext, ToolRegistry, ToolResult};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

/// A tool that records the start time and sleeps briefly.
/// Marked as concurrency-safe or not depending on constructor.
struct TimedTool {
    name: String,
    safe: bool,
    start_times: Arc<std::sync::Mutex<Vec<u64>>>,
    sleep_ms: u64,
}

#[async_trait]
impl Tool for TimedTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "timed test tool"
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({"type": "object"})
    }
    fn is_concurrency_safe(&self) -> bool {
        self.safe
    }
    async fn call(&self, _input: Value, _ctx: &ToolContext) -> ToolResult {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        self.start_times.lock().unwrap().push(now);
        tokio::time::sleep(std::time::Duration::from_millis(self.sleep_ms)).await;
        ToolResult::success("done")
    }
}

fn test_ctx() -> ToolContext {
    let cwd = std::env::current_dir().unwrap();
    ToolContext {
        cwd: cwd.clone(),
        working_root: Arc::new(std::sync::Mutex::new(cwd.clone())),
        path_base: Arc::new(std::sync::Mutex::new(cwd)),
        cancel: tokio_util::sync::CancellationToken::new(),
        read_files: Arc::new(std::sync::Mutex::new(HashSet::new())),
        agent_runner: None,
        session_reminders: None,
        memory_config: share::config::MemoryConfig::default(),
        plan_mode: None,
        allow_all: true,
        max_tool_concurrency: 10,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
        context_stack: Arc::new(std::sync::Mutex::new(Vec::new())),
    }
}

#[tokio::test]
async fn test_execute_tools_concurrent_safe_tools_run_in_parallel() {
    let start_times = Arc::new(std::sync::Mutex::new(Vec::new()));
    let registry = ToolRegistry::new();
    registry.register(Box::new(TimedTool {
        name: "parallel_a".to_string(),
        safe: true,
        start_times: start_times.clone(),
        sleep_ms: 200,
    }));
    registry.register(Box::new(TimedTool {
        name: "parallel_b".to_string(),
        safe: true,
        start_times: start_times.clone(),
        sleep_ms: 200,
    }));

    let ctx = test_ctx();
    let agent = Agent {
        registry: &registry,
        ctx,
    };

    let tool_calls = vec![
        ToolCall {
            id: "a".to_string(),
            name: "parallel_a".to_string(),
            input: serde_json::json!({}),
        },
        ToolCall {
            id: "b".to_string(),
            name: "parallel_b".to_string(),
            input: serde_json::json!({}),
        },
    ];

    let start = std::time::Instant::now();
    let results = agent.execute_tools(&tool_calls).await;
    let elapsed = start.elapsed();

    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| !r.2), "no errors expected");

    // If they ran in parallel, total time should be < 350ms (2 * 200ms = 400ms if serial)
    assert!(
        elapsed.as_millis() < 350,
        "expected parallel execution (< 350ms), got {}ms",
        elapsed.as_millis()
    );

    // Both should have started within 50ms of each other
    let times = start_times.lock().unwrap();
    let diff = times[0].abs_diff(times[1]);
    assert!(
        diff < 100,
        "expected both tools to start within 100ms, got {diff}ms apart"
    );
}

#[tokio::test]
async fn test_execute_tools_non_concurrent_safe_run_sequentially() {
    let start_times = Arc::new(std::sync::Mutex::new(Vec::new()));
    let registry = ToolRegistry::new();
    registry.register(Box::new(TimedTool {
        name: "seq_a".to_string(),
        safe: false,
        start_times: start_times.clone(),
        sleep_ms: 150,
    }));
    registry.register(Box::new(TimedTool {
        name: "seq_b".to_string(),
        safe: false,
        start_times: start_times.clone(),
        sleep_ms: 150,
    }));

    let ctx = test_ctx();
    let agent = Agent {
        registry: &registry,
        ctx,
    };

    let tool_calls = vec![
        ToolCall {
            id: "a".to_string(),
            name: "seq_a".to_string(),
            input: serde_json::json!({}),
        },
        ToolCall {
            id: "b".to_string(),
            name: "seq_b".to_string(),
            input: serde_json::json!({}),
        },
    ];

    let start = std::time::Instant::now();
    let results = agent.execute_tools(&tool_calls).await;
    let elapsed = start.elapsed();

    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| !r.2), "no errors expected");

    // Sequential: must take at least 2 * 150ms = 300ms
    assert!(
        elapsed.as_millis() >= 280,
        "expected sequential execution (>= 280ms), got {}ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn test_execute_tools_preserves_original_order() {
    let counter = Arc::new(AtomicU64::new(0));
    struct OrderTool {
        name: String,
        order_counter: Arc<AtomicU64>,
        results: Arc<std::sync::Mutex<Vec<(String, u64)>>>,
    }

    #[async_trait]
    impl Tool for OrderTool {
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            "order test tool"
        }
        fn input_schema(&self) -> Value {
            serde_json::json!({"type": "object"})
        }
        fn is_concurrency_safe(&self) -> bool {
            true
        }
        async fn call(&self, _input: Value, _ctx: &ToolContext) -> ToolResult {
            let seq = self.order_counter.fetch_add(1, AtomicOrdering::SeqCst);
            self.results.lock().unwrap().push((self.name.clone(), seq));
            ToolResult::success(format!("seq={seq}"))
        }
    }

    let results = Arc::new(std::sync::Mutex::new(Vec::new()));
    let registry = ToolRegistry::new();
    registry.register(Box::new(OrderTool {
        name: "tool_c".to_string(),
        order_counter: counter.clone(),
        results: results.clone(),
    }));
    registry.register(Box::new(OrderTool {
        name: "tool_a".to_string(),
        order_counter: counter.clone(),
        results: results.clone(),
    }));
    registry.register(Box::new(OrderTool {
        name: "tool_b".to_string(),
        order_counter: counter.clone(),
        results: results.clone(),
    }));

    let ctx = test_ctx();
    let agent = Agent {
        registry: &registry,
        ctx,
    };

    // Pass calls in order: tool_c, tool_a, tool_b
    let tool_calls = vec![
        ToolCall {
            id: "1".to_string(),
            name: "tool_c".to_string(),
            input: serde_json::json!({}),
        },
        ToolCall {
            id: "2".to_string(),
            name: "tool_a".to_string(),
            input: serde_json::json!({}),
        },
        ToolCall {
            id: "3".to_string(),
            name: "tool_b".to_string(),
            input: serde_json::json!({}),
        },
    ];

    let exec_results = agent.execute_tools(&tool_calls).await;
    assert_eq!(exec_results.len(), 3);

    // Results should be in the original call order: tool_c, tool_a, tool_b
    assert_eq!(exec_results[0].0, "1"); // tool_c
    assert_eq!(exec_results[1].0, "2"); // tool_a
    assert_eq!(exec_results[2].0, "3"); // tool_b
}

#[tokio::test]
async fn test_execute_tools_mixed_concurrent_and_sequential() {
    let start_times = Arc::new(std::sync::Mutex::new(Vec::new()));
    let registry = ToolRegistry::new();
    registry.register(Box::new(TimedTool {
        name: "parallel".to_string(),
        safe: true,
        start_times: start_times.clone(),
        sleep_ms: 100,
    }));
    registry.register(Box::new(TimedTool {
        name: "sequential".to_string(),
        safe: false,
        start_times: start_times.clone(),
        sleep_ms: 100,
    }));

    let ctx = test_ctx();
    let agent = Agent {
        registry: &registry,
        ctx,
    };

    let tool_calls = vec![
        ToolCall {
            id: "p1".to_string(),
            name: "parallel".to_string(),
            input: serde_json::json!({}),
        },
        ToolCall {
            id: "s1".to_string(),
            name: "sequential".to_string(),
            input: serde_json::json!({}),
        },
        ToolCall {
            id: "p2".to_string(),
            name: "parallel".to_string(),
            input: serde_json::json!({}),
        },
    ];

    let results = agent.execute_tools(&tool_calls).await;
    assert_eq!(results.len(), 3);

    // Verify order is preserved: p1, s1, p2
    assert_eq!(results[0].0, "p1");
    assert_eq!(results[1].0, "s1");
    assert_eq!(results[2].0, "p2");
    assert!(results.iter().all(|r| !r.2), "no errors expected");
}
