mod handle;
mod input_port;
mod logging;

use crate::tui::adapter::event_mapping::sdk_event_to_tui_event;
use crate::tui::adapter::tui_runtime_event::TuiRuntimeEvent;
use std::sync::Arc;

pub(crate) use handle::{shutdown_and_save, ProcessingHandle, SpawnContext, SpawnContextRefs};
pub(crate) use input_port::TuiInputEventPort;
pub(crate) use logging::{log_sdk_event, log_tui_runtime_delivery};

pub(crate) fn spawn_processing(ctx: SpawnContext) -> ProcessingHandle {
    let join = composition::delivery_logging::spawn_instrumented(
        composition::delivery_logging::capture(),
        async move {
            let mut stream = match ctx
                .agent_client
                .chat(sdk::ChatRequest {
                    ingress: Arc::new(ctx.input_event_port.clone()),
                })
                .await
            {
                Ok(stream) => stream,
                Err(e) => {
                    let _ = ctx
                        .runtime_tx
                        .send(TuiRuntimeEvent::Error(e.to_string()))
                        .await;
                    let _ = ctx
                        .runtime_tx
                        .send(TuiRuntimeEvent::Done {
                            context: ctx.fallback_context.clone(),
                            duration_ms: None,
                        })
                        .await;
                    return;
                }
            };
            while let Some(event) = stream.recv().await {
                log_sdk_event(&event, "sdk->tui.recv");
                let runtime_events = sdk_event_to_tui_event(event).into_runtime_events();
                for runtime_event in runtime_events {
                    log_tui_runtime_delivery(&runtime_event, "forwarding");
                    if ctx.runtime_tx.send(runtime_event).await.is_err() {
                        crate::tui::log_warn!(
                            "event_delivery boundary=sdk_to_tui kind=runtime_event outcome=receiver_closed"
                        );
                        break;
                    }
                }
            }
        },
    );
    ProcessingHandle { join }
}

#[cfg(test)]
#[path = "processing_tests.rs"]
mod tests;
