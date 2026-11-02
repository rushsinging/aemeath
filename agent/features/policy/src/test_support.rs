//! Policy 测试实现面（#2146 实现权治理：Policy 实现权归本 crate，
//! 跨 crate 测试经此获得替身；生产不编译）。

use std::sync::Arc;

use crate::domain::{Policy, PolicyDecisionData, PolicyModeData, PolicyRequestData};

/// 脚本化 Policy：evaluate 按脚本，mode 固定。
pub fn scripted_policy(
    evaluate: impl Fn(&PolicyRequestData) -> PolicyDecisionData + Send + Sync + 'static,
    mode: PolicyModeData,
) -> Arc<dyn Policy> {
    Arc::new(ScriptedPolicy {
        evaluate: Box::new(evaluate),
        mode,
    })
}

struct ScriptedPolicy {
    evaluate: Box<dyn Fn(&PolicyRequestData) -> PolicyDecisionData + Send + Sync>,
    mode: PolicyModeData,
}

impl Policy for ScriptedPolicy {
    fn evaluate(&self, request: &PolicyRequestData) -> PolicyDecisionData {
        (self.evaluate)(request)
    }
    fn current_mode(&self) -> PolicyModeData {
        self.mode
    }
}

/// 恒 Allow(STANDARD) + Standard 模式。
pub fn allow_all_standard() -> Arc<dyn Policy> {
    scripted_policy(
        |_| PolicyDecisionData::Allow(share::tools_vocab::AuthorizationContext::STANDARD),
        PolicyModeData::Standard,
    )
}

// #2146 密封解锁。
impl crate::sealed::Sealed for ScriptedPolicy {}
