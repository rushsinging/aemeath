#[derive(Debug, Clone)]
pub struct DeniedCall {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub reason: String,
}
