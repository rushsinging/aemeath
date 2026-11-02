//! session dataset 扫描 adapter（#2048，设计 04 分片 §2A.1/2A.2）。
//!
//! 从 `~/.agents/session/<id>.dataset/members/*.json` 的 step 记录提取
//! 扁平事件流（`SessionEventRecord`），交 domain 聚合。只读落盘，
//! NEVER 依赖主进程内存态；流式逐成员解析，NEVER 全量载入。

use crate::constants::{
    EVIDENCE_EXCERPT_BOUND, SESSION_CONTEXT_NEIGHBOR_CHARS, SESSION_CONTEXT_SOURCE_CHARS,
    SESSION_CONTEXT_TARGET_CHARS,
};
use crate::domain::evidence::{
    truncate_with_marker, EvidenceSourceRef, SessionContextExcerpt, SessionEventRecord,
};

use serde::Deserialize;
use std::io::BufReader;

/// 只投影 cursor.run_id 的成员探针（未知字段由 serde 跳过）。
#[derive(Deserialize)]
struct MemberCursorProbe {
    run_id: Option<String>,
}

#[derive(Deserialize)]
struct MemberProbe {
    cursor: Option<MemberCursorProbe>,
}

/// 提取 dataset 的 run id 清单（有序去重；cursor 投影逐成员流式读取，
/// NEVER 物化完整成员 JSON）。
pub fn dataset_run_ids(dataset_dir: &std::path::Path) -> Vec<String> {
    let mut run_ids: Vec<String> = Vec::new();
    let Ok(entries) = std::fs::read_dir(dataset_dir.join("members")) else {
        return run_ids;
    };
    let members_dir = dataset_dir.join("members");
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || !member_path_is_inside(&members_dir, &path) {
            continue;
        }
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        let Ok(member) = serde_json::from_reader::<_, MemberProbe>(BufReader::new(file)) else {
            continue;
        };
        if let Some(run_id) = member
            .cursor
            .and_then(|cursor| cursor.run_id)
            .filter(|run_id| !run_id.is_empty() && run_id != "unknown")
        {
            if !run_ids.iter().any(|existing| existing == &run_id) {
                run_ids.push(run_id);
            }
        }
    }
    run_ids
}

/// members 内的普通文件才可作为输入；canonicalize 后必须仍在 members 目录内，
/// 拒绝成员级 symlink 指向 dataset 外（防上下文/凭据注入）。
fn member_path_is_inside(members_dir: &std::path::Path, path: &std::path::Path) -> bool {
    let Ok(members_canonical) = members_dir.canonicalize() else {
        return false;
    };
    let Ok(member_canonical) = path.canonicalize() else {
        return false;
    };
    member_canonical.starts_with(&members_canonical)
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
        // 顶层 legacy 布局：session id 就是 dataset 目录名。
        let session_id = dataset_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string();
        Self::scan_dataset_as(dataset_dir, &session_id)
    }

    /// 项目布局：session id 保留 `<project>/<session>.dataset` 相对键，
    /// 供 RSI 区分项目/全局落点；路径逃逸返回空报告。
    pub fn scan_dataset_under_root(
        datasets_root: &std::path::Path,
        session_key: &str,
    ) -> ScanReport {
        let session_path = std::path::Path::new(session_key);
        let normal_count = session_path
            .components()
            .filter(|component| matches!(component, std::path::Component::Normal(_)))
            .count();
        if normal_count != 2
            || normal_count != session_path.components().count()
            || !session_key.ends_with(".dataset")
        {
            return ScanReport::default();
        }
        let dataset_dir = datasets_root.join(session_path);
        let Ok(root_canonical) = datasets_root.canonicalize() else {
            return ScanReport::default();
        };
        let Ok(dataset_canonical) = dataset_dir.canonicalize() else {
            return ScanReport::default();
        };
        if !dataset_canonical.starts_with(&root_canonical) {
            return ScanReport::default();
        }
        Self::scan_dataset_as(&dataset_dir, session_key)
    }

    fn scan_dataset_as(dataset_dir: &std::path::Path, session_id: &str) -> ScanReport {
        let mut report = ScanReport::default();
        let Ok(entries) = std::fs::read_dir(dataset_dir.join("members")) else {
            return report;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || !member_path_is_inside(&dataset_dir.join("members"), &path) {
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
            let Some(cursor_run_id) = member
                .pointer("/cursor/run_id")
                .and_then(|value| value.as_str())
            else {
                report.skipped_members += 1;
                continue;
            };
            let Some(cursor_step_id) = member
                .pointer("/cursor/step_id")
                .and_then(|value| value.as_str())
            else {
                report.skipped_members += 1;
                continue;
            };
            // cursor 是回读来源的唯一稳定身份；step 本体不一致时视为损坏成员，
            // NEVER 退化成 unknown 去绑定另一条记录。
            if let Some(body_step_id) = member
                .pointer("/step/step_id")
                .and_then(|value| value.as_str())
            {
                if body_step_id != cursor_step_id {
                    report.skipped_members += 1;
                    continue;
                }
            }
            if session_id == "unknown" || cursor_run_id.is_empty() || cursor_step_id.is_empty() {
                report.skipped_members += 1;
                continue;
            }
            Self::extract_step_events(
                session_id,
                cursor_run_id,
                cursor_step_id,
                step,
                &mut report.events,
            );
        }
        report
    }

    fn extract_step_events(
        session_id: &str,
        run_id: &str,
        step_id: &str,
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
                            step_id: step_id.to_string(),
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
                step_id: step_id.to_string(),
                tool_name: None,
                is_error: false,
                error_text: Some(finalize_cause.to_string()),
                result_chars: 0,
                call_fingerprint: None,
            });
        }
    }
    /// 在 datasets root 内安全定位 session；session id 只支持项目布局
    /// `<project>/<session>.dataset`，全部组件 MUST 是 Normal，canonicalize
    /// 后仍在 root 内（防逃逸）。顶层 legacy key NEVER 进入生产回读。
    pub fn read_step_context_under_root(
        datasets_root: &std::path::Path,
        source: &EvidenceSourceRef,
    ) -> Option<SessionContextExcerpt> {
        let session_path = std::path::Path::new(&source.session_id);
        let component_count = session_path.components().count();
        if component_count != 2
            || !session_path
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_)))
            || !source.session_id.ends_with(".dataset")
        {
            return None;
        }
        let dataset_dir = datasets_root.join(session_path);
        let root_canonical = datasets_root.canonicalize().ok()?;
        let dataset_canonical = dataset_dir.canonicalize().ok()?;
        if !dataset_canonical.starts_with(&root_canonical) {
            return None;
        }
        Self::read_step_context(&dataset_canonical, source)
    }

    /// 回读来源 step 及相邻 step 的有界上下文（只读落盘，NEVER 全量载入）。
    pub fn read_step_context(
        dataset_dir: &std::path::Path,
        source: &EvidenceSourceRef,
    ) -> Option<SessionContextExcerpt> {
        let entries = std::fs::read_dir(dataset_dir.join("members")).ok()?;
        let mut steps: Vec<(u64, String, std::path::PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || !member_path_is_inside(&dataset_dir.join("members"), &path) {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(member): Result<serde_json::Value, _> = serde_json::from_str(&raw) else {
                continue;
            };
            let Some(run_id) = member
                .pointer("/cursor/run_id")
                .and_then(|value| value.as_str())
            else {
                continue;
            };
            if run_id != source.run_id {
                continue;
            }
            let Some(step_id) = member
                .pointer("/cursor/step_id")
                .and_then(|value| value.as_str())
            else {
                continue;
            };
            if step_id.is_empty() {
                continue;
            }
            if let Some(body_step_id) = member
                .pointer("/step/step_id")
                .and_then(|value| value.as_str())
            {
                if body_step_id != step_id {
                    continue;
                }
            }
            let revision = member
                .pointer("/step/outcome/committed_revision")
                .and_then(|value| value.as_u64())
                .unwrap_or_default();
            steps.push((revision, step_id.to_string(), path));
        }
        // 同一 step 可能存在多 revision；先按 revision 降序去重保留最新副本，
        // 再恢复时间序，避免来源窗口锚定旧版本。
        steps.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        steps.dedup_by(|left, right| left.1 == right.1);
        steps.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        let target_index = steps
            .iter()
            .position(|(_, step_id, _)| step_id == &source.step_id)?;
        let start = target_index.saturating_sub(1);
        let end = (target_index + 2).min(steps.len());
        let mut segments: Vec<String> = Vec::new();
        for (selected_index, (_, step_id, path)) in
            steps.iter().enumerate().skip(start).take(end - start)
        {
            let Ok(raw) = std::fs::read_to_string(path) else {
                continue;
            };
            let Ok(member): Result<serde_json::Value, _> = serde_json::from_str(&raw) else {
                continue;
            };
            let mut lines = vec![format!("[{step_id}]")];
            append_accepted_input_context(&member, &mut lines);
            append_outcome_context(&member, &mut lines);
            let budget = if selected_index == target_index {
                SESSION_CONTEXT_TARGET_CHARS
            } else {
                SESSION_CONTEXT_NEIGHBOR_CHARS
            };
            segments.push(truncate_with_marker(&lines.join("\n"), budget));
        }
        let text = truncate_with_marker(&segments.join("\n"), SESSION_CONTEXT_SOURCE_CHARS);
        Some(SessionContextExcerpt {
            source: source.clone(),
            text,
        })
    }
}

/// 追加 accepted_input 中的用户意图摘要。
fn append_accepted_input_context(member: &serde_json::Value, lines: &mut Vec<String>) {
    let Some(messages) = member
        .pointer("/step/accepted_input/messages")
        .and_then(|value| value.as_array())
    else {
        return;
    };
    for message in messages {
        append_message_blocks(message, "用户", lines);
    }
}

/// 追加 outcome 消息中的 assistant 文本与工具事实。
fn append_outcome_context(member: &serde_json::Value, lines: &mut Vec<String>) {
    let Some(messages) = member
        .pointer("/step/outcome/messages")
        .and_then(|value| value.as_array())
    else {
        return;
    };
    for message in messages {
        let role = message
            .get("role")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        append_message_blocks(
            message,
            if role == "assistant" {
                "assistant"
            } else {
                "用户"
            },
            lines,
        );
    }
}

/// 从消息 content 提取有界事实（文本、tool_use、tool_result）。
fn append_message_blocks(message: &serde_json::Value, actor: &str, lines: &mut Vec<String>) {
    let Some(blocks) = message.get("content").and_then(|value| value.as_array()) else {
        return;
    };
    for block in blocks {
        match block.get("type").and_then(|value| value.as_str()) {
            Some("text") => {
                let text = block
                    .get("text")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                if !text.trim().is_empty() {
                    lines.push(format!("{actor}: {}", bounded_text(text, 240)));
                }
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                let goal = block
                    .pointer("/input/goal")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                let input = compact_json(block.get("input"), 240);
                lines.push(format!(
                    "工具调用 {name}{}：{input}",
                    if goal.is_empty() {
                        String::new()
                    } else {
                        format!("（goal: {}）", bounded_text(goal, 100))
                    }
                ));
            }
            Some("tool_result") => {
                let is_error = block
                    .get("is_error")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                let text = block
                    .get("text")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| compact_json(block.get("content"), 240));
                if !text.trim().is_empty() {
                    lines.push(format!(
                        "工具结果{}：{}",
                        if is_error { "（错误）" } else { "" },
                        bounded_text(&text, 320)
                    ));
                }
            }
            _ => {}
        }
    }
}

/// 文本有界截断（字符数口径，省略号计入预算）。
fn bounded_text(text: &str, bound: usize) -> String {
    truncate_with_marker(text, bound)
}

/// JSON 值紧凑摘要（缺失返回空字符串）。
fn compact_json(value: Option<&serde_json::Value>, bound: usize) -> String {
    let Some(value) = value else {
        return String::new();
    };
    bounded_text(&value.to_string(), bound)
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
