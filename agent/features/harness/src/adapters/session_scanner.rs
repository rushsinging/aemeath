//! session dataset 扫描 adapter（#2048，设计 04 分片 §2A.1/2A.2）。
//!
//! 从 `~/.agents/session/<id>.dataset/members/*.json` 的 step 记录提取
//! 扁平事件流（`SessionEventRecord`），交 domain 聚合。只读落盘，
//! NEVER 依赖主进程内存态；流式逐成员解析，NEVER 全量载入。

use crate::constants::EVIDENCE_EXCERPT_BOUND;
use crate::domain::evidence::SessionEventRecord;

/// 轻量提取 dataset 的 run id 清单（有序去重；只读 cursor，NEVER 全量解析成员体）。
pub fn dataset_run_ids(dataset_dir: &std::path::Path) -> Vec<String> {
    let mut run_ids: Vec<String> = Vec::new();
    let Ok(entries) = std::fs::read_dir(dataset_dir.join("members")) else {
        return run_ids;
    };
    for entry in entries.flatten() {
        let Ok(raw) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(member) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        if let Some(run_id) = member.pointer("/cursor/run_id").and_then(|v| v.as_str()) {
            if !run_ids.iter().any(|existing| existing == run_id) {
                run_ids.push(run_id.to_string());
            }
        }
    }
    run_ids
}

/// 扫描单个 session dataset 目录，产出事件流（成员损坏跳过并计数）。
pub struct SessionDatasetScanner;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub events: Vec<SessionEventRecord>,
    /// 解析失败被跳过的成员数（NEVER 让单成员损坏中断扫描）。
    pub skipped_members: usize,
}

impl SessionDatasetScanner {
    pub fn scan_dataset(dataset_dir: &std::path::Path) -> ScanReport {
        let mut report = ScanReport::default();
        let Ok(entries) = std::fs::read_dir(dataset_dir.join("members")) else {
            return report;
        };
        let session_id = dataset_dir
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("unknown")
            .trim_end_matches(".dataset")
            .to_string();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&path) else {
                report.skipped_members += 1;
                continue;
            };
            let Ok(member) = serde_json::from_str::<serde_json::Value>(&raw) else {
                report.skipped_members += 1;
                continue;
            };
            let Some(step) = member.get("step") else {
                // 非 step 成员（manifest/state/metadata）——不是事件来源
                continue;
            };
            let run_id = member
                .pointer("/cursor/run_id")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_string();
            Self::extract_step_events(&session_id, &run_id, step, &mut report.events);
        }
        report
    }

    fn extract_step_events(
        session_id: &str,
        run_id: &str,
        step: &serde_json::Value,
        events: &mut Vec<SessionEventRecord>,
    ) {
        // 工具事件：outcome.messages 中 tool_use / tool_result 按 id 配对
        let Some(messages) = step.pointer("/outcome/messages").and_then(|v| v.as_array()) else {
            return;
        };
        let mut tool_inputs: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();
        for message in messages {
            let Some(blocks) = message.get("content").and_then(|v| v.as_array()) else {
                continue;
            };
            for block in blocks {
                match block.get("type").and_then(|v| v.as_str()) {
                    Some("tool_use") => {
                        let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let name = block
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let input_fingerprint = fingerprint_input(block.get("input"));
                        tool_inputs.insert(id.to_string(), (name.to_string(), input_fingerprint));
                    }
                    Some("tool_result") => {
                        let call_id = block
                            .get("tool_use_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let (tool_name, fingerprint) = tool_inputs
                            .get(call_id)
                            .cloned()
                            .unwrap_or_else(|| ("unknown".to_string(), String::new()));
                        let is_error = block
                            .get("is_error")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let text = block.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        let error_text = if is_error {
                            Some(truncate_error_text(text))
                        } else {
                            None
                        };
                        events.push(SessionEventRecord {
                            session_id: session_id.to_string(),
                            run_id: run_id.to_string(),
                            tool_name: Some(tool_name),
                            is_error,
                            error_text,
                            result_chars: text.chars().count(),
                            call_fingerprint: if fingerprint.is_empty() {
                                None
                            } else {
                                Some(fingerprint)
                            },
                        });
                    }
                    _ => {}
                }
            }
        }
        // run 级事件：finalize_cause 作为终态（每 step 一条，domain 聚合去重计分布）
        if let Some(finalize_cause) = step
            .pointer("/outcome/finalize_cause")
            .and_then(|v| v.as_str())
        {
            events.push(SessionEventRecord {
                session_id: session_id.to_string(),
                run_id: run_id.to_string(),
                tool_name: None,
                is_error: false,
                error_text: Some(finalize_cause.to_string()),
                result_chars: 0,
                call_fingerprint: None,
            });
        }
    }
}

/// 输入指纹：tool 名无关（domain 以 (tool, fingerprint) 聚合），取值的
/// 紧凑序列化并截断——同参数重调用的识别原料。
fn fingerprint_input(input: Option<&serde_json::Value>) -> String {
    let Some(input) = input else {
        return String::new();
    };
    let serialized = input.to_string();
    let bounded: String = serialized.chars().take(120).collect();
    bounded
}

/// 错误文本有界截断（首两行 + 摘录上限）。
fn truncate_error_text(text: &str) -> String {
    let head: String = text.lines().take(2).collect::<Vec<_>>().join(" ");
    if head.chars().count() <= EVIDENCE_EXCERPT_BOUND {
        return head;
    }
    let mut truncated: String = head.chars().take(EVIDENCE_EXCERPT_BOUND).collect();
    truncated.push('…');
    truncated
}
