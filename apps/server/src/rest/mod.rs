use crate::model::app::AppState;

pub mod board;
pub mod chat;
pub mod health;

pub fn router(state: AppState) -> axum::Router {
    health::router().merge(chat::router(state.clone())).merge(board::router(state))
}
