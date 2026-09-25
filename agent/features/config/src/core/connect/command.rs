//! Connect 向导的 typed 命令。
//!
//! ## 设计目标
//!
//! - 客户端通过 [`crate::connect::ConnectCommand`] 推进状态；
//! - 每个命令携带 session id 与 expected revision；
//! - 非法 stage / 过期 revision / 未知 Catalog / 重复终态返回
//!   [`crate::connect::ConnectError`] typed error，**无副作用**。
//!
//! ## 阶段 → 命令映射表
//!
//! | 当前阶段              | 合法命令                                                                                    |
//! |----------------------|---------------------------------------------------------------------------------------------|
//! | SelectProvider       | `SelectProvider`                                                                            |
//! | ConfirmOverwrite     | `ConfirmOverwrite` / `RejectOverwrite`                                                      |
//! | EditEndpoint         | `SetEndpoint`                                                                               |
//! | EditCredential       | `SetCredential`                                                                             |
//! | EditUserAgent        | `SetProviderUserAgent`                                                                      |
//! | SelectModel          | `SelectRecommendedModel` / `EnterCustomModel`                                               |
//! | EditCustomModel      | `SetCustomModel`                                                                            |
//! | ChooseGlobalDefault  | `SetGlobalDefault`                                                                          |
//! | ChooseProbe          | `SkipProbe` / `BeginProbe`                                                                  |
//! | Probing              | `ContinueAfterProbeFailure` / `EditAfterProbeFailure`（取决于探测结果）                      |
//! | Review               | `ConfirmSave`                                                                               |
//! | Saving               | `ConfirmSave`（重试）                                                                        |
//! | 任意阶段              | `Cancel`（除 Completed 外）                                                                 |
//!
//! `ConnectAppService` 在 [`crate::connect::service`] 维护该映射。

use crate::catalog::ProviderSource;

/// Connect 命令。
///
/// 每个变体都要求客户端提供**与命令无关**的 session id 与预期 revision。
/// 服务端在校验通过前**不**修改 session 状态；任何 typed error 都是无副作用的。
#[derive(Debug, Clone)]
pub enum ConnectCommand {
    /// 返回当前阶段的上一编辑页；SelectProvider 与异步/终态阶段拒绝该命令。
    Back,

    // --- SelectProvider / ConfirmOverwrite ---
    /// 选择固定 source，迁移至 EditEndpoint（或经 ConfirmOverwrite）。
    SelectProvider {
        source: ProviderSource,
    },
    /// 进入完全自定义 Provider 编辑页（名称 / driver / endpoint 全手填）。
    BeginCustomProvider,
    /// 提交自定义 Provider 三要素；source 名为用户输入（无需在 Catalog）。
    SelectCustomProvider {
        name: String,
        driver: String,
        base_url: String,
    },
    /// 确认覆盖现有 Provider，迁移至 EditEndpoint 并以 existing 预填 draft。
    ConfirmOverwrite,
    /// 拒绝覆盖，回退至 SelectProvider 并清空已选 source / driver。
    RejectOverwrite,

    // --- EditEndpoint ---
    /// 设置 base URL，归一化为合法 http(s)；`api_style` 仅 OpenAI 系 driver
    /// 有意义（`Some("responses")` 走 Responses API，`None` 为 Chat Completions）。
    SetEndpoint {
        base_url: String,
        api_style: Option<String>,
    },

    // --- EditCredential ---
    /// 设置 API key。空字符串映射为 `has_api_key = false`，但仍保留字段语义。
    SetCredential {
        api_key: String,
    },

    // --- EditUserAgent ---
    /// 设置 Provider 专属 UA。`raw = None` / `raw = Some("")` 清除覆盖；
    /// 含控制字符的输入返回 Validation 错误。
    SetProviderUserAgent {
        raw: Option<String>,
    },

    // --- SelectModel / EditCustomModel ---
    /// 模型页提交完整所选集合（推荐与自定义的任意组合，整体替换）。
    SetSelectedModels {
        models: Vec<super::draft::ModelDraft>,
    },
    /// 切换至 EditCustomModel 阶段（添加 / 编辑模型，可无限次进入）。
    EnterCustomModel,
    /// 在 EditCustomModel 阶段提交模型字段；model_id 命中已有条目即编辑
    /// （替换属性），否则追加，随后返回模型页。
    UpsertCustomModel {
        model: super::draft::ModelDraft,
    },

    // --- ChooseGlobalDefault ---
    SetGlobalDefault {
        set_as_default: bool,
    },

    // --- ChooseProbe ---
    /// 跳过 Probe 直接进入 Review。
    SkipProbe,
    /// 发起 Probe；服务内部调用 [`crate::ports::ProviderProbePort`]，返回
    /// 后根据结果将 session 留在 Probing 阶段，等待显式指令。
    BeginProbe,
    /// 探测完成（包括成功 / 失败）后，用户显式接受 Probe 结果并进入 Review。
    /// Probe 失败时该命令是 spec 要求的"继续保存"显式选择；成功时是"查看
    /// Review"的常规入口。仅在 Probing 阶段合法。
    ContinueAfterProbe,
    /// 探测失败时返回编辑；仅在 Probing 失败时合法。
    EditAfterProbeFailure,

    // --- Review / Saving ---
    /// 提交 draft 至 [`crate::connect::ConnectCommitPort`]。
    /// - 在 Review 阶段首次调用，迁移至 Saving；
    /// - 在 Saving 阶段重试已失败提交。
    ConfirmSave,
}

/// 单条命令的 stage 前置条件（白名单）。由 service 在内部对照 session 当前
/// stage 调用；若不在白名单，返回 [`crate::connect::ConnectError::InvalidTransition`]。
///
/// 该函数仅服务自己使用——客户端**不**依赖；它只是把规则集中在 production
/// 代码内，方便测试覆盖。
pub(crate) fn expected_stages(command: &ConnectCommand) -> &'static [super::states::ConnectStage] {
    use super::states::ConnectStage::*;
    match command {
        ConnectCommand::Back => &[
            ConfirmOverwrite,
            EditEndpoint,
            EditCredential,
            EditUserAgent,
            SelectModel,
            EditCustomModel,
            ChooseGlobalDefault,
            ChooseProbe,
            Probing,
            Review,
        ],
        ConnectCommand::SelectProvider { .. } => &[SelectProvider],
        ConnectCommand::BeginCustomProvider => &[SelectProvider],
        ConnectCommand::SelectCustomProvider { .. } => &[EditCustomProvider],
        ConnectCommand::ConfirmOverwrite | ConnectCommand::RejectOverwrite => &[ConfirmOverwrite],
        ConnectCommand::SetEndpoint { .. } => &[EditEndpoint],
        ConnectCommand::SetCredential { .. } => &[EditCredential],
        ConnectCommand::SetProviderUserAgent { .. } => &[EditUserAgent],
        ConnectCommand::SetSelectedModels { .. } | ConnectCommand::EnterCustomModel => {
            &[SelectModel]
        }
        ConnectCommand::UpsertCustomModel { .. } => &[EditCustomModel],
        ConnectCommand::SetGlobalDefault { .. } => &[ChooseGlobalDefault],
        ConnectCommand::SkipProbe | ConnectCommand::BeginProbe => &[ChooseProbe],
        ConnectCommand::ContinueAfterProbe | ConnectCommand::EditAfterProbeFailure => &[Probing],
        ConnectCommand::ConfirmSave => &[Review, Saving],
    }
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod command_tests;
