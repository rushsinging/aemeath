//! 经验库（lesson corpus）：RSI 改进经验的持久化与复利输入。
//!
//! 设计契约：`docs/design/02-modules/harness/04-evidence-and-library.md`。
//! 独立于 memory corpus——路径、schema、写路径全部分离，NEVER 混用。

use crate::constants::ERROR_DOMAIN;
use crate::domain::component::ComponentId;
use crate::domain::errors::HarnessError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// SignalKind 统一定义于 domain::evidence（#2048 扩展六类，
// CycleObservation 保留为兜底信号；旧 ManualContext 迁移语义由
// CycleObservation 覆盖——手动触发上下文即周期观察）。
pub use crate::domain::evidence::SignalKind;

/// 证据绑定：每条被采纳的 lesson MUST 至少携带一条。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceBinding {
    /// 来源标识（周期 id / run id / 触发请求 id）。
    pub run_ref: String,
    pub signal: SignalKind,
    /// 行为摘录（有界）。
    pub excerpt: String,
}

/// 经验条目状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LessonStatus {
    Active,
    Retired,
}

/// 改进生效判定状态（#2080，对齐 Harness² helped/hurt/confounded）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    /// 待验证：尚未拿到「改进后」的新证据。
    #[default]
    Pending,
    /// 已生效：目标信号在改进后的新证据中消失。
    Verified,
    /// 未生效：目标信号仍出现——规则无效，应作为重修输入。
    Ineffective,
}

/// 单条经验的验证状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LessonVerification {
    pub status: VerificationStatus,
    /// 本经验要解决的信号 id（判定依据；空 = 不参与验证，旧条目）。
    #[serde(default)]
    pub target_signal: String,
    /// 判定时刻（Unix 秒；0 = 未判定——幂等键）。
    #[serde(default)]
    pub checked_at: u64,
}

/// 单条经验。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LessonEntry {
    /// 稳定唯一 id（UUIDv7）。
    pub id: String,
    /// 指向哪类组件。
    pub component_kind: String,
    /// 可复用结论（有界长度）。
    pub lesson: String,
    pub evidence: Vec<EvidenceBinding>,
    pub adopted_at: u64,
    pub status: LessonStatus,
    /// 改进生效判定（#2080；旧条目缺该字段时按默认 Pending/空 target）。
    #[serde(default)]
    pub verification: LessonVerification,
}

impl LessonEntry {
    /// 构造 Active 新条目（校验证据与长度不变量）。
    pub fn new(
        id: String,
        component_kind: impl Into<String>,
        lesson: impl Into<String>,
        evidence: Vec<EvidenceBinding>,
        adopted_at: u64,
    ) -> Result<Self, HarnessError> {
        let lesson = lesson.into();
        let component_kind = component_kind.into();
        if evidence.is_empty() {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                "经验条目 MUST 携带至少一条证据绑定",
            ));
        }
        if lesson.chars().count() > crate::constants::LESSON_MAX_CHARS {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!(
                    "经验条目超长（>{} 字符），请收敛表述",
                    crate::constants::LESSON_MAX_CHARS
                ),
            ));
        }
        for binding in &evidence {
            if binding.excerpt.chars().count() > crate::constants::EXCERPT_MAX_CHARS {
                return Err(HarnessError::invalid(
                    ERROR_DOMAIN,
                    format!(
                        "证据摘录超长（>{} 字符）",
                        crate::constants::EXCERPT_MAX_CHARS
                    ),
                ));
            }
        }
        Ok(Self {
            id,
            component_kind,
            lesson,
            evidence,
            adopted_at,
            status: LessonStatus::Active,
            verification: LessonVerification::default(),
        })
    }
}

/// 单条验证判定结果（供事件流与报告）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationOutcome {
    pub lesson_id: String,
    pub component_kind: String,
    pub target_signal: String,
    pub status: VerificationStatus,
}

/// 验证判定（#2080，纯函数）：只判 Active + Pending + 有 target_signal
/// 且未判定过的条目。
///
/// - 无新证据（`has_new_evidence == false`）→ 保持 Pending（样本不足，
///   NEVER 编造结论）
/// - 目标信号仍出现 → Ineffective（规则未挡住）
/// - 目标信号消失 → Verified
///
/// 信号匹配用「精确或互相包含」：错误文本可能被截断，严格相等会漏判。
pub fn judge_verification(
    lesson: &LessonEntry,
    current_signals: &[String],
    has_new_evidence: bool,
) -> Option<VerificationOutcome> {
    if lesson.status != LessonStatus::Active
        || lesson.verification.status != VerificationStatus::Pending
        || lesson.verification.target_signal.trim().is_empty()
        || lesson.verification.checked_at != 0
        || !has_new_evidence
    {
        return None;
    }
    let target = lesson.verification.target_signal.as_str();
    let still_present = current_signals.iter().any(|signal| {
        signal == target || signal.contains(target) || target.contains(signal.as_str())
    });
    Some(VerificationOutcome {
        lesson_id: lesson.id.clone(),
        component_kind: lesson.component_kind.clone(),
        target_signal: lesson.verification.target_signal.clone(),
        status: if still_present {
            VerificationStatus::Ineffective
        } else {
            VerificationStatus::Verified
        },
    })
}

/// JSONL 文件经验库。
pub struct LessonStore {
    path: PathBuf,
}

impl LessonStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// 追加一条经验（append-only）。
    pub fn append(&self, entry: &LessonEntry) -> Result<(), HarnessError> {
        use std::io::Write;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                HarnessError::storage(ERROR_DOMAIN, format!("创建经验库目录失败：{error}"))
            })?;
        }
        let line = serde_json::to_string(entry).map_err(|error| {
            HarnessError::invalid(ERROR_DOMAIN, format!("经验条目序列化失败：{error}"))
        })?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| {
                HarnessError::storage(ERROR_DOMAIN, format!("打开经验库失败：{error}"))
            })?;
        writeln!(file, "{line}").map_err(|error| {
            HarnessError::storage(ERROR_DOMAIN, format!("写入经验库失败：{error}"))
        })
    }

    /// 读取全部条目（损坏行跳过并 warn，NEVER 整库失败）。
    pub fn load_all(&self) -> Result<Vec<LessonEntry>, HarnessError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(HarnessError::storage(
                    ERROR_DOMAIN,
                    format!("读取经验库失败：{error}"),
                ))
            }
        };
        let mut entries = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<LessonEntry>(line) {
                Ok(entry) => entries.push(entry),
                Err(error) => {
                    log::warn!(target: crate::LOG_TARGET, "经验库损坏行已跳过：{error}");
                }
            }
        }
        Ok(entries)
    }

    /// Active lessons（改进周期的复利输入）。
    pub fn load_active(&self) -> Result<Vec<LessonEntry>, HarnessError> {
        Ok(self
            .load_all()?
            .into_iter()
            .filter(|entry| entry.status == LessonStatus::Active)
            .collect())
    }

    /// 将指定 id 标记 Retired（重写文件：经验库小，全量重写可接受）。
    /// 退役某组件的全部 Active lesson（id 尾缀 ":{kind}/{name}" 匹配；
    /// #2065 B：新提案覆盖组件内容后，旧 lesson 与新现实矛盾——
    /// 继续注入会误导后续周期（复利毒药），NEVER 留脏 lesson）。
    pub fn retire_for_component(&self, component_id: &ComponentId) -> Result<usize, HarnessError> {
        let suffix = format!(":{component_id}");
        let entries = self.load_all()?;
        let mut retired_count = 0usize;
        let mut rewritten = Vec::with_capacity(entries.len());
        for mut entry in entries {
            if entry.status == LessonStatus::Active && entry.id.ends_with(&suffix) {
                entry.status = LessonStatus::Retired;
                retired_count += 1;
            }
            rewritten.push(entry);
        }
        if retired_count > 0 {
            self.rewrite_all(&rewritten)?;
        }
        Ok(retired_count)
    }

    pub fn retire(&self, lesson_id: &str) -> Result<bool, HarnessError> {
        let entries = self.load_all()?;
        let mut found = false;
        let mut rewritten = Vec::with_capacity(entries.len());
        for mut entry in entries {
            if entry.id == lesson_id {
                entry.status = LessonStatus::Retired;
                found = true;
            }
            rewritten.push(entry);
        }
        if found {
            self.rewrite_all(&rewritten)?;
        }
        Ok(found)
    }

    /// 验证阶段（#2080）：对全部 Pending 经验按本周期证据判定，落盘结果。
    /// 返回判定明细（空 = 无可验证条目或样本不足）。
    pub fn verify_pending(
        &self,
        current_signals: &[String],
        has_new_evidence: bool,
        now: u64,
    ) -> Result<Vec<VerificationOutcome>, HarnessError> {
        let entries = self.load_all()?;
        let mut outcomes = Vec::new();
        let mut rewritten = Vec::with_capacity(entries.len());
        for mut entry in entries {
            if let Some(outcome) = judge_verification(&entry, current_signals, has_new_evidence) {
                entry.verification.status = outcome.status;
                entry.verification.checked_at = now;
                outcomes.push(outcome);
            }
            rewritten.push(entry);
        }
        if !outcomes.is_empty() {
            self.rewrite_all(&rewritten)?;
        }
        Ok(outcomes)
    }

    fn rewrite_all(&self, entries: &[LessonEntry]) -> Result<(), HarnessError> {
        let mut text = String::new();
        for entry in entries {
            let line = serde_json::to_string(entry).map_err(|error| {
                HarnessError::invalid(ERROR_DOMAIN, format!("经验条目序列化失败：{error}"))
            })?;
            text.push_str(&line);
            text.push('\n');
        }
        atomic_write(&self.path, &text)
    }
}

/// 原子写（tmp+rename，对齐 registry 写约定）。
fn atomic_write(path: &Path, text: &str) -> Result<(), HarnessError> {
    let temp_path = path.with_extension("tmp");
    std::fs::write(&temp_path, text).map_err(|error| {
        HarnessError::storage(ERROR_DOMAIN, format!("写临时经验库失败：{error}"))
    })?;
    std::fs::rename(&temp_path, path).map_err(|error| {
        HarnessError::storage(ERROR_DOMAIN, format!("原子替换经验库失败：{error}"))
    })?;
    Ok(())
}
