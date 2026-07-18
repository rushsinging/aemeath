//! Tool 双端口（DDD §6.4.3）。
//!
//! 设计来源：`docs/design/02-modules/tools/02-ports-and-lifecycle.md`。
//!
//! # ToolCatalogPort
//!
//! 只读投影端口：根据 Registry Scope 与 Tool Profile 生成可见 Tool 快照。
//! 返回 `ToolDescriptor`，**禁止**返回 `Arc<dyn Tool>`、Registry handle 或
//! MCP client / transport。
//!
//! # ToolExecutionPort
//!
//! 执行端口：接收 `ToolInvocation`，在调用瞬间重新验证（Tool 存在、Profile 允许、
//! resources 可用、input 符合 schema），然后调用实际函数并标准化为 `ToolOutcome`。
//!
//! 故意保持单一 `ToolOutcome` 通道（含错误），避免调用方在 `Result::Err` 与
//! `ToolOutcome::Failure` 之间产生两套失败语义。

use async_trait::async_trait;

use super::published_language::{
    RegistryScopeName, ToolCatalogError, ToolCatalogSnapshot, ToolInvocation,
    ToolOutcome as ToolExecutionOutcome, ToolProfileName,
};

/// Tool Catalog 只读投影端口。
///
/// 消费方（Runtime）通过此端口获取可见工具快照，不接触 Registry 内部。
pub trait ToolCatalogPort: Send + Sync {
    /// 根据 Scope 和 Profile 生成当前可见 Tool 的只读快照。
    ///
    /// 保证：
    /// - ToolName 唯一、required resources 齐备、capabilities 被允许；
    /// - 组合 built-in 与 MCP Tool，但隐藏来源实现；
    /// - 返回的 `ToolDescriptor` 不包含 Tool 实例或函数指针。
    fn snapshot(
        &self,
        scope: &RegistryScopeName,
        profile: &ToolProfileName,
    ) -> Result<ToolCatalogSnapshot, ToolCatalogError>;
}

/// Tool 执行端口。
///
/// Runtime 通过此端口执行单个 Tool 调用。执行前重新验证 Tool 存在性、
/// Profile 权限、resources 可用性和 input schema 合法性。
///
/// Policy、Hook、人工审批、timeout 和跨 Tool 并发不得下沉进此端口。
#[async_trait]
pub trait ToolExecutionPort: Send + Sync {
    /// 执行一次 Tool 调用。
    ///
    /// 返回 `ToolOutcome`（单一通道，含错误）。Tool 不存在返回
    /// `Failure(ToolUnavailable)`；schema 失败返回 `Failure(InvalidInput)`。
    async fn execute(&self, invocation: ToolInvocation) -> ToolExecutionOutcome;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::published_language::*;
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        let waker = std::task::Waker::noop();
        let mut context = std::task::Context::from_waker(waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                std::task::Poll::Ready(output) => return output,
                std::task::Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    /// 用于测试的 Fake ToolCatalogPort。
    struct FakeCatalogPort {
        snapshot: ToolCatalogSnapshot,
    }

    impl ToolCatalogPort for FakeCatalogPort {
        fn snapshot(
            &self,
            _scope: &RegistryScopeName,
            _profile: &ToolProfileName,
        ) -> Result<ToolCatalogSnapshot, ToolCatalogError> {
            Ok(self.snapshot.clone())
        }
    }

    #[test]
    fn test_catalog_port_returns_snapshot_without_tool_instances() {
        let desc = ToolDescriptor {
            name: ToolName::new("Read"),
            description: "Read tool".into(),
            input_schema: serde_json::json!({"type": "object"}),
            required_capabilities: ToolCapabilities::ReadWorkspace,
            concurrency: ConcurrencyDeclaration::safe(),
            cancellation: CancellationDeclaration::Cooperative,
        };
        let snapshot = ToolCatalogSnapshot::new("main", "full", vec![desc]);
        let port = FakeCatalogPort { snapshot };

        let result = port.snapshot(
            &RegistryScopeName::new("main"),
            &ToolProfileName::new("full"),
        );
        assert!(result.is_ok());
        let snap = result.unwrap();
        assert_eq!(snap.len(), 1);
        assert!(snap.find(&ToolName::new("read")).is_some());
    }

    /// 用于测试的 Fake ToolExecutionPort。
    struct FakeExecutionPort {
        outcomes: Arc<Mutex<Vec<ToolOutcome>>>,
    }

    #[async_trait]
    impl ToolExecutionPort for FakeExecutionPort {
        async fn execute(&self, invocation: ToolInvocation) -> ToolOutcome {
            let mut outcomes = self.outcomes.lock();
            if outcomes.is_empty() {
                if invocation.tool_name == ToolName::new("Read") {
                    return ToolOutcome::success_text("file content");
                }
                return ToolOutcome::failure(ToolErrorKind::ToolUnavailable, "unknown tool");
            }
            outcomes.remove(0)
        }
    }

    #[test]
    fn test_execution_port_known_tool_returns_success() {
        let port = FakeExecutionPort {
            outcomes: Arc::new(Mutex::new(vec![])),
        };
        let inv = ToolInvocation::new("Read", serde_json::json!({"path": "/tmp"}));
        let outcome = block_on(port.execute(inv));
        assert!(outcome.is_success());
    }

    #[test]
    fn test_execution_port_unknown_tool_returns_unavailable() {
        let port = FakeExecutionPort {
            outcomes: Arc::new(Mutex::new(vec![])),
        };
        let inv = ToolInvocation::new("NonExistent", serde_json::json!({}));
        let outcome = block_on(port.execute(inv));
        assert!(outcome.is_failure());
        match outcome {
            ToolOutcome::Failure(f) => assert_eq!(f.kind, ToolErrorKind::ToolUnavailable),
            _ => panic!("应为 Failure"),
        }
    }

    #[test]
    fn test_execution_port_queued_outcome() {
        let port = FakeExecutionPort {
            outcomes: Arc::new(Mutex::new(vec![ToolOutcome::failure(
                ToolErrorKind::InvalidInput,
                "bad schema",
            )])),
        };
        let inv = ToolInvocation::new("Read", serde_json::json!({}));
        let outcome = block_on(port.execute(inv));
        assert!(outcome.is_failure());
        match outcome {
            ToolOutcome::Failure(f) => {
                assert_eq!(f.kind, ToolErrorKind::InvalidInput);
                assert_eq!(f.safe_message, "bad schema");
            }
            _ => panic!("应为 Failure"),
        }
    }
}
