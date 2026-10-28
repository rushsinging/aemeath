//! session dataset 读取：把 content-addressed 存储还原成压缩输入快照。
//!
//! 结构（与 runtime 落盘一致）：
//!   `<session>.dataset/primary/manifest.json`   成员名 → 内容摘要
//!   `<session>.dataset/members/<内容摘要>`       实际 JSON blob
//!
//! 本模块只读，不修改任何文件。

use std::path::{Path, PathBuf};

use serde_json::Value;

#[derive(Debug, Default)]
pub(crate) struct SessionSnapshot {
    pub(crate) session_id: String,
    pub(crate) model: Option<String>,
    pub(crate) previous_summary: Option<String>,
    pub(crate) committed_step_count: usize,
    pub(crate) messages: Vec<Value>,
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("读取失败 {}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("解析失败 {}: {error}", path.display()))
}

fn member_index(dataset: &Path) -> Result<std::collections::BTreeMap<String, String>, String> {
    let manifest = read_json(&dataset.join("primary/manifest.json"))?;
    let evidence = manifest
        .get("成员证据")
        .and_then(Value::as_array)
        .ok_or_else(|| "manifest 缺少「成员证据」".to_string())?;
    let mut index = std::collections::BTreeMap::new();
    for entry in evidence {
        let (Some(name), Some(digest)) = (
            entry.get("名称").and_then(Value::as_str),
            entry.get("内容摘要").and_then(Value::as_str),
        ) else {
            continue;
        };
        index.insert(name.to_string(), digest.to_string());
    }
    Ok(index)
}

fn load_blob(
    dataset: &Path,
    index: &std::collections::BTreeMap<String, String>,
    name: &str,
) -> Result<Option<Value>, String> {
    let Some(digest) = index.get(name) else {
        return Ok(None);
    };
    read_json(&dataset.join("members").join(digest)).map(Some)
}

/// 与 runtime 的 step 成员命名保持一致（hex 编码）。
pub(crate) fn step_member_name(run_id: &str, step_id: &str) -> String {
    format!("step-{}-{}.json", hex(run_id), hex(step_id))
}

fn hex(value: &str) -> String {
    value.bytes().map(|byte| format!("{byte:02x}")).collect()
}

/// 读取 session：按 committed steps 顺序还原消息序列。
pub(crate) fn load_session(dataset: &Path) -> Result<SessionSnapshot, String> {
    let index = member_index(dataset)?;
    let state = load_blob(dataset, &index, "session-state.json")?
        .ok_or_else(|| "dataset 缺少 session-state.json".to_string())?;
    let metadata = load_blob(dataset, &index, "metadata.json")?.unwrap_or(Value::Null);

    let committed = state
        .get("committed_steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut messages = Vec::new();
    for reference in &committed {
        let (Some(run_id), Some(step_id)) = (
            reference.get("run_id").and_then(Value::as_str),
            reference.get("step_id").and_then(Value::as_str),
        ) else {
            continue;
        };
        let name = step_member_name(run_id, step_id);
        let Some(blob) = load_blob(dataset, &index, &name)? else {
            continue;
        };
        let Some(outcome) = blob.get("step").and_then(|step| step.get("outcome")) else {
            continue;
        };
        if let Some(items) = outcome.get("messages").and_then(Value::as_array) {
            messages.extend(items.iter().cloned());
        }
    }

    let session_id = metadata
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| state.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| {
            dataset
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_default()
        });
    let model = metadata
        .get("metadata")
        .and_then(|value| value.get("model"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let previous_summary = state
        .get("compact")
        .and_then(|value| value.get("summary"))
        .and_then(Value::as_str)
        .map(str::to_string);

    Ok(SessionSnapshot {
        session_id,
        model,
        previous_summary,
        committed_step_count: committed.len(),
        messages,
    })
}

/// 列出 `<root>/*/*.dataset` 中带 manifest 的目录。
pub(crate) fn iter_datasets(root: &Path) -> Vec<PathBuf> {
    let Ok(outer) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut datasets = Vec::new();
    for entry in outer.flatten() {
        let Ok(inner) = std::fs::read_dir(entry.path()) else {
            continue;
        };
        for candidate in inner.flatten() {
            let path = candidate.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("dataset") {
                continue;
            }
            if path.join("primary/manifest.json").is_file() {
                datasets.push(path);
            }
        }
    }
    datasets.sort();
    datasets
}

/// 与 runtime 估算口径对齐：CJK 字符 ×0.6，其余 ×0.3（向上取整）。
pub(crate) fn estimate_tokens(text: &str) -> u64 {
    let mut cjk = 0u64;
    let mut other = 0u64;
    for char in text.chars() {
        let code = char as u32;
        if matches!(code, 0x3000..=0x303F | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
        {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    (cjk as f64 * 0.6 + other as f64 * 0.3).ceil() as u64
}

/// 把一条消息的 content blocks 摊平成文本。
pub(crate) fn message_text(message: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(blocks) = message.get("content").and_then(Value::as_array) {
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") | Some("thinking") => {
                    if let Some(text) = block
                        .get("text")
                        .or_else(|| block.get("thinking"))
                        .and_then(Value::as_str)
                    {
                        parts.push(text.to_string());
                    }
                }
                Some("tool_use") => {
                    if let Some(input) = block.get("input") {
                        parts.push(input.to_string());
                    }
                    if let Some(name) = block.get("name").and_then(Value::as_str) {
                        parts.push(name.to_string());
                    }
                }
                Some("tool_result") => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        parts.push(text.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    parts.join("\n")
}

#[cfg(test)]
#[path = "dataset_tests.rs"]
mod dataset_tests;
