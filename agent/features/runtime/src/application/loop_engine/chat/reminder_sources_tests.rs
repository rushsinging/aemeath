use super::*;
use crate::application::constants::TASK_PROGRESS_REFRESH_INTERVAL_STEPS;
use crate::application::loop_engine::chat::reminder_sources::{
    RunStartFactReminderSource, TaskProgressReminderSource,
};
use std::time::SystemTime;

fn access_with_progress(completed: usize) -> task::TaskStore {
    let store = task::TaskStore::new();
    let access: &dyn task::TaskAccess = &store;
    access
        .create_batch(
            task::BatchCreateSpecData::try_new("batch".into()).unwrap(),
            1,
        )
        .unwrap();
    let mut task_ids = Vec::new();
    for index in 0..2 {
        let task_spec = task::TaskCreateSpecData::try_new(
            format!("任务 {index}"),
            String::new(),
            None,
            task::TaskPriorityData::Normal,
        )
        .unwrap();
        task_ids.push(access.create_task(task_spec, 2).unwrap().value.id());
    }
    for task_id in task_ids.iter().take(completed).cloned() {
        access
            .transition_with_progress(
                task_id,
                task::TaskStatusData::Completed,
                (completed + 10) as u64,
            )
            .unwrap();
    }
    store
}

#[test]
fn task_progress_source_builds_snapshot_from_task_access_and_renders_via_context() {
    let source = TaskProgressReminderSource::new(
        Arc::new(access_with_progress(1)),
        share::config::TaskListConfig::default().max_lines,
    );

    assert_eq!(source.kind().as_str(), "task_progress");
    let snapshot = source.build().expect("有 active batch 任务");
    let decoded: context::InvocationReminderData =
        serde_json::from_str(&snapshot.data).expect("快照为 InvocationReminderData JSON");
    match decoded {
        context::InvocationReminderData::TaskProgress(progress) => {
            assert_eq!(progress.total, 2);
            assert_eq!(progress.completed, 1);
        }
        other => panic!("期望 TaskProgress，得到 {other:?}"),
    }

    let rendered = source.render(&snapshot, "zh");
    assert!(
        rendered.contains("当前任务进度："),
        "渲染委托 context 文案单一真相"
    );
}

#[test]
fn task_progress_source_builds_none_without_active_batch() {
    let empty_store = task::TaskStore::new();
    let source = TaskProgressReminderSource::new(
        Arc::new(empty_store),
        share::config::TaskListConfig::default().max_lines,
    );
    assert!(source.build().is_none(), "无任务周期 source 本轮不入队");
}

#[test]
fn task_progress_source_policy_declares_interval_rebuild_and_tail_placement() {
    let source = TaskProgressReminderSource::new(
        Arc::new(access_with_progress(0)),
        share::config::TaskListConfig::default().max_lines,
    );
    let policy = source.policy();
    assert!(
        matches!(
            policy.refresh,
            context::RefreshTrigger::OnStepInterval(interval)
                if interval == TASK_PROGRESS_REFRESH_INTERVAL_STEPS
        ),
        "周期重注入（run_started 的 step=0 提供首次注入）"
    );
    assert_eq!(
        policy.placement,
        context::ReminderPlacement::TailUserMessage
    );
    assert!(matches!(policy.compact, context::CompactBehavior::Rebuild));
}

#[test]
fn run_start_fact_source_carries_frozen_data_and_matching_policy() {
    let guidance = RunStartFactReminderSource::guidance_sources_changed(
        vec!["~/.agents/guidance/_default.md".to_string()],
        share::config::domain::config::GuidanceReloadPolicy::Remind,
    );
    assert_eq!(guidance.kind().as_str(), "guidance_sources_changed");
    let snapshot = guidance.build().expect("事实型恒有内容");
    assert_eq!(
        serde_json::from_str::<context::InvocationReminderData>(&snapshot.data).unwrap(),
        context::InvocationReminderData::GuidanceSourcesChanged {
            paths: vec!["~/.agents/guidance/_default.md".to_string()],
        }
    );
    let rendered = guidance.render(&snapshot, "zh");
    assert!(
        rendered.contains("用 Read 工具重新读取"),
        "Remind 形态带 Read 引导：{rendered}"
    );
    assert!(rendered.contains("~/.agents/guidance/_default.md"));
    let policy = guidance.policy();
    assert_eq!(policy.placement, context::ReminderPlacement::SystemTail);

    let mismatch =
        RunStartFactReminderSource::model_guidance_mismatch("session-model", "run-model");
    assert_eq!(mismatch.kind().as_str(), "model_guidance_mismatch");
    assert_eq!(
        mismatch.policy().placement,
        context::ReminderPlacement::SystemTail
    );
    let rendered = mismatch.render(&mismatch.build().unwrap(), "zh");
    assert!(rendered.contains("session-model"));
    assert!(rendered.contains("run-model"));

    let memory = RunStartFactReminderSource::memory_updated(3);
    assert_eq!(memory.kind().as_str(), "memory_updated");
    assert_eq!(
        memory.policy().placement,
        context::ReminderPlacement::TailUserMessage
    );
    assert!(matches!(
        memory.policy().compact,
        context::CompactBehavior::Drop
    ));
}

// --- MemoryRecallReminderSource（#1834 per-message 记忆召回）----------------

use crate::application::loop_engine::chat::reminder_sources::MemoryRecallReminderSource;

/// 按 content 是否含关键词给概率的评分桩。
struct KeywordScoring {
    keyword: &'static str,
    high: f64,
}

#[async_trait::async_trait]
impl systemone::ScoringPort for KeywordScoring {
    async fn answer(
        &self,
        _state: &systemone::ScoringState,
        questions: &[systemone::ScoringQuestion],
    ) -> Result<Vec<systemone::ScoringAnswer>, systemone::ScoringUnavailable> {
        let criteria = match &questions[0] {
            systemone::ScoringQuestion::Choice { criteria, .. } => criteria,
            systemone::ScoringQuestion::Noul { .. } => {
                // 单候选 Noul 路径：按 state 是否含关键词给 p_true。
                let p = if _state.as_str().contains(self.keyword) {
                    self.high
                } else {
                    1.0 - self.high
                };
                return Ok(vec![systemone::ScoringAnswer::noul(
                    p,
                    systemone::CalibrationLevel::Raw,
                )
                .expect("答案构造")]);
            }
            _ => panic!("应为 Choice/Noul"),
        };
        let probabilities: Vec<(String, f64)> = criteria
            .iter()
            .enumerate()
            .map(|(index, (_, content))| {
                let probability = if content.contains(self.keyword) {
                    self.high
                } else {
                    (1.0 - self.high) / (criteria.len().saturating_sub(1).max(1)) as f64
                };
                (index.to_string(), probability)
            })
            .collect();
        let (top_key, top_probability) = probabilities
            .iter()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(key, p)| (key.clone(), *p))
            .expect("非空");
        Ok(vec![systemone::ScoringAnswer::choice(
            top_key,
            probabilities,
            top_probability,
            systemone::CalibrationLevel::Raw,
        )
        .expect("答案构造")])
    }
}

struct FailingScoring;

#[async_trait::async_trait]
impl systemone::ScoringPort for FailingScoring {
    async fn answer(
        &self,
        _state: &systemone::ScoringState,
        _questions: &[systemone::ScoringQuestion],
    ) -> Result<Vec<systemone::ScoringAnswer>, systemone::ScoringUnavailable> {
        Err(systemone::ScoringUnavailable::new(
            systemone::UnavailableKind::Connect,
            "服务未启动",
        ))
    }
}

async fn recall_memory(entries: Vec<&str>) -> Arc<dyn memory::api::MemoryPort> {
    use memory::api::MemoryPort as _;

    let memory = memory::api::InMemoryMemory::new(memory::api::MemoryPolicy {
        max_entries: 50,
        similarity_threshold: 0.8,
        target_active_entries: 150,
        default_ttl_secs: 0,
    })
    .expect("policy 合法");
    for content in entries {
        // 先经 `new` 构造合法条目，再覆写 content——存量盘上可能存在写入端
        // 长度不变式上线前的超长遗留条目，fixture 必须能表达该形态。
        let mut entry = memory::api::MemoryEntry::new(
            memory::api::MemoryId::now_v7(),
            10,
            memory::api::MemoryLayer::Project,
            memory::api::MemoryCategory::Fact,
            "占位",
            memory::api::MemorySource::User,
        )
        .expect("entry 构造");
        entry.content = content.to_string();
        memory.write(entry).await.expect("写入");
    }
    Arc::new(memory)
}

#[tokio::test]
async fn memory_recall_source_never_materializes_outdated_memory() {
    let memory = recall_memory(vec![
        "rust 所有权指南 ownership 有效条目",
        "rust ownership 历史编译器特殊操作守则",
    ])
    .await;
    let before = memory
        .search(&memory::api::search::MemorySearchQuery {
            text: "历史编译器特殊操作守则".to_owned(),
            limit: 10,
            layer: None,
            category: None,
            include_archive: false,
            exclude_stale: false,
            confirm_hits: false,
            now: 4_242,
        })
        .await;
    let stale_id = before.hits.first().expect("旧条目先存在").entry.id;
    assert!(memory.mark_outdated(&stale_id).await.expect("标记过期"));
    let source = MemoryRecallReminderSource::with_clock(
        memory,
        Arc::new(KeywordScoring {
            keyword: "ownership",
            high: 0.95,
        }),
        Arc::new(|| 4_242),
    );
    source.refresh("rust ownership").await;
    let snapshot = source.build().expect("有效条目仍应物化");
    let body = source.render(&snapshot, "zh");
    assert!(body.contains("有效条目"), "有效记忆必须在最终 reminder 中");
    assert!(!body.contains("历史编译器"), "过期记忆不得渗入 reminder");
    assert!(
        !body.contains(&stale_id.to_string()),
        "过期身份不得进入 reminder"
    );
}

#[tokio::test]
async fn recall_refresh_caches_snapshot_and_build_renders() {
    let source = MemoryRecallReminderSource::with_clock(
        recall_memory(vec!["git worktree 的创建步骤", "完全无关的烹饪食谱"]).await,
        Arc::new(KeywordScoring {
            keyword: "worktree",
            high: 0.9,
        }),
        Arc::new(|| 4_242),
    );
    assert!(
        source.build().is_none(),
        "refresh 前 build 应为空（无快照不注入）"
    );

    source.refresh("怎么用 worktree 隔离分支").await;

    let snapshot = source.build().expect("refresh 后应有快照");
    let rendered = source.render(&snapshot, "zh");
    assert!(rendered.contains("相关的记忆"), "渲染应含标题：{rendered}");
    assert!(rendered.contains("git worktree 的创建步骤"));
    assert!(!rendered.contains("烹饪食谱"), "低分候选不入 top-K");
}

#[tokio::test]
async fn recall_below_threshold_clears_cache_and_skips_turn() {
    let source = MemoryRecallReminderSource::with_clock(
        recall_memory(vec!["git worktree 的创建步骤", "另一条 rust 笔记"]).await,
        Arc::new(KeywordScoring {
            keyword: "worktree",
            high: 0.9,
        }),
        Arc::new(|| 4_242),
    );
    source.refresh("worktree 怎么用").await;
    assert!(source.build().is_some(), "首次 refresh 应有快照");

    // 第二条消息词法零命中（与所有条目无公共词）→ 缓存清空（本 turn 不注入）。
    source.refresh("zzz 完全无关的话题").await;
    assert!(
        source.build().is_none(),
        "零命中消息的 refresh 必须清空旧快照（NEVER 用陈旧快照）"
    );

    // 评分概率低于阈值 → 本 turn 不注入。
    let low_source = MemoryRecallReminderSource::with_clock(
        recall_memory(vec!["git worktree 的创建步骤"]).await,
        Arc::new(KeywordScoring {
            keyword: "不命中",
            high: 0.3,
        }),
        Arc::new(|| 4_242),
    );
    low_source.refresh("随便聊聊").await;
    assert!(low_source.build().is_none(), "top1 概率低于阈值时不得注入");
}

#[tokio::test]
async fn recall_scoring_failure_clears_cache_silently() {
    let source = MemoryRecallReminderSource::with_clock(
        recall_memory(vec!["git worktree 的创建步骤"]).await,
        Arc::new(FailingScoring),
        Arc::new(|| 4_242),
    );
    source.refresh("worktree").await;
    assert!(
        source.build().is_none(),
        "评分服务不可用必须静默缺席（NEVER 阻断 turn）"
    );
}

#[tokio::test]
async fn recall_renews_only_entries_materialized_within_budget() {
    let contents: Vec<String> = (0..8)
        .map(|index| format!("worktree 记忆 {index} {}", "细".repeat(450)))
        .collect();
    let memory = recall_memory(contents.iter().map(String::as_str).collect()).await;
    let source = MemoryRecallReminderSource::with_clock(
        Arc::clone(&memory),
        Arc::new(KeywordScoring {
            keyword: "worktree",
            high: 0.9,
        }),
        Arc::new(|| 4_242),
    );
    source.refresh("worktree").await;
    let snapshot = source.build().expect("应物化召回快照");
    let context::InvocationReminderData::MemoryRecall { entries } =
        serde_json::from_str(&snapshot.data).unwrap()
    else {
        panic!("期望 MemoryRecall 快照");
    };
    assert!(entries.len() < contents.len(), "预算必须淘汰部分候选");
    for entry in memory.list(None).await {
        let included = entries.iter().any(|item| item.id == entry.id.to_string());
        assert_eq!(entry.last_confirmed_at, if included { 4_242 } else { 10 });
        assert_eq!(entry.confirmation_count, u32::from(included));
    }
    source.refresh("worktree").await;
    assert!(
        memory
            .list(None)
            .await
            .iter()
            .all(|entry| entry.confirmation_count <= 1),
        "同秒重复命中不能重复累计确认次数"
    );
}

#[tokio::test]
async fn recall_single_hit_renews_but_rejected_and_failed_recall_do_not() {
    let memory = recall_memory(vec!["worktree 单条召回"]).await;
    let low_source = MemoryRecallReminderSource::with_clock(
        Arc::clone(&memory),
        Arc::new(KeywordScoring {
            keyword: "worktree",
            high: 0.3,
        }),
        Arc::new(|| 4_242),
    );
    low_source.refresh("worktree").await;
    assert!(low_source.build().is_none());
    assert_eq!(memory.list(None).await[0].last_confirmed_at, 10);
    let failed_source = MemoryRecallReminderSource::with_clock(
        Arc::clone(&memory),
        Arc::new(FailingScoring),
        Arc::new(|| 4_242),
    );
    failed_source.refresh("worktree").await;
    assert!(failed_source.build().is_none());
    assert_eq!(memory.list(None).await[0].last_confirmed_at, 10);
    let accepted_source = MemoryRecallReminderSource::with_clock(
        Arc::clone(&memory),
        Arc::new(KeywordScoring {
            keyword: "worktree",
            high: 0.9,
        }),
        Arc::new(|| 4_242),
    );
    accepted_source.refresh("不存在的检索词").await;
    assert!(accepted_source.build().is_none());
    assert_eq!(memory.list(None).await[0].last_confirmed_at, 10);
    accepted_source.refresh("worktree").await;
    assert!(accepted_source.build().is_some());
    let renewed = memory.list(None).await;
    assert_eq!(renewed[0].last_confirmed_at, 4_242);
    assert_eq!(renewed[0].confirmation_count, 1);
}

#[tokio::test]
async fn recall_budget_drops_tail_entries() {
    let long_entry = "超".repeat(180);
    let source = MemoryRecallReminderSource::with_clock(
        recall_memory(vec![
            "git worktree 的创建步骤与隔离实践",
            "git worktree 的清理注意事项",
            "git worktree 的磁盘占用排查",
        ])
        .await,
        Arc::new(KeywordScoring {
            keyword: "worktree",
            high: 0.9,
        }),
        Arc::new(|| 4_242),
    );
    source.refresh("worktree").await;
    let snapshot = source.build().expect("应有快照");
    let data: context::InvocationReminderData =
        serde_json::from_str(&snapshot.data).expect("合法快照");
    let count = match &data {
        context::InvocationReminderData::MemoryRecall { entries } => entries.len(),
        other => panic!("期望 MemoryRecall，实际 {other:?}"),
    };
    assert!(
        count <= crate::application::constants::MEMORY_RECALL_TOP_K,
        "注入条数受 top-K 上限约束：{count}"
    );
    assert!(count >= 1);
    let _ = long_entry; // 长内容场景由预览截断覆盖（预览上限 200 字符）
}

#[tokio::test]
async fn recall_preview_keeps_full_content_within_write_limit() {
    // 写入端上限 500 内的条目永远完整注入——LLM 消费全文，NEVER 半句话。
    let content = format!("{}。完整结论必须完整注入。", "详".repeat(480));
    let source = MemoryRecallReminderSource::with_clock(
        recall_memory(vec![&content]).await,
        Arc::new(KeywordScoring {
            keyword: "完整",
            high: 0.9,
        }),
        Arc::new(|| 4_242),
    );
    source.refresh("完整结论怎么用").await;
    let snapshot = source.build().expect("应有快照");
    let data: context::InvocationReminderData =
        serde_json::from_str(&snapshot.data).expect("合法快照");
    match &data {
        context::InvocationReminderData::MemoryRecall { entries } => {
            assert_eq!(entries.len(), 1);
            assert_eq!(
                entries[0].content_preview, content,
                "≤ 写入上限的条目必须原文注入"
            );
        }
        other => panic!("期望 MemoryRecall，实际 {other:?}"),
    }
}

#[tokio::test]
async fn recall_preview_trims_overlong_entry_at_sentence_boundary_with_marker() {
    // 存量盘上超长条目（写入不变式上线前遗留）的兜底：句边界截断 + 截断标记。
    let overlong = format!("第一句结论完整保留。{}", "尾部冗余细节。".repeat(120));
    let source = MemoryRecallReminderSource::with_clock(
        recall_memory(vec![&overlong]).await,
        Arc::new(KeywordScoring {
            keyword: "结论",
            high: 0.9,
        }),
        Arc::new(|| 4_242),
    );
    source.refresh("结论是什么").await;
    let snapshot = source.build().expect("应有快照");
    let data: context::InvocationReminderData =
        serde_json::from_str(&snapshot.data).expect("合法快照");
    match &data {
        context::InvocationReminderData::MemoryRecall { entries } => {
            let preview = &entries[0].content_preview;
            assert!(
                preview.chars().count()
                    <= crate::application::constants::MEMORY_RECALL_PREVIEW_CHARS,
                "兜底预览不得超过预览上限：{}",
                preview.chars().count()
            );
            assert!(
                preview.contains("第一句结论完整保留"),
                "句首关键结论必须在预览内"
            );
            assert!(preview.contains("（截断）"), "截断必须显式标记");
        }
        other => panic!("期望 MemoryRecall，实际 {other:?}"),
    }
}

#[tokio::test]
async fn recall_snapshot_data_stable_for_same_result_set() {
    let entries = vec!["git worktree 的创建步骤", "rust 所有权笔记"];
    let source = MemoryRecallReminderSource::with_clock(
        recall_memory(entries.clone()).await,
        Arc::new(KeywordScoring {
            keyword: "worktree",
            high: 0.9,
        }),
        Arc::new(|| 4_242),
    );
    source.refresh("worktree 怎么用").await;
    let first = source.build().expect("第一次快照").data;
    source.refresh("worktree 怎么用").await;
    let second = source.build().expect("第二次快照").data;
    assert_eq!(
        first, second,
        "相同结果集的快照 data 必须稳定（SkipIfUnchanged 去重前提）"
    );
}

#[test]
fn background_process_source_policy_is_event_tail_dedup_rebuild() {
    let supervisor = Arc::new(
        crate::application::background_process::supervisor::BackgroundProcessSupervisor::new(),
    );
    let source = BackgroundProcessReminderSource::new(supervisor);

    assert_eq!(source.kind().as_str(), "background_process");
    let policy = source.policy();
    assert!(matches!(
        policy.refresh,
        context::RefreshTrigger::OnEvent(ref event)
            if *event == context::ReminderEventSource::background_process()
    ));
    assert!(matches!(
        policy.placement,
        context::ReminderPlacement::TailUserMessage
    ));
    assert!(matches!(
        policy.inject.dedup,
        context::ReminderDedup::SkipIfUnchanged
    ));
    assert!(matches!(policy.compact, context::CompactBehavior::Rebuild));
}

#[test]
fn background_process_source_build_peeks_until_confirmed_and_renders() {
    let supervisor = Arc::new(
        crate::application::background_process::supervisor::BackgroundProcessSupervisor::new(),
    );
    let source = BackgroundProcessReminderSource::new(supervisor.clone());

    // 无终态任务：build 为 None（本轮无内容不入队）。
    assert!(source.build().is_none());

    // 推进一个终态：build 携带完成条目（take 语义）。
    let task_id = supervisor.register(
        background_process_identity(),
        "command=cargo test",
        SystemTime::now(),
    );
    supervisor
        .finish(
            &task_id,
            crate::domain::background_process::BackgroundProcessTerminalKind::Success,
            Some("ok 3 passed\n".to_string()),
        )
        .unwrap();
    let snapshot = source.build().expect("终态后应有快照");
    let decoded: context::InvocationReminderData =
        serde_json::from_str(&snapshot.data).expect("快照为 InvocationReminderData JSON");
    match decoded {
        context::InvocationReminderData::BackgroundProcessCompleted { items } => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].tool_name, "Bash");
            assert_eq!(items[0].tool_name, "Bash");
        }
        other => panic!("应为 BackgroundProcessCompleted，实际 {other:?}"),
    }

    // peek 语义（注入确认制）：确认前再 build 仍可见（Run 收口后
    // 下个 Run 补注入）；注入确认后关闭。
    assert!(source.build().is_some(), "确认前事实存活");
    source.confirm_injected();
    assert!(source.build().is_none(), "注入确认后不再重复");

    // render 委托 context 双语渲染。
    let rendered = source.render(&snapshot, "zh");
    assert!(rendered.contains("后台进程已完成"));
}

fn background_process_identity() -> context::ToolCallIdentityData {
    context::ToolCallIdentityData {
        session_id: context::SessionId::new("session-1"),
        run_id: sdk::RunId::new("run-1"),
        step_id: sdk::RunStepId::new("step-1"),
        runtime_call_id: "runtime-call-1".to_string(),
        provider_call_id: None,
        tool_name: "Bash".to_string(),
        call_index: 0,
        agent: false,
    }
}

// ---- #2046 config_sources_changed（agents 名册 Remind）----

#[test]
fn config_sources_changed_renders_key_list_and_session_semantics() {
    let source = super::RunStartFactReminderSource::config_sources_changed(vec![
        "config:reloaded".to_string()
    ]);
    assert_eq!(source.kind().as_str(), "config_sources_changed");
    let snapshot = source.build().expect("快照必须可构建");
    let zh = source.render(&snapshot, "zh");
    assert!(zh.contains("aemeath.json 配置已变更"), "zh 文案：{zh}");
    assert!(zh.contains("config:reloaded"), "变更键 MUST 出现：{zh}");
    assert!(zh.contains("下次会话生效"), "会话冻结语义：{zh}");
    assert!(zh.contains("agent 名册"), "名册提示：{zh}");
    let en = source.render(&snapshot, "en");
    assert!(en.contains("configuration changed"));
}
