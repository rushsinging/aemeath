use super::super::registry::lookup_display;
use serde_json::json;

fn card(name: &str, input: serde_json::Value, result: serde_json::Value) {
    let d = lookup_display(name).unwrap();
    let header = d.format_header(&input, None);
    let lines = d.format_result_lines(Some(&result)).unwrap();
    println!("[{name}] header = {header:?}");
    println!("[{name}] last   = {:?}", lines.last().unwrap());
    println!("[{name}] all    = {lines:?}");
}

#[test]
fn probe_four_real_cards() {
    card(
        "workflow_create",
        json!({"name":"显示验收","objective":"验证卡片渲染"}),
        json!({"workflow_id":"01a19289-4867-7923-9c86-53ef2a41a1a7","status":"active","active_mode":"executing","root_node_id":"root","complete_node_id":"done","node_count":2}),
    );
    card(
        "workflow_query",
        json!({"workflow_id":"01a19289-4867-7923-9c86-53ef2a41a1a7"}),
        json!({"workflow_id":"01a19289-4867-7923-9c86-53ef2a41a1a7","name":"显示验收","objective":"验证卡片渲染","status":"active","active_mode":"executing","pending_mode":null,"root_node_id":"root","complete_node_id":"done","nodes":[{"node_id":"root","kind":"work","role":"implementer","status":"active","description":"执行验收工作","completion_criteria":null,"conclusion":null,"evidence_refs":[],"successors":[{"node_id":"done","description":"完成节点"}]},{"node_id":"done","kind":"complete","role":null,"status":"pending","description":"完成节点","completion_criteria":null,"conclusion":null,"evidence_refs":[],"successors":[]}],"edges":[{"from_node_id":"root","to_node_id":"done"}],"facts":[],"allowed_actions":["deactivate","complete","cancel","node_complete","node_fail"]}),
    );
    card(
        "workflow_node_update",
        json!({"workflow_id":"01a19289-4867-7923-9c86-53ef2a41a1a7","node_id":"root","action":"fail","conclusion":"卡片验收失败路径"}),
        json!({"workflow_id":"01a19289-4867-7923-9c86-53ef2a41a1a7","node_id":"root","node_status":"failed","status":"active","active_mode":"replanning","activated_node_ids":[],"stopped_node_ids":[]}),
    );
    card(
        "workflow_update",
        json!({"workflow_id":"01a19289-4867-7923-9c86-53ef2a41a1a7","action":"fail","summary":"结束"}),
        json!({"workflow_id":"01a19289-4867-7923-9c86-53ef2a41a1a7","status":"failed","active_mode":null,"pending_mode":null}),
    );
}
