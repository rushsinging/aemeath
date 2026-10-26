//! 会话级系统提示组装入口（#1863 W4）。
//!
//! 组装职责归 Context：本函数只负责把会话事实（工作区、模型、语言、配置快照）
//! 与 Hook 通知端口交给 `context::SystemPromptAssembly`，并把结果返回给组合根
//! 冻结进 prompt 装配数据。Runtime 不再拼接任何提示段落。

use std::sync::Arc;

use hook::HookDispatcher;
use share::config::domain::snapshot::ConfigSnapshot;

use crate::application::prompt::instructions_notifier::HookInstructionsNotifier;

/// 组装会话级静态系统提示。
///
/// `workspace_is_git` 由调用方探测后传入——Context 组装提示时不触发子进程。
pub async fn build_static_prompt(
    cwd: &std::path::Path,
    model: &str,
    reasoning: bool,
    config_file: Option<&ConfigSnapshot>,
    hook_port: &Arc<dyn HookDispatcher>,
    workspace_is_git: bool,
) -> String {
    let snapshot = match config_file {
        Some(snapshot) => snapshot.clone(),
        None => {
            share::config::domain::snapshot::ConfigSnapshot::new(share::config::Config::default())
        }
    };
    let notifier: Arc<dyn hook::InstructionsLoadedNotifier> = Arc::new(
        HookInstructionsNotifier::new(Arc::clone(hook_port), cwd.to_path_buf()),
    );
    let assembly = context::SystemPromptAssembly::new(Some(notifier));
    let inputs = context::SessionPromptInputsData {
        workspace_root: cwd.to_path_buf(),
        workspace_is_git,
        model_id: model.to_string(),
        reasoning,
        language: snapshot.language().to_string(),
        config_snapshot: snapshot,
    };
    assembly.materialize_session_prompt(&inputs).await
}
