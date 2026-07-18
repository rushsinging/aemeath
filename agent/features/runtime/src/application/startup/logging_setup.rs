use logging;

/// 设置全局 session ID（只能调用一次）。委托 `logging::set_session_id`。
pub fn set_session_id(id: String) {
    logging::set_session_id(id);
}

/// 设置当前 turn。委托 `logging::set_current_turn`。
pub fn set_current_turn(turn: usize) {
    logging::set_current_turn(turn);
}
