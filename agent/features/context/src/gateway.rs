//! Context crate 的 gateway 层——跨 crate 函数 / trait re-export。

pub mod context_port {
    pub use crate::context_port::{CompactionUrgency, ContextPort, ContextPortError};
}

pub mod guidance {
    pub use crate::prompt::gateway::guidance::*;
}

pub mod skill {
    pub use crate::prompt::gateway::skill::*;
}
