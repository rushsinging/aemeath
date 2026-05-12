pub(super) fn enforce_openai_tool_pairs(messages: &mut Vec<serde_json::Value>) {
    use std::collections::HashSet;

    // Step 1: 收集所有 assistant 发出过的 tool_call_id
    let mut all_call_ids: HashSet<String> = HashSet::new();
    for m in messages.iter() {
        if m.get("role").and_then(|r| r.as_str()) == Some("assistant") {
            if let Some(tcs) = m.get("tool_calls").and_then(|t| t.as_array()) {
                for tc in tcs {
                    if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                        all_call_ids.insert(id.to_string());
                    }
                }
            }
        }
    }

    // Step 2: 移除孤儿 tool 消息（其 tool_call_id 不属于任何 assistant.tool_calls）
    let before = messages.len();
    messages.retain(|m| {
        if m.get("role").and_then(|r| r.as_str()) == Some("tool") {
            let tcid = m.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or("");
            if !all_call_ids.contains(tcid) {
                log::warn!("[openai-compat] dropping orphan tool message id={}", tcid);
                return false;
            }
        }
        true
    });
    if messages.len() != before {
        log::warn!(
            "[openai-compat] dropped {} orphan tool messages",
            before - messages.len()
        );
    }

    // Step 3: 对每条带 tool_calls 的 assistant，检查紧跟的 tool messages 是否覆盖全部 id；
    //         缺哪个就立即在它后面插入占位
    let mut i = 0;
    while i < messages.len() {
        let pending: Vec<String> = messages[i]
            .get("role")
            .and_then(|r| r.as_str())
            .filter(|r| *r == "assistant")
            .and_then(|_| messages[i].get("tool_calls").and_then(|t| t.as_array()))
            .map(|tcs| {
                tcs.iter()
                    .filter_map(|tc| tc.get("id").and_then(|i| i.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        if pending.is_empty() {
            i += 1;
            continue;
        }

        // 收集紧跟 i 的连续 tool messages 已覆盖哪些 id
        let mut covered: HashSet<String> = HashSet::new();
        let mut last_tool_idx = i;
        let mut j = i + 1;
        while j < messages.len() && messages[j].get("role").and_then(|r| r.as_str()) == Some("tool")
        {
            if let Some(id) = messages[j].get("tool_call_id").and_then(|v| v.as_str()) {
                covered.insert(id.to_string());
            }
            last_tool_idx = j;
            j += 1;
        }

        // 缺失的 id：插入占位 tool 消息
        let missing: Vec<&String> = pending.iter().filter(|id| !covered.contains(*id)).collect();
        if !missing.is_empty() {
            log::warn!(
                "[openai-compat] assistant at index {} has {} tool_calls but only {} are answered. Inserting {} placeholder tool message(s).",
                i, pending.len(), covered.len(), missing.len()
            );
            let insert_after = last_tool_idx;
            for (offset, mid) in missing.iter().enumerate() {
                messages.insert(
                    insert_after + 1 + offset,
                    serde_json::json!({
                        "role": "tool",
                        "tool_call_id": mid,
                        "content": "[result missing — auto-filled to satisfy tool_calls schema]"
                    }),
                );
            }
            i = insert_after + 1 + missing.len();
        } else {
            i = last_tool_idx + 1;
        }
    }
}
