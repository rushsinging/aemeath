//! 证据流端口（#2049）：装配层实现流模型（枚举/游标/窗口/步长/分块），
//! 周期任务只依赖该端口——domain 逻辑与 IO 分离。
//!
//! 指纹语义（v1 拍板，注释即契约）：块执行时 registry 现读（无缓存），
//! 不存在陈旧提案覆盖；指纹用于 plan 时检测「游标后外部改动」
//! （warn 可见）与 advance 记录块后基线。回退游标重处理的全语义
//! 随 #2046 验收评估。

use crate::domain::evidence::EvidenceBundle;
use crate::domain::stream::{EvidenceBlockPlan, StreamPlanParams};
use std::sync::Mutex;

/// 证据流端口：plan（规划块）→ block_evidence（块证据）→ advance（推游标）。
pub trait EvidenceStreamPort: Send + Sync {
    /// 规划本次周期的数据源块（含游标读取、窗口过滤、步长截断、分块；
    /// 组件树指纹与游标不符时 warn 可见，证据处理照常——session 行为
    /// 证据与组件面正交）。
    fn plan(&self, params: &StreamPlanParams) -> Vec<EvidenceBlockPlan>;

    /// 扫描单块 sessions 并聚合证据（扫描失败降级空证据 NEVER 阻断）。
    fn block_evidence(&self, block: &EvidenceBlockPlan) -> EvidenceBundle;

    /// 推进游标到块尾（只在块处理完成后调用——输出存在不等于完成；
    /// 记录块执行后的组件树指纹作下一块基线）。
    fn advance(&self, block: &EvidenceBlockPlan, component_tree_fingerprint: String);

    /// 当前 checkpoint 概要（dry-run 报告用：起始游标位置；None=首次）。
    fn cursor_summary(&self) -> Option<String> {
        None
    }
}

/// 组件树指纹：registry 全组件内容哈希（稳定序）。
pub fn component_tree_fingerprint(
    registry: &mut dyn crate::application::ComponentRegistry,
) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for id in registry.list() {
        id.kind.as_str().hash(&mut hasher);
        id.name.as_str().hash(&mut hasher);
        if let Ok(content) = registry.read(&id) {
            content.body.hash(&mut hasher);
        }
    }
    format!("{:016x}", hasher.finish())
}

/// 测试桩：固定块计划与证据。
#[derive(Default)]
pub struct FakeEvidenceStream {
    pub plans: Mutex<Vec<EvidenceBlockPlan>>,
    pub advanced: Mutex<Vec<String>>,
}

impl EvidenceStreamPort for FakeEvidenceStream {
    fn plan(&self, _params: &StreamPlanParams) -> Vec<EvidenceBlockPlan> {
        self.plans.lock().expect("plans lock").clone()
    }

    fn block_evidence(&self, _block: &EvidenceBlockPlan) -> EvidenceBundle {
        EvidenceBundle::default()
    }

    fn advance(&self, block: &EvidenceBlockPlan, _fingerprint: String) {
        self.advanced
            .lock()
            .expect("advanced lock")
            .extend(block.runs.iter().map(|run| run.run_id.clone()));
    }
}
