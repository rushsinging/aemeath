//! 会话运行态——UI 基础设施关注点。
//!
//! 与 `ConversationAggregate`（核心域：对话内容）分离。
//! 对话域产出的 `ConversationChange` 经映射层翻译为 `RuntimeState` 方法调用。

use super::compact_progress::CompactProgressModel;
use super::processing_job::{ProcessingJob, ProcessingStatus};
use super::spinner::{SpinnerModel, SpinnerPhase};
use super::status_notice::StatusNotice;
use super::task_status::TaskStatusSnapshot;
use super::usage::UsageSummary;
use super::workspace::WorkspaceState;
use std::time::Instant;

/// 会话运行态聚合——spinner / usage / workspace / status 等基础设施关注点。
///
/// TODO: 字段将逐步私有化，改为只经业务方法操作。
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeState {
    pub spinner: SpinnerModel,
    pub provider: Option<String>,
    pub model_id: Option<String>,
    pub workspace: WorkspaceState,
    pub usage: UsageSummary,
    pub live_tps: Option<f64>,
    pub task_status: TaskStatusSnapshot,
    pub processing_jobs: Vec<ProcessingJob>,
    pub status_notice: StatusNotice,
    pub thinking: bool,
    pub graph_phase: Option<String>,
    pub transient_notice_expiry: Option<Instant>,
    pub compact_progress: Option<CompactProgressModel>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            spinner: SpinnerModel::default(),
            provider: None,
            model_id: None,
            workspace: WorkspaceState::default(),
            usage: UsageSummary::default(),
            live_tps: None,
            task_status: TaskStatusSnapshot::default(),
            processing_jobs: Vec::new(),
            status_notice: StatusNotice::default(),
            thinking: true,
            graph_phase: None,
            transient_notice_expiry: None,
            compact_progress: None,
        }
    }
}

// ── 只读访问器（供 view assembler 读取） ──

impl RuntimeState {
    pub fn spinner(&self) -> &SpinnerModel {
        &self.spinner
    }
    pub fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }
    pub fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }
    pub fn workspace(&self) -> &WorkspaceState {
        &self.workspace
    }
    pub fn usage(&self) -> &UsageSummary {
        &self.usage
    }
    pub fn live_tps(&self) -> Option<f64> {
        self.live_tps
    }
    pub fn task_status(&self) -> &TaskStatusSnapshot {
        &self.task_status
    }
    pub fn processing_jobs(&self) -> &[ProcessingJob] {
        &self.processing_jobs
    }
    pub fn status_notice(&self) -> &StatusNotice {
        &self.status_notice
    }
    pub fn thinking(&self) -> bool {
        self.thinking
    }
    pub fn graph_phase(&self) -> Option<&str> {
        self.graph_phase.as_deref()
    }
    pub fn compact_progress(&self) -> Option<&CompactProgressModel> {
        self.compact_progress.as_ref()
    }
}

// ── 对话生命周期驱动的状态转换 ──

impl RuntimeState {
    /// 对话开始：激活 spinner。
    pub fn start_chat(&mut self) {
        self.spinner.chat_active = true;
        self.spinner.phase = Some(SpinnerPhase::Thinking);
    }

    /// 对话完成：停 spinner。
    pub fn complete_chat(&mut self) {
        self.spinner.chat_active = false;
        self.spinner.running_tool_count = 0;
        self.spinner.phase = None;
    }

    /// 生成文本中：phase → Generating。
    pub fn generate(&mut self) {
        self.spinner.phase = Some(SpinnerPhase::Generating);
    }

    /// 思考中：phase → Thinking。
    pub fn think(&mut self) {
        self.spinner.phase = Some(SpinnerPhase::Thinking);
    }

    /// 工具调用开始：running_tool_count++ + phase → CallingTool。
    pub fn start_tool_call(&mut self, name: &str) {
        self.spinner.running_tool_count += 1;
        self.spinner.phase = Some(SpinnerPhase::CallingTool(name.to_string()));
    }

    /// 工具调用完成：running_tool_count-- + 归零判断。
    pub fn complete_tool_call(&mut self) {
        self.spinner.running_tool_count = self.spinner.running_tool_count.saturating_sub(1);
        if self.spinner.running_tool_count == 0 {
            self.spinner.phase = Some(SpinnerPhase::Thinking);
        } else {
            self.spinner.phase = Some(SpinnerPhase::CallingTools {
                remaining: self.spinner.running_tool_count,
            });
        }
    }

    /// Agent 进度报告：phase → AgentWorking。
    pub fn report_agent_progress(&mut self) {
        self.spinner.phase = Some(SpinnerPhase::AgentWorking);
    }

    /// 暂停对话（AskUser）：spinner inactive。
    pub fn pause_chat(&mut self) {
        self.spinner.chat_active = false;
        self.spinner.phase = None;
    }

    /// 恢复对话（AskUser 应答后）：spinner active + Thinking。
    pub fn resume_chat(&mut self) {
        self.spinner.chat_active = true;
        self.spinner.phase = Some(SpinnerPhase::Thinking);
    }

    /// 异常中止：与 complete_chat 相同效果。
    pub fn abort_chat(&mut self) {
        self.spinner.chat_active = false;
        self.spinner.running_tool_count = 0;
        self.spinner.phase = None;
    }

    /// 强制空闲（resume 场景覆盖副作用）。
    pub fn force_idle(&mut self) {
        self.spinner.chat_active = false;
        self.spinner.phase = None;
        self.spinner.running_tool_count = 0;
    }

    /// Compact 开始：spinner active + Compacting。
    pub fn start_compact(&mut self) {
        self.spinner.chat_active = true;
        self.spinner.phase = Some(SpinnerPhase::Compacting);
    }

    /// Compact 结束（或异常中止）兜底清理（#540）：
    ///
    /// 集中清空 compact 关联的运行态字段，避免 MessagesSync 兜底路径遗漏：
    /// - `compact_progress` 清空 → 进度条消失
    /// - `running_tool_count` 清零 → 防止残留工具计数
    ///
    /// **不**触碰 `chat_active` / `phase`——这两个字段归 `spinner_stop()` / `pause_chat()` /
    /// `complete_chat()` 等对话生命周期方法管理，调用方按需叠加（#540 重构后
    /// MessagesSync 路径统一复用 `spinner_stop()` + 本方法）。
    pub fn clear_compact_runtime(&mut self) {
        self.compact_progress = None;
        self.spinner.running_tool_count = 0;
    }
}

// ── 临时 notice 过期逻辑 ──

impl RuntimeState {
    pub(crate) fn notice_from_phase(phase: Option<&str>) -> StatusNotice {
        match phase {
            None | Some("idle") => StatusNotice::success("Ready"),
            Some(p) => StatusNotice::normal(p.to_string()),
        }
    }

    /// 检查临时 notice 是否过期；过期则回退到 graph_phase 派生的持久态。
    pub fn expire_transient_notice(&mut self, now: Instant) -> bool {
        if self.transient_notice_expiry.is_some_and(|exp| now >= exp) {
            self.transient_notice_expiry = None;
            self.status_notice = Self::notice_from_phase(self.graph_phase.as_deref());
            return true;
        }
        false
    }
}

// ── 运行态 intent 的直接字段操作（纯运行态 intent 不经过对话域 change 映射） ──

impl RuntimeState {
    pub fn set_provider_model(&mut self, provider: Option<String>, model_id: Option<String>) {
        self.provider = provider;
        self.model_id = model_id;
    }

    pub fn update_workspace(&mut self, cwd: String, worktree: Option<String>) {
        self.workspace.cwd = Some(cwd);
        self.workspace.worktree = worktree;
    }

    pub fn set_workspace_snapshot(
        &mut self,
        path_base: Option<String>,
        workspace_root: Option<String>,
        branch: Option<String>,
        kind: super::workspace::WorktreeKind,
    ) {
        self.workspace.path_base = path_base;
        self.workspace.workspace_root = workspace_root;
        self.workspace.branch = branch;
        self.workspace.kind = kind;
    }

    pub fn record_usage(
        &mut self,
        input_tokens: u64,
        output_tokens: u64,
        last_input_tokens: u64,
        cost_usd: f64,
    ) -> (u64, u64, f64) {
        self.usage.input_tokens += input_tokens;
        self.usage.output_tokens += output_tokens;
        self.usage.last_input_tokens = last_input_tokens;
        self.usage.api_calls += 1;
        self.usage.cost_usd += cost_usd;
        (
            self.usage.input_tokens,
            self.usage.output_tokens,
            self.usage.cost_usd,
        )
    }

    pub fn set_context_size(&mut self, size: u64) -> (u64, u64, f64) {
        self.usage.context_size = size;
        (
            self.usage.input_tokens,
            self.usage.output_tokens,
            self.usage.cost_usd,
        )
    }

    pub fn update_last_input_tokens(&mut self, tokens: u64) -> (u64, u64, f64) {
        self.usage.last_input_tokens = tokens;
        (
            self.usage.input_tokens,
            self.usage.output_tokens,
            self.usage.cost_usd,
        )
    }

    pub fn set_live_tps(&mut self, tps: f64) {
        self.live_tps = Some(tps);
    }

    pub fn set_task_status(&mut self, total: usize, completed: usize, in_progress: usize) {
        self.task_status = TaskStatusSnapshot {
            total,
            completed,
            in_progress,
            lines: std::mem::take(&mut self.task_status.lines),
        };
    }

    pub fn set_task_lines(&mut self, lines: Vec<String>) {
        self.task_status.lines = lines;
    }

    pub fn start_processing_job(&mut self, id: String, chat_id: Option<String>) {
        self.processing_jobs.push(ProcessingJob {
            id,
            chat_id,
            status: ProcessingStatus::Running,
        });
    }

    pub fn finish_processing_job(&mut self, id: &str, success: bool) {
        if let Some(job) = self.processing_jobs.iter_mut().find(|job| job.id == id) {
            job.status = if success {
                ProcessingStatus::Finished
            } else {
                ProcessingStatus::Failed
            };
        }
    }

    pub fn set_status_notice(&mut self, notice: StatusNotice) {
        self.status_notice = notice;
        self.transient_notice_expiry = None;
    }

    pub fn set_transient_status_notice(&mut self, notice: StatusNotice, expires_at: Instant) {
        self.status_notice = notice;
        self.transient_notice_expiry = Some(expires_at);
    }

    pub fn set_thinking(&mut self, thinking: bool) {
        self.thinking = thinking;
    }

    pub fn set_graph_phase(&mut self, phase: Option<String>) {
        self.graph_phase = phase.clone();
        if self.transient_notice_expiry.is_none() {
            self.status_notice = Self::notice_from_phase(phase.as_deref());
        }
    }

    pub fn set_compact_progress(
        &mut self,
        stage: String,
        current: Option<u32>,
        total: Option<u32>,
    ) {
        self.compact_progress = Some(CompactProgressModel {
            stage,
            current,
            total,
        });
        self.start_compact();
    }

    /// 为兼容 spinner.rs 注释中 `model.spinner.phase = None` 的直接写法提供逃生通道。
    /// TODO: 逐步替换为业务方法调用后移除。
    pub fn spinner_mut(&mut self) -> &mut SpinnerModel {
        &mut self.spinner
    }
}
