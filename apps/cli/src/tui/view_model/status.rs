use super::style::SemanticStyle;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusNoticeViewKind {
    #[default]
    Normal,
    Success,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusNoticeViewModel {
    pub text: String,
    pub kind: StatusNoticeViewKind,
}

impl Default for StatusNoticeViewModel {
    fn default() -> Self {
        Self {
            text: "Ready".to_string(),
            kind: StatusNoticeViewKind::Normal,
        }
    }
}

/// 视图层自有的工作目录类型枚举，避免 view_model 依赖 model 内部类型。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusWorktreeKind {
    #[default]
    Main,
    Worktree,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatusViewModel {
    pub notice: StatusNoticeViewModel,
    pub runtime: StatusRuntimeViewModel,
    pub line: StatusLineViewModel,
    pub thinking: bool,
}

/// 状态栏运行态视图模型：StatusBar 渲染所需 token/tps/model/session/context 的唯一派生表示。
///
/// 真相来自 `RuntimeModel`/`SessionModel`（经 `StatusViewAssembler` 派生），StatusBar 不再
/// 自持镜像字段，渲染直接读取本 ViewModel。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatusRuntimeViewModel {
    pub model: Option<String>,
    pub session_id: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub last_input_tokens: u64,
    pub api_calls: u64,
    pub context_size: u64,
    pub tps: f64,
    pub context: StatusContextViewModel,
}

/// 工作目录上下文视图模型（StatusBar 第二行的 model 派生字段；
/// permission_mode 为启动期配置，不由本模型承载）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusContextViewModel {
    pub path_base: String,
    pub working_root: String,
    pub branch: Option<String>,
    pub kind: StatusWorktreeKind,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StatusLineViewModel {
    pub left: Vec<StatusSegment>,
    pub center: Vec<StatusSegment>,
    pub right: Vec<StatusSegment>,
    pub severity: StatusSeverity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusSegment {
    pub key: String,
    pub text: String,
    pub style: SemanticStyle,
    pub priority: u8,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusSeverity {
    #[default]
    Normal,
    Info,
    Warning,
    Error,
}
