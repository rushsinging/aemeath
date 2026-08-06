#[tokio::test]
async fn test_run_session_command_driver_stop_hook_blocked_continues_until_success() {
    // 每次测试生成独立 flag 路径，避免 cargo test 并行 race。
    let flag_path = std::env::temp_dir().join(format!(
        "aemeath_stop_hook_once_{}.flag",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&flag_path);
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let provider = Arc::new(SequenceProvider::new(vec![
        "first attempted final",
        "after hook feedback",
    ]));

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "hello".to_string(),
            Vec::new(),
        ))
        .unwrap();

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        loop {
            if driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count()
                >= 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let shell = test_shell_with_hooks(blocking_then_success_hook_port(&flag_path));
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(provider.clone()),
    );
    shell.set_test_session_id("test-stop-hook-blocked");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run_session_command_driver(ctx),
    )
    .await
    .expect("run_session_command_driver should complete after shutdown");
    driver.await.unwrap();
    let _ = std::fs::remove_file(&flag_path);

    let events = sink.events();
    let feedback_sync = events
        .iter()
        .position(|event| event.starts_with("SessionMessageStateChanged:"))
        .expect("blocked Stop hook feedback should publish message state");
    let hook_activity = events
        .iter()
        .position(|event| event.starts_with("HookActivityChanged:Finished:"))
        .expect("blocked Stop hook should finish its activity");
    let second_text = events
        .iter()
        .position(|event| event == "Text:after hook feedback")
        .expect("blocked Stop hook should continue to another LLM turn");
    let done = events
        .iter()
        .position(|event| event == "DoneWithDuration")
        .expect("loop should finish after Stop hook succeeds");

    assert!(hook_activity < feedback_sync);
    assert!(feedback_sync < second_text);
    assert!(second_text < done);
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        2,
        "Stop block should trigger one continuation request"
    );
    let continuation = &requests[1];
    let texts = continuation
        .iter()
        .map(Message::text_content)
        .collect::<Vec<_>>();
    let assistant_idx = texts
        .iter()
        .position(|text| text == "first attempted final")
        .expect("blocked assistant output must remain in canonical history");
    let feedback_idx = texts
        .iter()
        .position(|text| text.contains("Stop hook prevented stopping"))
        .expect("Stop hook feedback must reach the continuation request");
    assert!(
        assistant_idx < feedback_idx,
        "history must precede Stop feedback: {texts:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.as_str() == "DoneWithDuration")
            .count(),
        1
    );
}

#[tokio::test]
async fn stop_hook_block_merges_feedback_with_follow_up_before_continuation() {
    let flag_path = std::env::temp_dir().join(format!(
        "aemeath_stop_hook_follow_up_{}.flag",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&flag_path);
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let provider = Arc::new(SequenceProvider::new(vec!["attempted final", "continued"]));
    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "initial".to_string(),
            Vec::new(),
        ))
        .unwrap();

    let driver_sink = sink.clone();
    let driver_input = input_tx.clone();
    let driver = tokio::spawn(async move {
        loop {
            if driver_sink
                .events()
                .iter()
                .any(|event| event.starts_with("HookActivityChanged:Started:"))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        driver_input
            .send(sdk::ChatInputEvent::user_message(
                "follow up during stop hook".to_string(),
                Vec::new(),
            ))
            .unwrap();
        loop {
            if driver_sink
                .events()
                .iter()
                .any(|event| event.as_str() == "DoneWithDuration")
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let shell = test_shell_with_hooks(delayed_blocking_then_success_hook_port(&flag_path));
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(provider.clone()),
    );
    shell.set_test_session_id("test-stop-hook-follow-up");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run_session_command_driver(ctx),
    )
    .await
    .expect("run_session_command_driver should complete after shutdown");
    driver.await.unwrap();
    let _ = std::fs::remove_file(&flag_path);

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        2,
        "follow-up must join the continuation, not start a new Run"
    );
    let texts = requests[1]
        .iter()
        .map(Message::text_content)
        .collect::<Vec<_>>();
    let assistant_idx = texts
        .iter()
        .position(|text| text == "attempted final")
        .unwrap();
    let feedback_idx = texts
        .iter()
        .position(|text| text.contains("Stop hook prevented stopping"))
        .unwrap();
    let follow_up_idx = texts
        .iter()
        .position(|text| text == "follow up during stop hook")
        .unwrap();
    assert!(
        assistant_idx < feedback_idx && feedback_idx < follow_up_idx,
        "unexpected continuation order: {texts:?}"
    );
    let feedback_count = sink
        .synced_messages()
        .into_iter()
        .filter(|messages| {
            messages
                .iter()
                .filter(|message| {
                    message
                        .text_content()
                        .contains("Stop hook prevented stopping")
                })
                .count()
                > 1
        })
        .count();
    assert_eq!(
        feedback_count, 0,
        "UI sync must not duplicate Stop feedback"
    );
}

#[tokio::test]
async fn test_stop_hook_feedback_message_is_marked_stop_hook() {
    let flag_path = std::env::temp_dir().join(format!(
        "aemeath_stop_hook_metadata_{}.flag",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&flag_path);
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "hello".to_string(),
            Vec::new(),
        ))
        .unwrap();

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        loop {
            if driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count()
                >= 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let shell = test_shell_with_hooks(blocking_then_success_hook_port(&flag_path));
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(Arc::new(
            SequenceProvider::new(vec!["first attempted final", "after hook feedback"]),
        )),
    );
    shell.set_test_session_id("test-stop-hook-metadata");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run_session_command_driver(ctx),
    )
    .await
    .expect("run_session_command_driver should complete after shutdown");
    driver.await.unwrap();
    let _ = std::fs::remove_file(&flag_path);

    let feedback = sink
        .synced_messages()
        .into_iter()
        .flatten()
        .find(|message| {
            message
                .text_content()
                .contains("Stop hook prevented stopping")
        })
        .expect("blocked Stop hook feedback should be synced into messages");

    assert_eq!(feedback.role, Role::User);
    assert_eq!(feedback.source(), MessageSource::Hook);
}

#[tokio::test]
async fn test_run_session_command_driver_uses_workspace_workspace_root_for_stop_hook_env() {
    let sink = RecordingSink::default();
    // #894: stop hook 的 cwd / `AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR` 必须取自
    // restore 后的 `workspace_root`。要让 `workspace_root` 合法地不同于 wire 时的路径，
    // 必须满足 Project 不变量：一个 linked worktree 与主仓共享同一 git common dir。
    // 因此创建真实 git 仓库 + linked worktree 作为合法 fixture（而非两个互不相关的临时目录，
    // 那样无法通过 prepare_restore 的同 repo 校验）。
    let tmp = tempfile::tempdir().unwrap();
    let main_repo = tmp.path().join("main");
    let linked_wt = tmp.path().join("linked");
    std::fs::create_dir_all(&main_repo).unwrap();
    let run_git = |args: &[&str], cwd: &std::path::Path| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .unwrap()
            .success()
    };
    assert!(run_git(&["init"], &main_repo), "git init 失败");
    run_git(&["config", "user.name", "test"], &main_repo);
    run_git(&["config", "user.email", "test@example.com"], &main_repo);
    run_git(&["config", "commit.gpgsign", "false"], &main_repo);
    std::fs::write(main_repo.join("README.md"), "init").unwrap();
    assert!(run_git(&["add", "-A"], &main_repo), "git add 失败");
    assert!(
        run_git(&["commit", "-m", "init"], &main_repo),
        "git commit 失败"
    );
    assert!(
        run_git(
            &["worktree", "add", linked_wt.to_str().unwrap(), "-b", "wt"],
            &main_repo
        ),
        "git worktree add 失败"
    );

    // 取 canonical 路径，构造自洽且满足不变量的完整 DTO。
    let main_repo = main_repo.canonicalize().unwrap();
    let workspace_root = linked_wt.canonicalize().unwrap();
    // `--git-common-dir` 可能输出相对路径（相对 main_repo），需按 base 解析后再 canonicalize，
    // 与 GitCli::resolve_git_path 语义一致。
    let raw_common = String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "--git-common-dir"])
            .current_dir(&main_repo)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    let common_path = std::path::PathBuf::from(raw_common);
    let common_dir = if common_path.is_absolute() {
        common_path
    } else {
        main_repo.join(common_path)
    }
    .canonicalize()
    .unwrap();

    let identity = share::session_types::ProjectIdentity {
        initial_cwd: main_repo.display().to_string(),
        git_common_dir: Some(common_dir.display().to_string()),
    };
    let workspace_root_str = workspace_root.display().to_string();
    let workspace_dto = context::session::PersistedWorkspaceContext {
        workspace_id: share::session_types::WorkspaceId::derive(&identity, &workspace_root_str),
        project_identity: identity,
        path_base: workspace_root_str.clone(),
        workspace_root: workspace_root_str,
        worktree_kind: share::session_types::WorktreeKind::Linked,
        context_stack: vec![],
    };
    // 从主仓 wire；prepare_restore + commit_restore 后 workspace_root 切换为 linked worktree
    // （与主仓路径不同），这正是本测试要验证的 stop hook env 来源。
    let workspace = project::wire_production_workspace(main_repo.clone())
        .expect("workspace 初始化成功")
        .into_views();
    let prepared = workspace
        .persist()
        .prepare_restore(&workspace_dto)
        .expect("prepare_restore 合法 DTO 应通过同 repo 校验");
    workspace.persist().commit_restore(prepared);

    let marker = tmp.path().join("stop-hook-env.txt");
    let marker_path = marker.display().to_string();
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: format!(
                "printf '%s|%s|%s' \"$AEMEATH_PROJECT_DIR\" \"$CLAUDE_PROJECT_DIR\" \"$PWD\" > \"{}\"",
                marker_path
            ),
            timeout: 5,
        }],
    );

    let (input_tx, input_events) = ChannelInputEvents::new();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "hello".to_string(),
            Vec::new(),
        ))
        .unwrap();

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        loop {
            if driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count()
                >= 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let mut shell = test_shell_with_hooks(Arc::new(
        hook::build_dispatcher(&share::config::domain::snapshot::ConfigSnapshot::new(share::config::Config { hooks: HooksConfig {
        events,
        ..HooksConfig::default()
    }, ..share::config::Config::default() })).unwrap(),
    ));
    shell.workspace = workspace;
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(Arc::new(
            SequenceProvider::new(vec!["final response"]),
        )),
    );
    shell.set_test_session_id("test-worktree-stop-hook-env");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run_session_command_driver(ctx),
    )
    .await
    .expect("run_session_command_driver should complete after shutdown");
    driver.await.unwrap();

    assert!(sink
        .events()
        .iter()
        .any(|event| event.starts_with("HookActivityChanged:Finished:")));
    let output = std::fs::read_to_string(marker).unwrap();
    let parts: Vec<&str> = output.split('|').collect();
    assert_eq!(parts.len(), 3);
    let expected = workspace_root.clone();
    for part in parts {
        assert_eq!(std::fs::canonicalize(part).unwrap(), expected);
    }
}

#[tokio::test]
async fn test_run_session_command_driver_drains_input_after_stop_hook_before_done() {
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "hello".to_string(),
            Vec::new(),
        ))
        .unwrap();

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        loop {
            if driver_sink
                .events()
                .iter()
                .any(|event| event == "Text:initial final response")
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        input_tx
            .send(sdk::ChatInputEvent::user_message(
                "stop-hook input".to_string(),
                Vec::new(),
            ))
            .unwrap();
        loop {
            if driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count()
                >= 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let shell = test_shell_with_hooks(test_hook_port());
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(Arc::new(
            TwoTurnProvider,
        )),
    );
    shell.set_test_session_id("test-session");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run_session_command_driver(ctx),
    )
    .await
    .expect("run_session_command_driver should complete after shutdown");
    driver.await.unwrap();

    let events = sink.events();
    // #1272: queue input drained within the same Run cycle produces a
    // multi-step run (one drain → step → drain → step → seal).  The
    // two inputs are processed as two steps within a single terminal Run
    // (one DoneWithDuration), not as two separate Runs.
    let _first_done = events
        .iter()
        .position(|event| event == "DoneWithDuration")
        .expect("run should finish");
    let done_count = events
        .iter()
        .filter(|event| event.as_str() == "DoneWithDuration")
        .count();
    assert_eq!(
        done_count, 1,
        "queue input is drained in the same Run (#1272)"
    );
    assert!(
        events
            .iter()
            .any(|event| event == "Text:initial final response"),
        "first step response"
    );
    assert!(
        events
            .iter()
            .any(|event| event == "Text:handled queued input"),
        "queue input step response"
    );
}

/// Hook 首次输出 `{"continue": false}` JSON (exit 0)，之后放行。
/// 用于验证 `continue:false` 被识别为阻断（#372 缺陷 1）。
fn continue_false_then_allow_hook_port(flag_path: &std::path::Path) -> Arc<dyn HookPort> {
    let flag_path_str = flag_path.to_string_lossy().to_string();
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: format!(
                "python3 -c 'import json,sys,pathlib; \
                 p=pathlib.Path(\"{flag_path}\"); \
                 sys.exit(0 if p.exists() else \
                 (p.parent.mkdir(parents=True, exist_ok=True), \
                 p.write_text(\"1\"), \
                 print(json.dumps({{\"continue\": False, \"stopReason\": \"must keep working\"}})), 0)[3])'",
                flag_path = flag_path_str,
            ),
            timeout: 5,
        }],
    );
    Arc::new(hook::build_dispatcher(&share::config::domain::snapshot::ConfigSnapshot::new(share::config::Config { hooks: HooksConfig {
        events,
        ..HooksConfig::default()
    }, ..share::config::Config::default() })).unwrap())
}

/// Hook 前 `n` 次阻断 (exit 2)，之后放行。用计数器文件跟踪调用次数。
fn block_n_times_hook_port(counter_path: &std::path::Path, n: usize) -> Arc<dyn HookPort> {
    let counter_path_str = counter_path.to_string_lossy().to_string();
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: format!(
                "python3 -c 'import pathlib,sys; \
                 p=pathlib.Path(\"{path}\"); \
                 c=int(p.read_text()) if p.exists() else 0; \
                 p.parent.mkdir(parents=True, exist_ok=True); \
                 p.write_text(str(c+1)); \
                 sys.exit(2 if c < {n} else 0)'",
                path = counter_path_str,
                n = n,
            ),
            timeout: 5,
        }],
    );
    Arc::new(hook::build_dispatcher(&share::config::domain::snapshot::ConfigSnapshot::new(share::config::Config { hooks: HooksConfig {
        events,
        ..HooksConfig::default()
    }, ..share::config::Config::default() })).unwrap())
}

/// Hook 每次都阻断 (exit 2)。用于验证连续阻断超上限强制停止（#372 缺陷 3）。
fn always_blocking_hook_port() -> Arc<dyn HookPort> {
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: "echo always blocked; exit 2".to_string(),
            timeout: 5,
        }],
    );
    Arc::new(hook::build_dispatcher(&share::config::domain::snapshot::ConfigSnapshot::new(share::config::Config { hooks: HooksConfig {
        events,
        ..HooksConfig::default()
    }, ..share::config::Config::default() })).unwrap())
}

#[tokio::test]
async fn test_continue_false_json_treated_as_block() {
    // #372 缺陷 1：Stop hook 输出 {"continue": false} (exit 0) 应被识别为阻断
    let flag_path = std::env::temp_dir().join(format!(
        "aemeath_continue_false_{}.flag",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&flag_path);
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "hello".to_string(),
            Vec::new(),
        ))
        .unwrap();

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        loop {
            if driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count()
                >= 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let shell = test_shell_with_hooks(continue_false_then_allow_hook_port(&flag_path));
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(Arc::new(
            SequenceProvider::new(vec!["first response", "second response"]),
        )),
    );
    shell.set_test_session_id("test-continue-false");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run_session_command_driver(ctx),
    )
    .await
    .expect("run_session_command_driver should complete after shutdown");
    driver.await.unwrap();
    let _ = std::fs::remove_file(&flag_path);

    let events = sink.events();
    // continue:false 应产生独立 typed feedback、消息同步并终结 Hook Activity。
    assert_eq!(
        events
            .iter()
            .filter(|event| event.starts_with("HookNotice:"))
            .count(),
        1,
        "continue:false should publish one typed feedback event: {:?}",
        events
    );
    assert!(
        events
            .iter()
            .any(|event| event.starts_with("SessionMessageStateChanged:")),
        "continue:false should still synchronize the canonical message snapshot: {:?}",
        events
    );
    // 应有反馈注入（stopReason 内容）
    assert!(
        events.iter().any(|e| e.contains("must keep working")),
        "stopReason should appear in feedback: {:?}",
        events
    );
    // 应有第 2 次 LLM 响应（说明阻断后 loop 继续）
    assert!(
        events.iter().any(|e| e == "Text:second response"),
        "loop should continue to second LLM turn: {:?}",
        events
    );
    // 最终应完成
    assert_eq!(
        events
            .iter()
            .filter(|e| e.as_str() == "DoneWithDuration")
            .count(),
        1,
        "loop should finish after hook allows: {:?}",
        events
    );
}

#[tokio::test]
async fn test_stall_triggers_stop_hook_check() {
    // #372 缺陷 2：stall 终止前应调用 Stop hook，阻断则重置 detector 并继续
    let counter_path = std::env::temp_dir().join(format!(
        "aemeath_stall_hook_{}.counter",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&counter_path);
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "hello".to_string(),
            Vec::new(),
        ))
        .unwrap();

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        loop {
            if driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count()
                >= 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    // LLM 前 3 次返回相同输出（触发 stall），第 4 次返回不同输出
    // Stop hook 前 3 次阻断，第 4 次放行
    let shell = test_shell_with_hooks(block_n_times_hook_port(&counter_path, 3));
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(Arc::new(
            SequenceProvider::new(vec![
                "same output",
                "same output",
                "same output",
                "final ok",
            ]),
        )),
    );
    shell.set_test_session_id("test-stall-hook");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run_session_command_driver(ctx),
    )
    .await
    .expect("run_session_command_driver should complete after shutdown");
    driver.await.unwrap();
    let _ = std::fs::remove_file(&counter_path);

    let events = sink.events();
    // Repetition handling is owned by the shared engine's StuckGuard. The current engine records
    // soft text repetition but does not expose it as a domain/UI event; importantly, it still
    // preserves stop-hook feedback in this same Run and eventually reaches one terminal event.
    assert!(
        events
            .iter()
            .any(|event| event.starts_with("SessionMessageStateChanged:")),
        "stop hook should publish message state while the shared Run continues: {:?}",
        events
    );
    // stall 后 Stop hook 阻断，应有第 4 次 LLM 响应（说明 detector 重置并继续了）
    assert!(
        events.iter().any(|e| e == "Text:final ok"),
        "loop should continue after stall + Stop hook block: {:?}",
        events
    );
    // 最终应完成
    assert_eq!(
        events
            .iter()
            .filter(|e| e.as_str() == "DoneWithDuration")
            .count(),
        1,
        "loop should finish: {:?}",
        events
    );
}

/// Channel-backed input port: 投递事件经 `recv_next_input` 阻塞返回，
/// drop 发送端关闭通道使 `recv_next_input` 返回 `None`（= shutdown）。
#[derive(Clone)]
struct ChannelInputEvents {
    rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<sdk::ChatInputEvent>>>,
    deferred: Arc<Mutex<VecDeque<sdk::ChatInputEvent>>>,
}

impl ChannelInputEvents {
    fn new() -> (
        tokio::sync::mpsc::UnboundedSender<sdk::ChatInputEvent>,
        Self,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (
            tx,
            Self {
                rx: Arc::new(tokio::sync::Mutex::new(rx)),
                deferred: Arc::new(Mutex::new(VecDeque::new())),
            },
        )
    }
}

impl crate::application::loop_engine::input_strategy::SessionInputPort for ChannelInputEvents {
    fn defer(&self, event: sdk::ChatInputEvent) {
        self.deferred.lock().unwrap().push_back(event);
    }
}

impl InputEventDrainPort for ChannelInputEvents {
    fn drain_input_events<'a>(
        &'a self,
    ) -> crate::application::loop_engine::chat::InputEventFuture<'a> {
        Box::pin(async move {
            let mut events: Vec<_> = self.deferred.lock().unwrap().drain(..).collect();
            let mut rx = self.rx.lock().await;
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            events
        })
    }

    fn recv_next_input<'a>(
        &'a self,
    ) -> crate::application::loop_engine::chat::InputEventOptFuture<'a> {
        Box::pin(async move {
            if let Some(event) = self.deferred.lock().unwrap().pop_front() {
                return Some(event);
            }
            let mut rx = self.rx.lock().await;
            rx.recv().await
        })
    }
}
