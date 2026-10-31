//! #2084 协议与 adapter 行为测试（覆盖设计文档 §11.2 G 组协议部分）。

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;

use super::*;
use workflow::{
    ActiveMode, AllowedAction, CreateWorkflowSpec, EvidenceKind, EvidenceRef, FactSummaryData,
    LifecycleAction, LifecycleResultData, ListWorkflowsData, ListWorkflowsQuery, NodeAction,
    NodeKind, NodeStatus, NodeSummaryCounts, NodeUpdateResultData, NodeViewData,
    QueryWorkflowQuery, ReplanResultData, ReplanWorkflowCommand, SuccessorDecision,
    UpdateNodeCommand, UpdateWorkflowCommand, WorkflowDetailData, WorkflowStatus,
    WorkflowSummaryData, WorkflowToolError,
};

/// 记录调用并返回可配置结果的测试 backend。
#[derive(Default)]
struct MockBackend {
    create_spec: Mutex<Option<CreateWorkflowSpec>>,
    node_command: Mutex<Option<UpdateNodeCommand>>,
    update_command: Mutex<Option<UpdateWorkflowCommand>>,
    list_query: Mutex<Option<ListWorkflowsQuery>>,
    fail_with: Option<WorkflowToolError>,
}

fn summary() -> WorkflowSummaryData {
    WorkflowSummaryData {
        workflow_id: "wf_1".to_owned(),
        name: "design".to_owned(),
        status: WorkflowStatus::Active,
        active_mode: Some(ActiveMode::Executing),
        pending_mode: None,
        node_summary: NodeSummaryCounts {
            active: 1,
            pending: 1,
            ..Default::default()
        },
        updated_at: "2026-10-28T00:00:00Z".to_owned(),
    }
}

#[async_trait]
impl WorkflowToolBackend for MockBackend {
    async fn create(
        &self,
        spec: CreateWorkflowSpec,
    ) -> Result<WorkflowSummaryData, WorkflowToolError> {
        *self.create_spec.lock().unwrap() = Some(spec);
        if let Some(error) = &self.fail_with {
            return Err(error.clone());
        }
        Ok(summary())
    }

    async fn list(
        &self,
        query: ListWorkflowsQuery,
    ) -> Result<ListWorkflowsData, WorkflowToolError> {
        let limit = query.limit;
        let offset = query.offset;
        *self.list_query.lock().unwrap() = Some(query);
        Ok(ListWorkflowsData {
            workflows: vec![summary()],
            total: 1,
            limit,
            offset,
        })
    }

    async fn query(
        &self,
        query: QueryWorkflowQuery,
    ) -> Result<WorkflowDetailData, WorkflowToolError> {
        Ok(WorkflowDetailData {
            workflow_id: query.workflow_id,
            name: "design".to_owned(),
            objective: "ship".to_owned(),
            status: WorkflowStatus::Active,
            active_mode: Some(ActiveMode::Executing),
            pending_mode: None,
            root_node_id: "root".to_owned(),
            complete_node_id: "done".to_owned(),
            nodes: vec![NodeViewData {
                node_id: "root".to_owned(),
                kind: NodeKind::Work,
                role: Some(workflow::Role::Planner),
                status: NodeStatus::Active,
                description: "plan".to_owned(),
                completion_criteria: None,
                conclusion: None,
                evidence_refs: vec![EvidenceRef {
                    fact_id: "fact_1".to_owned(),
                    kind: EvidenceKind::ToolResult,
                }],
                successors: vec![SuccessorDecision {
                    node_id: "done".to_owned(),
                    description: "finish".to_owned(),
                }],
            }],
            edges: vec![workflow::EdgeDefinition {
                from_node_id: "root".to_owned(),
                to_node_id: "done".to_owned(),
            }],
            facts: vec![FactSummaryData {
                fact_id: "fact_1".to_owned(),
                kind: EvidenceKind::ToolResult,
                source: "Bash".to_owned(),
                related_node_ids: vec!["root".to_owned()],
                at: "2026-10-28T00:00:00Z".to_owned(),
            }],
            allowed_actions: vec![
                AllowedAction::Deactivate,
                AllowedAction::NodeComplete,
                AllowedAction::NodeFail,
            ],
        })
    }

    async fn update(
        &self,
        command: UpdateWorkflowCommand,
    ) -> Result<LifecycleResultData, WorkflowToolError> {
        *self.update_command.lock().unwrap() = Some(command);
        Ok(LifecycleResultData {
            workflow_id: "wf_1".to_owned(),
            status: WorkflowStatus::Inactive,
            active_mode: None,
            pending_mode: None,
        })
    }

    async fn node_update(
        &self,
        command: UpdateNodeCommand,
    ) -> Result<NodeUpdateResultData, WorkflowToolError> {
        *self.node_command.lock().unwrap() = Some(command);
        Ok(NodeUpdateResultData {
            workflow_id: "wf_1".to_owned(),
            node_id: "root".to_owned(),
            node_status: NodeStatus::Completed,
            status: WorkflowStatus::Active,
            active_mode: Some(ActiveMode::Executing),
            activated_node_ids: vec!["done".to_owned()],
            stopped_node_ids: Vec::new(),
            replan_boundary: None,
        })
    }

    async fn replan(
        &self,
        _command: ReplanWorkflowCommand,
    ) -> Result<ReplanResultData, WorkflowToolError> {
        Ok(ReplanResultData {
            workflow_id: "wf_1".to_owned(),
            status: WorkflowStatus::Active,
            active_mode: Some(ActiveMode::Executing),
            activated_node_ids: vec!["fix".to_owned()],
            applied_node_ids: vec!["fix".to_owned()],
            retired_pending_node_ids: Vec::new(),
        })
    }
}

fn context() -> crate::domain::ToolExecutionContext {
    let workspace = tempfile::tempdir().expect("workspace");
    crate::domain::test_support::TestToolExecutionContextBuilder::new(
        workspace.path().to_path_buf(),
    )
    .build()
}

fn create_input() -> serde_json::Value {
    json!({
        "name": "design",
        "objective": "ship",
        "root_index": 0,
        "nodes": [
            { "index": 0, "kind": "work", "role": "planner", "description": "plan" },
            { "index": 1, "kind": "complete", "description": "finish" }
        ],
        "edges": [{ "from_index": 0, "to_index": 1 }]
    })
}

/// 六工具的 input / data schema 必须暴露且关键字段在场。
#[test]
fn workflow_tool_schemas_exposed() {
    let backend = Arc::new(MockBackend::default());

    let create_schema = WorkflowCreateTool {
        backend: backend.clone(),
    }
    .input_schema();
    for field in ["name", "objective", "root_index", "nodes", "edges"] {
        assert!(
            create_schema["properties"].get(field).is_some(),
            "create input schema missing {field}: {create_schema}"
        );
    }
    let create_data = WorkflowCreateTool {
        backend: backend.clone(),
    }
    .data_schema();
    assert!(create_data["properties"].get("workflow_id").is_some());

    let list_schema = WorkflowListTool {
        backend: backend.clone(),
    }
    .input_schema();
    for field in ["status", "limit", "offset"] {
        assert!(
            list_schema["properties"].get(field).is_some(),
            "list schema missing {field}"
        );
    }

    let query_schema = WorkflowQueryTool {
        backend: backend.clone(),
    }
    .input_schema();
    for field in ["workflow_id", "include_facts"] {
        assert!(
            query_schema["properties"].get(field).is_some(),
            "query schema missing {field}"
        );
    }

    let update_schema = WorkflowUpdateTool {
        backend: backend.clone(),
    }
    .input_schema();
    for field in ["workflow_id", "action", "summary", "evidence_refs"] {
        assert!(
            update_schema["properties"].get(field).is_some(),
            "update schema missing {field}"
        );
    }

    let node_schema = WorkflowNodeUpdateTool {
        backend: backend.clone(),
    }
    .input_schema();
    for field in [
        "workflow_id",
        "node_id",
        "action",
        "conclusion",
        "evidence_refs",
        "successors",
    ] {
        assert!(
            node_schema["properties"].get(field).is_some(),
            "node schema missing {field}"
        );
    }

    let replan_schema = WorkflowReplanTool {
        backend: backend.clone(),
    }
    .input_schema();
    for field in [
        "workflow_id",
        "boundary",
        "new_nodes",
        "new_edges",
        "entry_indexes",
        "rejoin_edges",
        "pending_rewrites",
    ] {
        assert!(
            replan_schema["properties"].get(field).is_some(),
            "replan schema missing {field}"
        );
    }

    let replan_data = WorkflowReplanTool { backend }.data_schema();
    assert!(replan_data["properties"]
        .get("retired_pending_node_ids")
        .is_some());
}

/// create：协议输入映射为领域 spec，结果回填协议字段。
#[tokio::test]
async fn workflow_create_maps_input_and_result() {
    let backend = Arc::new(MockBackend::default());
    let tool = WorkflowCreateTool {
        backend: backend.clone(),
    };
    let result = tool.call(create_input(), &context()).await;
    assert!(!result.is_error, "create must succeed: {}", result.text);

    let spec = backend
        .create_spec
        .lock()
        .unwrap()
        .clone()
        .expect("spec recorded");
    assert!(
        spec.root_node_id.starts_with("wnd_"),
        "{}",
        spec.root_node_id
    );
    assert_eq!(spec.nodes.len(), 2);
    assert_eq!(spec.nodes[0].kind, NodeKind::Work);
    assert_eq!(spec.nodes[0].role, Some(workflow::Role::Planner));
    assert_eq!(spec.nodes[1].kind, NodeKind::Complete);
    assert_eq!(spec.edges.len(), 1);

    let data = result.data.expect("data");
    assert_eq!(data.workflow_id, "wf_1");
    assert_eq!(data.status, "active");
    assert!(data.root_node_id.starts_with("wnd_"));
    assert!(data.complete_node_id.starts_with("wnd_"));
    assert_eq!(data.node_ids.len(), 2);
    assert_eq!(data.node_count, 2);
}

/// 输入缺必填字段时返回协议错误，不调用 backend。
#[tokio::test]
async fn workflow_create_rejects_invalid_input() {
    let backend = Arc::new(MockBackend::default());
    let tool = WorkflowCreateTool {
        backend: backend.clone(),
    };
    let result = tool.call(json!({}), &context()).await;
    assert!(result.is_error);
    assert!(backend.create_spec.lock().unwrap().is_none());
}

/// list：默认分页与上限收敛。
#[tokio::test]
async fn workflow_list_applies_pagination_defaults() {
    let backend = Arc::new(MockBackend::default());
    let tool = WorkflowListTool {
        backend: backend.clone(),
    };
    let result = tool.call(json!({}), &context()).await;
    assert!(!result.is_error, "list must succeed: {}", result.text);
    let query = backend
        .list_query
        .lock()
        .unwrap()
        .clone()
        .expect("query recorded");
    assert_eq!(query.limit, 20);
    assert_eq!(query.offset, 0);
    assert_eq!(query.status, None);

    let _ = tool.call(json!({ "limit": 500 }), &context()).await;
    let query = backend
        .list_query
        .lock()
        .unwrap()
        .clone()
        .expect("query recorded");
    assert_eq!(query.limit, 100, "limit must be clamped to 100");
}

/// query：状态与 role 以字符串标签返回，allowed_actions 完整透传。
#[tokio::test]
async fn workflow_query_maps_labels_and_actions() {
    let backend = Arc::new(MockBackend::default());
    let tool = WorkflowQueryTool { backend };
    let result = tool
        .call(
            json!({ "workflow_id": "wf_1", "include_facts": true }),
            &context(),
        )
        .await;
    assert!(!result.is_error, "query must succeed: {}", result.text);
    let data = result.data.expect("data");
    assert_eq!(data.status, "active");
    assert_eq!(data.active_mode.as_deref(), Some("executing"));
    assert_eq!(data.nodes[0].kind, "work");
    assert_eq!(data.nodes[0].role.as_deref(), Some("planner"));
    assert_eq!(data.nodes[0].status, "active");
    assert_eq!(data.nodes[0].evidence_refs[0].fact_id, "fact_1");
    assert_eq!(data.nodes[0].successors[0].node_id, "done");
    assert_eq!(data.facts[0].kind, "tool_result");
    assert_eq!(
        data.allowed_actions,
        vec![
            "deactivate".to_owned(),
            "node_complete".to_owned(),
            "node_fail".to_owned()
        ]
    );
}

/// update：action 与 evidence 映射到领域命令。
#[tokio::test]
async fn workflow_update_maps_action_and_evidence() {
    let backend = Arc::new(MockBackend::default());
    let tool = WorkflowUpdateTool {
        backend: backend.clone(),
    };
    let result = tool
        .call(
            json!({
                "workflow_id": "wf_1",
                "action": "deactivate",
                "evidence_refs": [{ "fact_id": "fact_9", "kind": "user_message" }]
            }),
            &context(),
        )
        .await;
    assert!(!result.is_error, "update must succeed: {}", result.text);
    let command = backend
        .update_command
        .lock()
        .unwrap()
        .clone()
        .expect("command recorded");
    assert_eq!(command.action, LifecycleAction::Deactivate);
    assert_eq!(command.summary, None);
    assert_eq!(command.evidence_refs.len(), 1);
    assert_eq!(command.evidence_refs[0].kind, EvidenceKind::UserMessage);

    let data = result.data.expect("data");
    assert_eq!(data.status, "inactive");
    assert_eq!(data.active_mode, None);
}

/// node_update：conclusion / successors / evidence 全量映射。
#[tokio::test]
async fn workflow_node_update_maps_full_command() {
    let backend = Arc::new(MockBackend::default());
    let tool = WorkflowNodeUpdateTool {
        backend: backend.clone(),
    };
    let result = tool
        .call(
            json!({
                "workflow_id": "wf_1",
                "node_id": "root",
                "action": "complete",
                "conclusion": "plan ready",
                "evidence_refs": [{ "fact_id": "fact_2", "kind": "sub_agent_output" }],
                "successors": [{ "node_id": "done", "description": "finish now" }]
            }),
            &context(),
        )
        .await;
    assert!(
        !result.is_error,
        "node_update must succeed: {}",
        result.text
    );
    let command = backend
        .node_command
        .lock()
        .unwrap()
        .clone()
        .expect("command recorded");
    assert_eq!(command.action, NodeAction::Complete);
    assert_eq!(command.conclusion, "plan ready");
    assert_eq!(command.evidence_refs[0].kind, EvidenceKind::SubAgentOutput);
    assert_eq!(command.successors[0].description, "finish now");

    let data = result.data.expect("data");
    assert_eq!(data.node_status, "completed");
    assert_eq!(data.activated_node_ids, vec!["done".to_owned()]);
    assert!(data.stopped_node_ids.is_empty());
}

/// 业务错误以结构化 JSON 文本透传，不使用粗分类。
#[tokio::test]
async fn business_error_is_structured_json() {
    let backend = Arc::new(MockBackend {
        fail_with: Some(WorkflowToolError::WorkflowNotActive {
            workflow_id: "wf_1".to_owned(),
        }),
        ..Default::default()
    });
    let tool = WorkflowCreateTool { backend };
    let result = tool.call(create_input(), &context()).await;
    assert!(result.is_error);
    let payload: serde_json::Value =
        serde_json::from_str(&result.text).expect("error text must be JSON");
    assert_eq!(payload["kind"], "workflow_not_active");
    assert_eq!(payload["workflow_id"], "wf_1");
}

/// 13 个 typed business error 全部可序列化往返。
#[test]
fn all_business_errors_round_trip() {
    let errors = vec![
        WorkflowToolError::ActiveWorkflowAlreadyExists,
        WorkflowToolError::WorkflowNotActive {
            workflow_id: "wf".to_owned(),
        },
        WorkflowToolError::WorkflowAlreadyTerminal {
            workflow_id: "wf".to_owned(),
        },
        WorkflowToolError::WorkflowReplanning {
            workflow_id: "wf".to_owned(),
        },
        WorkflowToolError::WorkflowNotReplanning {
            workflow_id: "wf".to_owned(),
        },
        WorkflowToolError::WorkflowCannotContinue {
            workflow_id: "wf".to_owned(),
        },
        WorkflowToolError::NodeNotActive {
            node_id: "n1".to_owned(),
        },
        WorkflowToolError::NodeHistoryFrozen {
            node_id: "n1".to_owned(),
        },
        WorkflowToolError::InvalidSuccessor {
            subject: "n2".to_owned(),
        },
        WorkflowToolError::InvalidEvidenceReference,
        WorkflowToolError::GraphInvariantViolation {
            reason: workflow::GraphInvariantReason::MultipleRoots,
            subject: None,
        },
        WorkflowToolError::CompletionConditionsUnsatisfied {
            subject: "done".to_owned(),
        },
        WorkflowToolError::WorkflowNotVisible {
            workflow_id: "wf".to_owned(),
        },
    ];
    assert_eq!(errors.len(), 13, "MVP 完整错误枚举");
    for error in errors {
        let text = serde_json::to_string(&error).expect("serialize");
        let back: WorkflowToolError = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, error, "round trip for {text}");
    }
    // EvidenceNotFound 不得存在。
    let text = serde_json::to_string(&WorkflowToolError::InvalidEvidenceReference).unwrap();
    assert!(!text.contains("evidence_not_found"));
}
