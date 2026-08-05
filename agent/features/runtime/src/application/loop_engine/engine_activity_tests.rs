/// #1500：Loop Engine 的 compact 进度视图必须把 Context 的压缩阶段
/// （Preparing/Summarizing chunk 计数/Finalizing）实时转发到 Activity 观测——
/// TUI 进度条的最终数据源。复现：旧实现 hardcode Summarizing(None,None)，
/// chunk 计数从未到达 activity，进度条恒 50%。
#[tokio::test]
async fn compact_progress_view_updates_activity_with_chunk_counts() {
    let mut scenario = ScriptedScenario::default();
    let run_id = sdk::RunId::new_v7();
    let activities = std::sync::Arc::new(
        crate::application::activity::ActivityCoordinator::production_without_publisher(
            run_id.clone(),
        ),
    );
    let mut run_loop = scenario.ports().run_loop();
    run_loop.bind_activity_context(activities.clone(), "test-model".to_string());

    let step_id = sdk::RunStepId::new_v7();
    let activity_id = run_loop
        .start_compaction_activity(step_id.clone())
        .expect("compaction activity must start");
    let progress_view = run_loop.compact_progress_view(activity_id.clone());

    let compaction_detail = |activities: &crate::application::activity::ActivityCoordinator| {
        activities
            .snapshot()
            .activities
            .iter()
            .find_map(|activity| match &activity.detail {
                sdk::ActivityDetailView::Compact {
                    stage,
                    current,
                    total,
                } => Some((*stage, *current, *total)),
                _ => None,
            })
            .expect("compaction activity must be observed")
    };

    // 模拟 Context map-reduce 管线上报的进度序列，每步后立即断言 activity 状态
    progress_view.emit(sdk::CompactStageView::Preparing, None, None);
    assert_eq!(
        compaction_detail(&activities),
        (sdk::CompactStageView::Preparing, None, None)
    );

    progress_view.emit(sdk::CompactStageView::Summarizing, Some(2), Some(5));
    assert_eq!(
        compaction_detail(&activities),
        (sdk::CompactStageView::Summarizing, Some(2), Some(5)),
        "chunk 计数 2/5 必须实时到达 activity（#1500）"
    );

    progress_view.emit(sdk::CompactStageView::Summarizing, Some(4), Some(5));
    assert_eq!(
        compaction_detail(&activities),
        (sdk::CompactStageView::Summarizing, Some(4), Some(5)),
        "chunk 计数 4/5 必须实时到达 activity（#1500）"
    );

    progress_view.emit(sdk::CompactStageView::Finalizing, None, None);
    assert_eq!(
        compaction_detail(&activities),
        (sdk::CompactStageView::Finalizing, None, None)
    );
}
