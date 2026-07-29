use std::future::Future;
use std::pin::Pin;

pub type QueueFuture<'a> = Pin<Box<dyn Future<Output = Option<Vec<String>>> + Send + 'a>>;

pub trait QueueDrainPort: Clone + Send + Sync + 'static {
    fn drain_queued_input<'a>(&'a self) -> QueueFuture<'a>;
}
