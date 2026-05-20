pub mod health;

pub fn router() -> axum::Router {
    health::router()
}
