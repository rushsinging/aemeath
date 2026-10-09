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
fn test_global_data_paths_use_agents_directory() {
    // 用 env 隔离，避免污染真实 home/.agents
    let temp_agents_dir = std::env::temp_dir().join(format!(
        "aemeath_shared_paths_{}",
        UNIQUE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _guard = TestEnvGuard::set(AGENTS_DIR_ENV, &temp_agents_dir);

    assert_eq!(global_config_path(), temp_agents_dir.join("aemeath.json"));
    assert_eq!(global_agents_md_path(), temp_agents_dir.join("AGENTS.md"));
    assert_eq!(global_skills_dir(), temp_agents_dir.join("skills"));
    assert_eq!(global_logs_dir(), temp_agents_dir.join("logs"));
    assert_eq!(global_guidance_dir(), temp_agents_dir.join("guidance"));
    assert_eq!(global_memory_dir(), temp_agents_dir.join("memory"));
    assert_eq!(global_sessions_dir(), temp_agents_dir.join("sessions"));
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
fn test_systemone_models_dir_under_custom_agents_root() {
    let temp_agents_dir = std::env::temp_dir().join(format!(
        "aemeath_shared_models_{}",
        UNIQUE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _guard = TestEnvGuard::set(AGENTS_DIR_ENV, &temp_agents_dir);

    assert_eq!(global_models_dir(), temp_agents_dir.join("models"));
    assert_eq!(
        systemone_models_dir(),
        temp_agents_dir.join("models/systemone")
    );
}

#[test]
fn test_systemone_models_dir_env_blank_falls_back_to_home_layout() {
    let _guard = TestEnvGuard::set(AGENTS_DIR_ENV, "   ");
    let expected = home_dir_or_dot()
        .join(AGENTS_DIR_NAME)
        .join("models/systemone");
    assert_eq!(systemone_models_dir(), expected);
}

#[test]
fn test_systemone_models_dir_env_missing_falls_back_to_home_layout() {
    let _guard = TestEnvGuard::unset(AGENTS_DIR_ENV);
    let expected = home_dir_or_dot()
        .join(AGENTS_DIR_NAME)
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
