use super::*;

/// 测试用环境变量守护：构造时设值，析构时还原。
/// 避免全局 env 污染其它测试。
static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 测试用唯一序号（避免读时钟——shared kernel 禁用 SystemTime::now）。
static UNIQUE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct TestEnvGuard {
    key: &'static str,
    old: Option<std::ffi::OsString>,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl TestEnvGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let guard = TEST_ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner());
        let old = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self {
            key,
            old,
            _guard: guard,
        }
    }

    fn unset(key: &'static str) -> Self {
        let guard = TEST_ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner());
        let old = std::env::var_os(key);
        unsafe {
            std::env::remove_var(key);
        }
        Self {
            key,
            old,
            _guard: guard,
        }
    }
}

impl Drop for TestEnvGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(old) = &self.old {
                std::env::set_var(self.key, old);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

#[test]
fn test_project_paths_use_agents_directory() {
    let cwd = PathBuf::from("/tmp/demo");
    assert_eq!(
        project_config_path(&cwd),
        PathBuf::from("/tmp/demo/.agents/aemeath.json")
    );
    assert_eq!(
        project_agents_md_path(&cwd),
        PathBuf::from("/tmp/demo/AGENTS.md")
    );
    assert_eq!(
        old_project_claude_md_path(&cwd),
        PathBuf::from("/tmp/demo/CLAUDE.md")
    );
    assert_eq!(
        project_claude_settings_path(&cwd),
        PathBuf::from("/tmp/demo/.claude/settings.json")
    );
    assert_eq!(
        project_claude_skills_dir(&cwd),
        PathBuf::from("/tmp/demo/.claude/skills")
    );
    assert_eq!(
        project_skills_dir(&cwd),
        PathBuf::from("/tmp/demo/.agents/skills")
    );
}

#[test]
fn test_global_config_and_knowledge_paths_use_agents_directory() {
    // #2179：配置、worktrees、通用知识与 skills 仍在 ~/.agents。
    let temp_agents_dir = std::env::temp_dir().join(format!(
        "aemeath_shared_paths_{}",
        UNIQUE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _guard = TestEnvGuard::set(AGENTS_DIR_ENV, &temp_agents_dir);

    assert_eq!(global_config_path(), temp_agents_dir.join("aemeath.json"));
    assert_eq!(global_agents_md_path(), temp_agents_dir.join("AGENTS.md"));
    assert_eq!(global_skills_dir(), temp_agents_dir.join("skills"));
    assert_eq!(global_worktrees_dir(), temp_agents_dir.join("worktrees"));
    assert_eq!(global_hooks_dir(), temp_agents_dir.join("hooks"));
    assert_eq!(global_mcp_config_path(), temp_agents_dir.join("mcp.json"));
    assert_eq!(global_history_path(), temp_agents_dir.join("history.json"));
    assert_eq!(
        global_settings_path(),
        temp_agents_dir.join("settings.json")
    );
}

#[test]
fn test_runtime_state_and_private_asset_paths_use_aemeath_home() {
    // #2179：运行时状态与模型/日志/guidance 落到 ~/.aemeath。
    let temp_home = std::env::temp_dir().join(format!(
        "aemeath_shared_home_{}",
        UNIQUE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _guard = TestEnvGuard::set(AEMEATH_HOME_DIR_ENV, &temp_home);

    let state = temp_home.join("state");
    assert_eq!(global_logs_dir(), temp_home.join("logs"));
    assert_eq!(global_guidance_dir(), temp_home.join("guidance"));
    assert_eq!(global_memory_dir(), state.join("memory"));
    assert_eq!(global_sessions_dir(), state.join("sessions"));
    assert_eq!(global_session_datasets_dir(), state.join("session"));
    assert_eq!(global_tool_results_dir(), state.join("tool-results"));
    assert_eq!(global_harness_dir(), state.join("harness"));
    assert_eq!(global_scoring_dir(), state.join("scoring"));
    assert_eq!(global_workflows_dir(), state.join("workflows"));
    assert_eq!(global_audit_dir(), state.join("audit"));
    assert_eq!(
        global_background_process_dir(),
        state.join("background-process")
    );
    assert_eq!(global_background_task_dir(), state.join("background-task"));
    assert_eq!(global_meta_dir(), state.join("meta"));
    assert_eq!(global_tmp_dir(), state.join("tmp"));
    assert_eq!(global_models_dir(), temp_home.join("models"));
    assert_eq!(systemone_models_dir(), temp_home.join("models/systemone"));
}

#[test]
fn test_systemone_models_dir_under_custom_aemeath_home() {
    let temp_home = std::env::temp_dir().join(format!(
        "aemeath_shared_models_{}",
        UNIQUE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _guard = TestEnvGuard::set(AEMEATH_HOME_DIR_ENV, &temp_home);

    assert_eq!(global_models_dir(), temp_home.join("models"));
    assert_eq!(systemone_models_dir(), temp_home.join("models/systemone"));
}

#[test]
fn test_systemone_models_dir_env_blank_falls_back_to_home_layout() {
    // 空白 AEMEATH_HOME_DIR 视为未设置，回退 home/.aemeath 布局。
    let _guard = TestEnvGuard::set(AEMEATH_HOME_DIR_ENV, "   ");
    let expected = home_dir_or_dot()
        .join(AEMEATH_HOME_DIR_NAME)
        .join("models/systemone");
    assert_eq!(systemone_models_dir(), expected);
}

#[test]
fn test_systemone_models_dir_env_missing_falls_back_to_home_layout() {
    let _guard = TestEnvGuard::unset(AEMEATH_HOME_DIR_ENV);
    let expected = home_dir_or_dot()
        .join(AEMEATH_HOME_DIR_NAME)
        .join("models/systemone");
    assert_eq!(systemone_models_dir(), expected);
}

#[test]
fn test_old_global_claude_md_path_uses_home_directory() {
    let expected = home_dir_or_dot().join(CLAUDE_DIR_NAME).join(CLAUDE_MD);
    assert_eq!(old_global_claude_md_path(), expected);
}

#[test]
fn test_global_agents_dir_falls_back_to_home() {
    // 无 env 时必须落到 home/.agents（而非相对路径 .agents）
    let _guard = TestEnvGuard::unset(AGENTS_DIR_ENV);
    let expected = home_dir_or_dot().join(AGENTS_DIR_NAME);
    assert_eq!(global_agents_dir(), expected);
}

#[test]
fn test_expand_home() {
    let home = home_dir_or_dot();
    assert_eq!(expand_home(Path::new("~")), home);
    assert_eq!(expand_home(Path::new("~/foo/bar")), home.join("foo/bar"));
    // 非 ~ 前缀原样返回
    assert_eq!(
        expand_home(Path::new("/abs/path")),
        PathBuf::from("/abs/path")
    );
    assert_eq!(
        expand_home(Path::new("relative")),
        PathBuf::from("relative")
    );
}

#[test]
fn test_global_agents_dir_env_empty_string_falls_back_to_home() {
    // env 设了但为空，应回退到 home/.agents（而非空路径）
    let _guard = TestEnvGuard::set(AGENTS_DIR_ENV, "   ");
    let expected = home_dir_or_dot().join(AGENTS_DIR_NAME);
    assert_eq!(global_agents_dir(), expected);
}

#[test]
fn test_project_instruction_dirs_includes_cwd_and_ancestors() {
    let cwd = PathBuf::from("/a/b/c/d");
    let dirs = project_instruction_dirs(&cwd, 2);
    assert_eq!(
        dirs,
        vec![
            PathBuf::from("/a/b/c/d"),
            PathBuf::from("/a/b/c"),
            PathBuf::from("/a/b"),
        ]
    );
}

#[test]
fn test_project_instruction_dirs_depth_zero_cwd_only() {
    let cwd = PathBuf::from("/a/b");
    let dirs = project_instruction_dirs(&cwd, 0);
    assert_eq!(dirs, vec![PathBuf::from("/a/b")]);
}

#[test]
fn test_old_project_paths_use_aemeath_and_claude() {
    let cwd = PathBuf::from("/tmp/demo");
    assert_eq!(
        old_project_config_path(&cwd),
        PathBuf::from("/tmp/demo/.aemeath/config.json")
    );
    assert_eq!(
        old_project_claude_md_path(&cwd),
        PathBuf::from("/tmp/demo/CLAUDE.md")
    );
    assert_eq!(
        project_claude_settings_path(&cwd),
        PathBuf::from("/tmp/demo/.claude/settings.json")
    );
    assert_eq!(
        project_claude_skills_dir(&cwd),
        PathBuf::from("/tmp/demo/.claude/skills")
    );
    assert_eq!(
        old_project_skills_dir(&cwd),
        PathBuf::from("/tmp/demo/.aemeath/skills")
    );
}

#[test]
fn test_aemeath_home_dir_defaults_to_dot_aemeath_under_home() {
    let home = PathBuf::from("/home/tester");
    assert_eq!(
        aemeath_home_dir_from(None, Some(home.clone())),
        home.join(AEMEATH_HOME_DIR_NAME)
    );
}

#[test]
fn test_aemeath_home_dir_env_override_absolute_path() {
    let home = PathBuf::from("/home/tester");
    assert_eq!(
        aemeath_home_dir_from(Some("/opt/aemeath".to_string()), Some(home)),
        PathBuf::from("/opt/aemeath")
    );
}

#[test]
fn test_aemeath_home_dir_env_override_expands_tilde() {
    let home = PathBuf::from("/home/tester");
    assert_eq!(
        aemeath_home_dir_from(Some("~/private-home".to_string()), Some(home.clone())),
        home.join("private-home")
    );
    assert_eq!(
        aemeath_home_dir_from(Some("~".to_string()), Some(home.clone())),
        home
    );
}

#[test]
fn test_aemeath_home_dir_env_override_keeps_relative_path() {
    // 与 global_agents_dir 同语义：非 `~` 前缀原样返回，不做 cwd 展开。
    let home = PathBuf::from("/home/tester");
    assert_eq!(
        aemeath_home_dir_from(Some("relative/home".to_string()), Some(home)),
        PathBuf::from("relative/home")
    );
}

#[test]
fn test_aemeath_home_dir_env_blank_falls_back_to_home_layout() {
    let home = PathBuf::from("/home/tester");
    for blank in ["   ", ""] {
        assert_eq!(
            aemeath_home_dir_from(Some(blank.to_string()), Some(home.clone())),
            home.join(AEMEATH_HOME_DIR_NAME)
        );
    }
}

#[test]
fn test_aemeath_home_dir_missing_home_falls_back_to_relative() {
    // HOME 缺失时不得 panic，也不得生成绝对空路径。
    assert_eq!(
        aemeath_home_dir_from(None, None),
        PathBuf::from(".").join(AEMEATH_HOME_DIR_NAME)
    );
}

#[test]
fn test_aemeath_state_dir_and_leaf() {
    let home = PathBuf::from("/home/tester");
    let expected_root = home.join(AEMEATH_HOME_DIR_NAME).join(STATE_DIR_NAME);
    assert_eq!(
        aemeath_home_dir_from(None, Some(home.clone())).join(STATE_DIR_NAME),
        expected_root
    );
    assert_eq!(
        expected_root.join("sessions"),
        home.join(AEMEATH_HOME_DIR_NAME)
            .join(STATE_DIR_NAME)
            .join("sessions")
    );
    let _guard = TestEnvGuard::set(AEMEATH_HOME_DIR_ENV, "/tmp/aemeath-home-test");
    assert_eq!(aemeath_home_dir(), PathBuf::from("/tmp/aemeath-home-test"));
    assert_eq!(
        aemeath_state_dir(),
        PathBuf::from("/tmp/aemeath-home-test/state")
    );
    assert_eq!(
        aemeath_state_leaf_dir("sessions"),
        PathBuf::from("/tmp/aemeath-home-test/state/sessions")
    );
}

#[test]
fn test_tool_guidance_canonical_path_lives_under_aemeath_home() {
    // 环境锁不支持嵌套持有：分两段覆盖两个 env 键。
    {
        let _home = TestEnvGuard::set(AEMEATH_HOME_DIR_ENV, "/tmp/aemeath-home-test");
        assert_eq!(
            global_tool_guidance_path(),
            PathBuf::from("/tmp/aemeath-home-test/tool_guidance.md")
        );
    }
    let _agents = TestEnvGuard::set(AGENTS_DIR_ENV, "/tmp/agents-root");
    assert_eq!(
        legacy_global_tool_guidance_path(),
        PathBuf::from("/tmp/agents-root/tool_guidance.md")
    );
}

#[test]
fn test_resolved_tool_guidance_prefers_canonical_then_legacy() {
    let canonical = PathBuf::from("/home/u/.aemeath/tool_guidance.md");
    let legacy = PathBuf::from("/home/u/.agents/tool_guidance.md");

    // 两者都存在：只用 canonical，不合并。
    assert_eq!(
        resolve_canonical_then_legacy(canonical.clone(), legacy.clone(), |_| true),
        canonical
    );
    // 仅旧存在：只读 fallback 到 legacy。
    assert_eq!(
        resolve_canonical_then_legacy(canonical.clone(), legacy.clone(), |path| path == legacy),
        legacy
    );
    // 都不存在：返回 canonical（调用方静默跳过或新建）。
    assert_eq!(
        resolve_canonical_then_legacy(canonical.clone(), legacy.clone(), |_| false),
        canonical
    );
}

#[test]
fn test_config_and_worktrees_paths_ignore_aemeath_home_dir() {
    // #2179：配置与 worktrees 保持在 ~/.agents，不受 AEMEATH_HOME_DIR 影响。
    {
        let _agents = TestEnvGuard::set(AGENTS_DIR_ENV, "/tmp/agents-root");
        assert_eq!(
            global_config_path(),
            PathBuf::from("/tmp/agents-root/aemeath.json")
        );
        assert_eq!(
            global_worktrees_dir(),
            PathBuf::from("/tmp/agents-root/worktrees")
        );
        assert_eq!(
            global_agents_md_path(),
            PathBuf::from("/tmp/agents-root/AGENTS.md")
        );
        assert_eq!(
            global_skills_dir(),
            PathBuf::from("/tmp/agents-root/skills")
        );
    }
    // 同一把 env 锁不支持嵌套持有：先释放 AGENTS_DIR 覆盖，再单独覆盖
    // AEMEATH_HOME_DIR，验证它不会改变配置与 worktrees 的 home 布局。
    let _home = TestEnvGuard::set(AEMEATH_HOME_DIR_ENV, "/tmp/aemeath-home-test");
    let expected = home_dir_or_dot().join(AGENTS_DIR_NAME);
    assert_eq!(aemeath_home_dir(), PathBuf::from("/tmp/aemeath-home-test"));
    assert_eq!(global_config_path(), expected.join(NEW_CONFIG_FILE));
    assert_eq!(global_worktrees_dir(), expected.join(WORKTREES_DIR_NAME));
}

#[test]
fn test_project_instruction_resolution_prefers_agents_then_claude() {
    let dir = PathBuf::from("/repo");
    let agents = dir.join(AGENTS_MD);
    let claude = dir.join(CLAUDE_MD);

    // 两者都存在：只用 AGENTS.md，不合并。
    assert_eq!(
        resolve_project_instruction_path_with(&dir, |_| true),
        Some(agents.clone())
    );
    // 仅 CLAUDE.md 存在：fallback。
    assert_eq!(
        resolve_project_instruction_path_with(&dir, |path| path == claude),
        Some(claude)
    );
    // 都不存在：None（不在此处创建）。
    assert_eq!(resolve_project_instruction_path_with(&dir, |_| false), None);
}

#[test]
fn test_project_instruction_write_target_falls_back_to_new_agents_file() {
    let dir = PathBuf::from("/repo");
    assert_eq!(
        project_instruction_write_target_with(&dir, |_| false),
        dir.join(AGENTS_MD)
    );
    assert_eq!(
        project_instruction_write_target_with(&dir, |path| path == dir.join(CLAUDE_MD)),
        dir.join(CLAUDE_MD)
    );
}
