//! `InstructionsLoadedNotifier` 的 Hook 派发实现（#1863 W4）。
//!
//! 端口由 Hook BC 定义、Context 在组装系统提示时触发；本类型把通知翻译成
//! `InstructionsLoaded` hook 订阅派发，是 Runtime 装配期的唯一桥接点。

use std::path::PathBuf;
use std::sync::Arc;

use hook::{HookDispatchContextData, HookDispatcher, HookInvocationData};

/// 把指令加载通知转发给 Hook 订阅。
pub struct HookInstructionsNotifier {
    hooks: Arc<dyn HookDispatcher>,
    workspace_root: PathBuf,
}

impl HookInstructionsNotifier {
    pub fn new(hooks: Arc<dyn HookDispatcher>, workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            hooks,
            workspace_root: workspace_root.into(),
        }
    }
}

impl share::instructions::InstructionsLoadedNotifier for HookInstructionsNotifier {
    fn instructions_loaded(&self, file_path: &str, instruction_type: &str) {
        // 通知是 fire-and-forget：触发方（Context 组装系统提示）不等待 hook
        // 子进程；此处把派发投递到当前 runtime，无 runtime 时静默丢弃
        // （测试/离线组装场景不应因此失败）。
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let hooks = Arc::clone(&self.hooks);
        let workspace_root = self.workspace_root.clone();
        let invocation = HookInvocationData::InstructionsLoaded {
            file_path: file_path.to_string(),
            instruction_type: instruction_type.to_string(),
        };
        handle.spawn(async move {
            let _ = hooks
                .dispatch_at(
                    invocation,
                    HookDispatchContextData::new(&workspace_root),
                    &tokio_util::sync::CancellationToken::new(),
                )
                .await;
        });
    }
}
