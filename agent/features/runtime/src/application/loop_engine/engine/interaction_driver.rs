use super::*;

pub(super) fn model_step_text(step: &ModelStep) -> String {
    match step {
        ModelStep::Complete { text } | ModelStep::Tools { text, .. } => text.clone(),
        #[cfg(test)]
        ModelStep::Continue { text } => text.clone(),
    }
}

pub(super) fn model_invocation(step: &ModelStep) -> ModelInvocation {
    let response = match step {
        ModelStep::Complete { text } | ModelStep::Tools { text, .. } => text.clone(),
        #[cfg(test)]
        ModelStep::Continue { text } => text.clone(),
    };
    ModelInvocation::new(response)
}

/// #1248 Task 5: Resolve tool suspensions through the interaction coordinator.
/// Creates `UserQuestions` intents, registers them via coordinator, publishes
/// to UI, stores the receiver on the port, and leaves the Run in AwaitingUser.
/// The actual reply/cancel is handled on the next drain cycle via
/// `poll_interaction`.
pub(super) async fn handle_suspensions(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    suspended: Vec<SuspendedToolCall>,
) -> Result<(), LoopEngineError> {
    use crate::application::interaction::coordinator::InteractionCoordinator;

    // #1248 Task 5: Start ONLY the first suspension. Queue the rest on the port
    // so they are handled one-at-a-time via finish_interaction_work.
    let mut iter = suspended.into_iter();
    let first = match iter.next() {
        Some(first) => first,
        None => return Ok(()),
    };
    let remaining: Vec<_> = iter.collect();

    // Build the current item for the first interaction — stores the full
    // SuspendedToolCall so finish_interaction_work can use the original
    // call's provider_id/name.
    let first_request_id = sdk::InteractionRequestId::new_v7();
    let first_continuation = InteractionContinuation::CompleteToolCall(first.call.id.clone());
    let current_item = PendingInteractionItem {
        request_id: first_request_id.clone(),
        continuation: first_continuation.clone(),
        suspended_call: Some(first.clone()),
        approval_call: None,
    };

    if !remaining.is_empty() {
        // Queue remaining on the port — the engine will start the next one
        // after the first resolves via finish_interaction_work.
        let queue: Vec<PendingInteractionItem> = remaining
            .into_iter()
            .map(|sc| {
                let request_id = sdk::InteractionRequestId::new_v7();
                PendingInteractionItem {
                    request_id,
                    continuation: InteractionContinuation::CompleteToolCall(sc.call.id.clone()),
                    suspended_call: Some(sc),
                    approval_call: None,
                }
            })
            .collect();
        port.set_pending_interaction_work(
            execution,
            PendingInteractionWork {
                current: Some(current_item.clone()),
                queue,
            },
        );
    } else {
        // No remaining items — still store the current item so
        // finish_interaction_work can look up the original call.
        port.set_pending_interaction_work(
            execution,
            PendingInteractionWork {
                current: Some(current_item.clone()),
                queue: Vec::new(),
            },
        );
    }

    // Start the first interaction
    {
        let questions: Vec<sdk::UserQuestion> = first
            .questions
            .iter()
            .map(|q| sdk::UserQuestion {
                prompt: q.prompt.clone(),
                options: q.options.clone(),
                allow_multi: q.allow_multi,
            })
            .collect();
        let body = sdk::InteractionRequestBody::UserQuestions(questions);
        let run_id = run.id().clone();

        let (rid, receiver) = InteractionCoordinator::begin(
            run,
            port.interaction_port(),
            first_request_id.clone(),
            run_id.clone(),
            body.clone(),
            first_continuation.clone(),
        )
        .map_err(|e| LoopEngineError::Adapter(format!("interaction begin failed: {e:?}")))?;

        // Publish to UI
        let request = sdk::InteractionRequest {
            id: first_request_id.clone(),
            run_id: run_id.clone(),
            tool_call_id: Some(first.call.id.to_string()),
            body: body.clone(),
        };
        let _interaction_activity_id = port.start_interaction_activity(
            run.active_step_id()
                .ok_or_else(|| LoopEngineError::Adapter("no active step".to_string()))?,
            first_request_id.clone(),
            sdk::InteractionKindView::UserQuestion,
        )?;
        port.publish_interaction(execution, &request).await?;

        let metadata = crate::application::interaction::port::InteractionRequestMetadata::new(
            first_request_id,
            run_id,
            body,
            first_continuation.clone(),
        );
        crate::application::interaction::coordinator::InteractionCoordinator::store_mailbox_receiver(
            execution, metadata, receiver,
        )
        .map_err(|error| {
            LoopEngineError::Adapter(format!(
                "interaction mailbox registration failed: {error:?}"
            ))
        })?;

        log::debug!(
            target: crate::LOG_TARGET,
            "[handle_suspensions] registered first interaction rid={:?} call={}",
            rid,
            first.call.id,
        );
    }

    // #1248: Do NOT advance tool calls to Success here — the calls are NOT
    // resolved yet.  Do NOT complete_step — the step stays active until all
    // interactions resolve via finish_interaction_work → advance_tool_call.
    // Only emit events so the UI knows the run is awaiting user input.
    emit_events(run, execution, port).await?;
    Ok(())
}

/// #1248 Task 5: Resolve tool approvals through the interaction coordinator.
/// Creates `ToolApproval` intents — only starts the FIRST, queues remaining.
pub(super) async fn handle_tool_approvals(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    calls_needing_approval: Vec<ApprovalRequiredCall>,
) -> Result<(), LoopEngineError> {
    use crate::application::interaction::coordinator::InteractionCoordinator;

    // Only start the FIRST approval; queue the rest.
    let mut iter = calls_needing_approval.into_iter();
    let first = match iter.next() {
        Some(first) => first,
        None => return Ok(()),
    };
    let remaining: Vec<_> = iter.collect();

    // Build the current item for the first approval — stores the full
    // ApprovalRequiredCall so finish_interaction_work can execute directly.
    let first_request_id = sdk::InteractionRequestId::new_v7();
    let first_continuation = InteractionContinuation::ContinueToolApproval(first.call.id.clone());
    let current_item = PendingInteractionItem {
        request_id: first_request_id.clone(),
        continuation: first_continuation.clone(),
        suspended_call: None,
        approval_call: Some(first.clone()),
    };

    if !remaining.is_empty() {
        let queue: Vec<PendingInteractionItem> = remaining
            .into_iter()
            .map(|ac| {
                let request_id = sdk::InteractionRequestId::new_v7();
                PendingInteractionItem {
                    request_id,
                    continuation: InteractionContinuation::ContinueToolApproval(ac.call.id.clone()),
                    suspended_call: None,
                    approval_call: Some(ac),
                }
            })
            .collect();
        port.set_pending_interaction_work(
            execution,
            PendingInteractionWork {
                current: Some(current_item.clone()),
                queue,
            },
        );
    } else {
        port.set_pending_interaction_work(
            execution,
            PendingInteractionWork {
                current: Some(current_item.clone()),
                queue: Vec::new(),
            },
        );
    }

    // Start the first approval
    {
        let body = sdk::InteractionRequestBody::ToolApproval(sdk::ToolApprovalPrompt {
            tool_name: first.call.name.clone(),
            args_summary: first.reason.clone(),
            risk_level: sdk::RiskLevel::Medium,
        });
        let run_id = run.id().clone();

        let (_rid, receiver) = InteractionCoordinator::begin(
            run,
            port.interaction_port(),
            first_request_id.clone(),
            run_id.clone(),
            body.clone(),
            first_continuation.clone(),
        )
        .map_err(|e| LoopEngineError::Adapter(format!("tool approval begin failed: {e:?}")))?;

        let request = sdk::InteractionRequest {
            id: first_request_id.clone(),
            run_id: run_id.clone(),
            tool_call_id: None,
            body: body.clone(),
        };
        let _interaction_activity_id = port.start_interaction_activity(
            run.active_step_id()
                .ok_or_else(|| LoopEngineError::Adapter("no active step".to_string()))?,
            first_request_id.clone(),
            sdk::InteractionKindView::ToolApproval,
        )?;
        port.publish_interaction(execution, &request).await?;
        let metadata = crate::application::interaction::port::InteractionRequestMetadata::new(
            first_request_id.clone(),
            run_id.clone(),
            body.clone(),
            first_continuation.clone(),
        );
        crate::application::interaction::coordinator::InteractionCoordinator::store_mailbox_receiver(
            execution, metadata, receiver,
        )
        .map_err(|error| {
            LoopEngineError::Adapter(format!(
                "interaction mailbox registration failed: {error:?}"
            ))
        })?;
    }

    emit_events(run, execution, port).await?;
    Ok(())
}

/// #1248: Handle a resolved or closed interaction via the coordinator.
/// After validation, calls `finish_interaction_work` to write results,
/// advances tool calls, and either starts the next queued interaction
/// or completes the step.
pub(super) async fn handle_interaction_completion(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    step_cancel: &CancellationToken,
    resolution: crate::application::interaction::port::InteractionResolution,
) -> Result<(), LoopEngineError> {
    use crate::application::interaction::coordinator::InteractionCoordinator;
    use crate::application::interaction::port::InteractionCompletion;
    use crate::application::interaction::port::InteractionResolution;

    let metadata = resolution.metadata().clone();
    let request_id = metadata.request_id.clone();
    let run_id = metadata.run_id.clone();
    let interaction_terminal = match &resolution {
        InteractionResolution::Resolved {
            completion: InteractionCompletion::Replied(_),
            ..
        } => ActivityTerminal::Succeeded,
        InteractionResolution::Resolved {
            completion: InteractionCompletion::Cancelled(_),
            ..
        }
        | InteractionResolution::Closed { .. } => ActivityTerminal::Cancelled,
    };
    port.finish_interaction_activity(&request_id, interaction_terminal)?;

    match &resolution {
        InteractionResolution::Resolved {
            completion: InteractionCompletion::Replied(reply),
            ..
        } => {
            log::debug!(
                target: crate::LOG_TARGET,
                "[handle_interaction_completion] replied request={request_id}",
            );
            // Validate and complete via coordinator
            let continuation =
                InteractionCoordinator::complete_reply(run, &request_id, &metadata.body, reply)
                    .map_err(|e| {
                        log::error!(target: crate::LOG_TARGET, "complete_reply failed: {e:?}");
                        LoopEngineError::Adapter(format!("interaction completion failed: {e:?}"))
                    })?;

            // Dispatch based on the continuation type
            dispatch_continuation(
                run,
                execution,
                port,
                step_cancel,
                &metadata,
                &continuation,
                reply,
            )
            .await?;
        }
        InteractionResolution::Resolved {
            completion: InteractionCompletion::Cancelled(reason),
            ..
        } => {
            log::debug!(
                target: crate::LOG_TARGET,
                "[handle_interaction_completion] cancelled request={request_id} reason={reason:?}",
            );
            let _ = InteractionCoordinator::cancel(run, &request_id);

            let outcome = {
                let context = port.interaction_completion_context(step_cancel.clone());
                InteractionCoordinator::complete_tool_interaction(
                    &context,
                    execution,
                    &metadata,
                    &InteractionCompletion::Cancelled(reason.clone()),
                )
                .await?
            };
            handle_interaction_outcome(run, execution, port, step_cancel, outcome).await?;
        }
        InteractionResolution::Closed { .. } => {
            log::warn!(
                target: crate::LOG_TARGET,
                "[handle_interaction_completion] closed request={request_id}",
            );
            InteractionCoordinator::cleanup_run(
                run,
                execution,
                port.interaction_port(),
                &run_id,
                sdk::InteractionCancelReason::RunCancelled,
            );
            let outcome = {
                let context = port.interaction_completion_context(step_cancel.clone());
                InteractionCoordinator::complete_tool_interaction(
                    &context,
                    execution,
                    &metadata,
                    &InteractionCompletion::Cancelled(sdk::InteractionCancelReason::RunCancelled),
                )
                .await?
            };
            handle_interaction_outcome(run, execution, port, step_cancel, outcome).await?;
        }
    }

    emit_events(run, execution, port).await?;
    Ok(())
}

/// #1248: Dispatch after a completed interaction based on the continuation type.
pub(super) async fn dispatch_continuation(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    step_cancel: &CancellationToken,
    metadata: &crate::application::interaction::port::InteractionRequestMetadata,
    continuation: &InteractionContinuation,
    reply: &sdk::InteractionReply,
) -> Result<(), LoopEngineError> {
    use crate::application::interaction::coordinator::InteractionCoordinator;
    use crate::application::interaction::port::InteractionCompletion;

    match continuation {
        InteractionContinuation::CompleteToolCall(tool_call_id) => {
            let outcome = {
                let context = port.interaction_completion_context(step_cancel.clone());
                InteractionCoordinator::complete_tool_interaction(
                    &context,
                    execution,
                    metadata,
                    &InteractionCompletion::Replied(reply.clone()),
                )
                .await?
            };
            handle_interaction_outcome(run, execution, port, step_cancel, outcome).await?;
            // Suppress unused warning
            let _ = tool_call_id;
        }
        InteractionContinuation::ContinueAfterHardPause => {
            // HardPauseContinue: the continuation already transitioned
            // the run back to ExecutingTools via complete_interaction.
            log::debug!(
                target: crate::LOG_TARGET,
                "[dispatch_continuation] HardPauseContinue — resuming"
            );
        }
        InteractionContinuation::ContinuePlanApproval => {
            if matches!(
                reply,
                sdk::InteractionReply::PlanApproval(sdk::ApprovalDecision::Deny { .. })
            ) {
                InteractionCoordinator::cleanup_run(
                    run,
                    execution,
                    port.interaction_port(),
                    &metadata.run_id,
                    sdk::InteractionCancelReason::UserCancelled,
                );
                emit_events(run, execution, port).await?;
            }
            // On approve: complete_interaction already transitioned back;
            // the step was already completed in handle_plan_approval.
        }
        InteractionContinuation::ContinueToolApproval(tool_call_id) => {
            if matches!(
                reply,
                sdk::InteractionReply::ToolApproval(sdk::ApprovalDecision::Approve)
            ) {
                log::debug!(
                    target: crate::LOG_TARGET,
                    "[dispatch_continuation] ToolApproval approve for call={tool_call_id}"
                );
                let outcome = {
                    let context = port.interaction_completion_context(step_cancel.clone());
                    InteractionCoordinator::complete_tool_interaction(
                        &context,
                        execution,
                        metadata,
                        &InteractionCompletion::Replied(reply.clone()),
                    )
                    .await?
                };
                handle_interaction_outcome(run, execution, port, step_cancel, outcome).await?;
            } else {
                let outcome = {
                    let context = port.interaction_completion_context(step_cancel.clone());
                    InteractionCoordinator::complete_tool_interaction(
                        &context,
                        execution,
                        metadata,
                        &InteractionCompletion::Cancelled(
                            sdk::InteractionCancelReason::UserCancelled,
                        ),
                    )
                    .await?
                };
                handle_interaction_outcome(run, execution, port, step_cancel, outcome).await?;
            }
        }
    }

    Ok(())
}

/// #1248: Process the outcome of `finish_interaction_work`.
/// Advances the resolved tool call, then either starts the next queued
/// interaction or completes the step.
pub(super) async fn handle_interaction_outcome(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    _step_cancel: &CancellationToken,
    outcome: InteractionWorkOutcome,
) -> Result<(), LoopEngineError> {
    use crate::application::interaction::coordinator::InteractionCoordinator;

    match outcome {
        InteractionWorkOutcome::Completed {
            call_id,
            status,
            remaining_queue,
            schedule_tool_results,
        } => {
            if schedule_tool_results {
                port.schedule_internal_continuation(InternalContinuationKind::ToolResults);
            }
            // Advance the just-resolved tool call — find the step
            let step_id = run
                .active_step_id()
                .ok_or_else(|| LoopEngineError::Adapter("no active step".to_string()))?;
            run.advance_tool_call(&step_id, &call_id, status)?;
            let terminal = match status {
                ToolCallStatus::Success => ActivityTerminal::Succeeded,
                ToolCallStatus::Error => ActivityTerminal::Failed,
                ToolCallStatus::Cancelled => ActivityTerminal::Cancelled,
                ToolCallStatus::Pending | ToolCallStatus::Ready | ToolCallStatus::Running => {
                    return Err(LoopEngineError::Adapter(format!(
                        "interaction tool returned non-terminal status: {status:?}"
                    )));
                }
            };
            port.finish_tool_activity_by_source(&call_id, terminal)?;

            if remaining_queue.is_empty() {
                // All interactions resolved — complete the step
                run.complete_step(&step_id)?;
                run_step_finalization_phase(
                    execution,
                    port.persistence_mut(),
                    &step_id,
                    crate::ports::FinalizeCause::Completed,
                )
                .await?;
                transition_and_emit(run, execution, port, RunTransition::ToolsCompleted).await?;
            } else {
                // Start the next interaction from the queue
                let next = remaining_queue[0].clone();
                log::debug!(
                    target: crate::LOG_TARGET,
                    "[handle_interaction_outcome] starting next interaction rid={:?}",
                    next.request_id,
                );

                if let Some(ref suspended) = next.suspended_call {
                    let questions: Vec<sdk::UserQuestion> = suspended
                        .questions
                        .iter()
                        .map(|q| sdk::UserQuestion {
                            prompt: q.prompt.clone(),
                            options: q.options.clone(),
                            allow_multi: q.allow_multi,
                        })
                        .collect();
                    let body = sdk::InteractionRequestBody::UserQuestions(questions);
                    let run_id = run.id().clone();

                    let (rid, receiver) = InteractionCoordinator::begin(
                        run,
                        port.interaction_port(),
                        next.request_id.clone(),
                        run_id.clone(),
                        body.clone(),
                        next.continuation.clone(),
                    )
                    .map_err(|e| {
                        LoopEngineError::Adapter(format!("queued interaction begin failed: {e:?}"))
                    })?;

                    let request = sdk::InteractionRequest {
                        id: next.request_id.clone(),
                        run_id: run_id.clone(),
                        tool_call_id: Some(suspended.call.id.to_string()),
                        body: body.clone(),
                    };
                    let _interaction_activity_id = port.start_interaction_activity(
                        step_id.clone(),
                        next.request_id.clone(),
                        sdk::InteractionKindView::UserQuestion,
                    )?;
                    port.publish_interaction(execution, &request).await?;
                    let metadata =
                        crate::application::interaction::port::InteractionRequestMetadata::new(
                            next.request_id.clone(),
                            run_id,
                            body,
                            next.continuation.clone(),
                        );
                    crate::application::interaction::coordinator::InteractionCoordinator::store_mailbox_receiver(
            execution, metadata, receiver,
        )
        .map_err(|error| {
            LoopEngineError::Adapter(format!(
                "interaction mailbox registration failed: {error:?}"
            ))
        })?;
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "[handle_interaction_outcome] queued UserQuestions rid={:?}",
                        rid,
                    );
                } else if let Some(ref approval) = next.approval_call {
                    let body = sdk::InteractionRequestBody::ToolApproval(sdk::ToolApprovalPrompt {
                        tool_name: approval.call.name.clone(),
                        args_summary: approval.reason.clone(),
                        risk_level: sdk::RiskLevel::Medium,
                    });
                    let run_id = run.id().clone();

                    let (_rid, receiver) = InteractionCoordinator::begin(
                        run,
                        port.interaction_port(),
                        next.request_id.clone(),
                        run_id.clone(),
                        body.clone(),
                        next.continuation.clone(),
                    )
                    .map_err(|e| {
                        LoopEngineError::Adapter(format!(
                            "queued tool approval begin failed: {e:?}"
                        ))
                    })?;

                    let request = sdk::InteractionRequest {
                        id: next.request_id.clone(),
                        run_id: run_id.clone(),
                        tool_call_id: None,
                        body: body.clone(),
                    };
                    let _interaction_activity_id = port.start_interaction_activity(
                        step_id.clone(),
                        next.request_id.clone(),
                        sdk::InteractionKindView::ToolApproval,
                    )?;
                    port.publish_interaction(execution, &request).await?;
                    let metadata =
                        crate::application::interaction::port::InteractionRequestMetadata::new(
                            next.request_id.clone(),
                            run_id,
                            body,
                            next.continuation.clone(),
                        );
                    crate::application::interaction::coordinator::InteractionCoordinator::store_mailbox_receiver(
            execution, metadata, receiver,
        )
        .map_err(|error| {
            LoopEngineError::Adapter(format!(
                "interaction mailbox registration failed: {error:?}"
            ))
        })?;
                }

                // Update the port: new current = the now-started item,
                // new queue = items after it.
                let rest: Vec<PendingInteractionItem> = if remaining_queue.len() > 1 {
                    remaining_queue[1..].to_vec()
                } else {
                    Vec::new()
                };
                port.set_pending_interaction_work(
                    execution,
                    PendingInteractionWork {
                        current: Some(next),
                        queue: rest,
                    },
                );
            }
        }
    }

    Ok(())
}

/// #1248 Task 5: Handle a HardPause stuck decision via the interaction coordinator.
/// Instead of failing the run, creates a HardPause interaction request that the
/// user can continue from. On resume, the run transitions back to ExecutingTools.
pub(super) async fn handle_hard_pause(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    step_id: &sdk::RunStepId,
    reason: String,
) -> Result<(), LoopEngineError> {
    use crate::application::interaction::coordinator::InteractionCoordinator;

    let request_id = sdk::InteractionRequestId::new_v7();
    let body = sdk::InteractionRequestBody::HardPause(sdk::StuckDiagnostic {
        reason: reason.clone(),
        recent_actions: vec![],
    });
    let continuation = InteractionContinuation::ContinueAfterHardPause;
    let run_id = run.id().clone();

    let (_rid, receiver) = InteractionCoordinator::begin(
        run,
        port.interaction_port(),
        request_id.clone(),
        run_id.clone(),
        body.clone(),
        continuation.clone(),
    )
    .map_err(|e| {
        // If interaction is unavailable, fall back to failing the run.
        log::error!(target: crate::LOG_TARGET, "HardPause interaction begin failed: {e:?}");
        LoopEngineError::Adapter(format!("HardPause interaction unavailable: {e:?}"))
    })?;

    // Publish to UI and store receiver
    let request = sdk::InteractionRequest {
        id: request_id.clone(),
        run_id: run_id.clone(),
        tool_call_id: None,
        body: body.clone(),
    };
    let _interaction_activity_id = port.start_interaction_activity(
        step_id.clone(),
        request_id.clone(),
        sdk::InteractionKindView::StuckDiagnostic,
    )?;
    port.publish_interaction(execution, &request).await?;
    let metadata = crate::application::interaction::port::InteractionRequestMetadata::new(
        request_id.clone(),
        run_id.clone(),
        body.clone(),
        continuation.clone(),
    );
    crate::application::interaction::coordinator::InteractionCoordinator::store_mailbox_receiver(
        execution, metadata, receiver,
    )
    .map_err(|error| {
        LoopEngineError::Adapter(format!(
            "interaction mailbox registration failed: {error:?}"
        ))
    })?;

    // Complete step and transition to AwaitingUser
    run.complete_step(step_id)?;
    run_step_finalization_phase(
        execution,
        port.persistence_mut(),
        step_id,
        crate::ports::FinalizeCause::Completed,
    )
    .await?;
    emit_events(run, execution, port).await?;

    Ok(())
}

/// #1248 Task 5: Handle plan approval via the interaction coordinator.
/// When the model produces a Complete response in plan mode, the user must review
/// the plan before the run proceeds. On approve, the run continues; on reject,
/// the run is cancelled.
pub(super) async fn handle_plan_approval(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    step_id: &sdk::RunStepId,
    plan_text: &str,
) -> Result<(), LoopEngineError> {
    use crate::application::interaction::coordinator::InteractionCoordinator;

    let request_id = sdk::InteractionRequestId::new_v7();
    let body = sdk::InteractionRequestBody::PlanApproval(sdk::PlanApprovalPrompt {
        plan_title: String::new(),
        steps: vec![plan_text.to_string()],
    });
    let continuation = InteractionContinuation::ContinuePlanApproval;
    let run_id = run.id().clone();

    let (_rid, receiver) = InteractionCoordinator::begin(
        run,
        port.interaction_port(),
        request_id.clone(),
        run_id.clone(),
        body.clone(),
        continuation.clone(),
    )
    .map_err(|e| {
        log::error!(target: crate::LOG_TARGET, "PlanApproval interaction begin failed: {e:?}");
        LoopEngineError::Adapter(format!("PlanApproval interaction unavailable: {e:?}"))
    })?;

    let request = sdk::InteractionRequest {
        id: request_id.clone(),
        run_id: run_id.clone(),
        tool_call_id: None,
        body: body.clone(),
    };
    let _interaction_activity_id = port.start_interaction_activity(
        step_id.clone(),
        request_id.clone(),
        sdk::InteractionKindView::PlanApproval,
    )?;
    port.publish_interaction(execution, &request).await?;
    let metadata = crate::application::interaction::port::InteractionRequestMetadata::new(
        request_id.clone(),
        run_id.clone(),
        body.clone(),
        continuation.clone(),
    );
    crate::application::interaction::coordinator::InteractionCoordinator::store_mailbox_receiver(
        execution, metadata, receiver,
    )
    .map_err(|error| {
        LoopEngineError::Adapter(format!(
            "interaction mailbox registration failed: {error:?}"
        ))
    })?;

    run.complete_step(step_id)?;
    run_step_finalization_phase(
        execution,
        port.persistence_mut(),
        step_id,
        crate::ports::FinalizeCause::Completed,
    )
    .await?;
    emit_events(run, execution, port).await?;

    Ok(())
}
