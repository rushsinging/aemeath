/// Header 渲染策略
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderPolicy {
    /// 标准 header：带 ● marker
    Standard,
    /// 紧凑 header：单行，无 marker（如 TaskUpdate）
    Compact,
    /// 自定义图标：用指定 emoji（如 📋 EnterPlanMode）
    CustomIcon(&'static str),
}

/// Details 渲染策略
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailsPolicy {
    /// 展开显示 details
    Expanded,
    /// 隐藏 details
    Hidden,
}

/// Result 渲染策略
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultPolicy {
    /// 不显示 result 子块（如 Read/Write/Edit）
    Hidden,
    /// 显示 result 子块
    Visible {
        /// 最大行数（None 表示全部显示，如 Edit diff）
        max_lines: Option<usize>,
        /// 渲染类型
        render_kind: ResultRender,
        /// tail 模式：只显示最后 N 行（如 Bash）
        tail_mode: bool,
    },
}

/// 工具的渲染策略配置
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolRenderPolicy {
    pub header: HeaderPolicy,
    pub details: DetailsPolicy,
    pub result: ResultPolicy,
}

/// 工具 result 的渲染类型。由工具显式声明，渲染层据此分发。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultRender {
    /// 纯文本原样预览。
    Plain,
    /// unified diff。
    Diff,
}
