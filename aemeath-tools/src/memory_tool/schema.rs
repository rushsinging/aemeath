use serde_json::Value;

pub(super) fn input_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["add", "delete", "search", "pin", "list", "add_reminder", "complete_reminder"],
                "description": "Memory action to perform"
            },
            "id": { "type": "string", "description": "Memory id for delete/pin actions" },
            "content": { "type": "string", "description": "Memory content, max 500 chars" },
            "query": { "type": "string", "description": "Search query" },
            "limit": { "type": "integer", "description": "Maximum number of results" },
            "layer": {
                "type": "string",
                "enum": ["global", "project"],
                "description": "Memory layer"
            },
            "category": {
                "type": "string",
                "enum": ["fact", "decision", "preference", "pattern", "pitfall"],
                "description": "Memory category"
            },
            "tags": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional tags"
            },
            "pinned": { "type": "boolean", "description": "Whether to pin the memory" },
            "priority": {
                "type": "string",
                "enum": ["low", "normal", "high"],
                "description": "Reminder priority"
            }
        },
        "required": ["action"]
    })
}
