//! Unified driver loop for action server event handling.
//!
//! This module provides a single event loop that handles all server-side
//! goal admission, execution and cancellation
//! in a sequential, race-condition-free manner.

use std::{
    future::Future,
    marker::PhantomData,
    sync::{Arc, Weak},
    time::Duration,
};

use crate::compat::JoinSet;
use crate::compat::CancellationToken;
use zenoh::Wait;

use super::{
    GoalInfo, ZAction,
    messages::*,
    server::{Executing, GoalHandle, InnerServer, Requested, ZActionServer},
    state::ServerGoalState,
};
use crate::msg::ZMessage;

// Retire unfinished goals on every exit, including native handler panics and
// task cancellation. Identity prevents a late drop from affecting a reused UUID.
struct ManagedGoal<A: ZAction> {
    server: ZActionServer<A>,
    id: super::GoalId,
    instance: Arc<()>,
}

impl<A: ZAction> Drop for ManagedGoal<A> {
    fn drop(&mut self) {
        self.server.abort_unfinished(self.id, &self.instance);
    }
}

/// Runs the unified driver loop for an action server with automatic goal handling.
///
/// This function consolidates all protocol logic into a single event loop,
/// eliminating race conditions and reducing task overhead.
///
/// # Arguments
///
/// * `weak_inner` - Weak reference to the inner server state
/// * `shutdown` - Cancellation token to stop the driver loop
/// * `handler` - Callback to execute goals automatically
pub(crate) async fn run_driver_loop<A, F, Fut>(
    weak_inner: Weak<InnerServer<A>>,
    shutdown: CancellationToken,
    handler: F,
) where
    A: ZAction,
    F: Fn(GoalHandle<A, Executing>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tracing::debug!("Action Server Driver Loop Started");

    // Try to upgrade the weak reference once at the start
    let Some(inner) = weak_inner.upgrade() else {
        tracing::debug!("Server already dropped, not starting driver loop");
        return;
    };

    let handler = Arc::new(handler);

    // Create a timer for periodic expiration checking (every 1 second)
    let mut expiration_timer = Box::pin(crate::compat::sleep(Duration::from_secs(1)));

    // STRUCTURED CONCURRENCY: Track all spawned goal tasks here
    let mut goal_tasks = JoinSet::new();

    loop {
        tokio::select! {
            // 1. Priority: Shutdown
            _ = shutdown.cancelled() => {
                tracing::debug!("Shutdown signal received. Aborting all goal tasks.");
                // This sends a cancellation signal to all running futures in the set
                goal_tasks.abort_all();
                break;
            }

            // 2. Reap Finished Tasks (Zombie Prevention)
            // This line is crucial. It removes finished tasks from memory.
            Some(res) = goal_tasks.join_next() => {
                if let Err(e) = res {
                    tracing::debug!("Action task ended: {e}");
                }
            }

            // 3. Goal Expiration Timer
            _ = &mut expiration_timer => {
                expiration_timer = Box::pin(crate::compat::sleep(Duration::from_secs(1)));
                // Check for expired goals and clean them up
                let server = ZActionServer::from_inner(Arc::clone(&inner));
                let expired_goals = server.expire_goals();
                if !expired_goals.is_empty() {
                    tracing::debug!("Expired {} goals: {:?}", expired_goals.len(), expired_goals);
                }
            }

            // 4. New Goal Requests
            query = inner.goal_server.queue().recv_async() => {
                let inner = inner.clone();
                let handler = handler.clone();

                // Spawn into the SET, not globally detached
                goal_tasks.spawn(async move {
                    // This is now safe. If it hangs, abort_all() kills it.
                    handle_goal_request(inner, query, handler).await;
                });
            }

            // 5. Cancel Requests
            query = inner.cancel_server.queue().recv_async() => {
                handle_cancel_request(&inner, query);
            }


        }
    }

    // Ensure everything is dead before we exit
    while goal_tasks.join_next().await.is_some() {}
    tracing::debug!("Action Server Driver Loop Stopped");
}

/// Handles incoming goal requests.
async fn handle_goal_request<A, F, Fut>(
    inner: Arc<InnerServer<A>>,
    query: zenoh::query::Query,
    handler: Arc<F>,
) where
    A: ZAction,
    F: Fn(GoalHandle<A, Executing>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tracing::debug!("Received goal request");
    if super::request_attachment(&query).is_err() { return; }
    let Some(payload) = query.payload() else { return };
    let payload = payload.to_bytes();
    let request = match <GoalRequest<A> as ZMessage>::deserialize(&payload) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to deserialize goal request: {}", e);
            return;
        }
    };

    // Create a temporary ZActionServer handle for the goal handle
    // This is safe because we're just passing it to the goal handler
    let server = ZActionServer::from_inner(Arc::clone(&inner));

    let requested = GoalHandle {
        goal: request.goal,
        info: GoalInfo::new(request.goal_id),
        server,
        query: Some(query),
        cancel_flag: None,
        instance: None,
        _state: PhantomData::<Requested>,
    };

    if inner.goal_manager.read(|manager| manager.goals.contains_key(&requested.info.goal_id)) {
        let _ = requested.reject();
        return;
    }
    let goal_id = requested.info.goal_id;
    let Ok(accepted) = requested.try_accept() else { return };
    let instance = accepted.instance.as_ref().expect("accepted goal has an identity").clone();
    let _completion = ManagedGoal {
        server: ZActionServer::from_inner(inner.clone()),
        id: goal_id,
        instance,
    };
    let executing = accepted.execute();

    // Execute the user's handler
    // No tokio::select! needed anymore. If the driver loop aborts this task,
    // this await simply acts as a cancellation point.
    let duration = inner.goal_manager.read(|manager| manager.goal_timeout);
    if let Some(duration) = duration {
        let _ = crate::compat::timeout(duration, handler(executing)).await;
    } else {
        handler(executing).await;
    }
}

/// Handles incoming cancel requests.
pub(crate) fn handle_cancel_request<A: ZAction>(
    inner: &Arc<InnerServer<A>>, query: zenoh::query::Query,
) {
    let Ok(attachment) = super::request_attachment(&query) else { return };
    let Some(payload) = query.payload() else { return };
    let Ok(request) = <CancelGoalServiceRequest as ZMessage>::deserialize(&payload.to_bytes()) else { return };
    let response = inner.goal_manager.modify(|manager| {
        let infos = inner.goal_info.lock();
        let stamp = (request.goal_info.stamp.sec, request.goal_info.stamp.nanosec);
        let by_id = request.goal_info.goal_id.is_valid();
        let by_time = stamp != (0, 0);
        let mut goals_canceling = Vec::new();
        for (id, state) in &manager.goals {
            let Some(info) = infos.get(id) else { continue };
            if (!by_id && !by_time) || (by_id && *id == request.goal_info.goal_id)
                || (by_time && (info.stamp.sec, info.stamp.nanosec) <= stamp) {
                match state {
                    ServerGoalState::Executing { cancel_flag, .. } => {
                        cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                        goals_canceling.push(info.clone());
                    },
                    ServerGoalState::Accepted { .. } => {
                        inner.cancel_pending.lock().insert(*id);
                        goals_canceling.push(info.clone());
                    },
                    ServerGoalState::Canceling { .. } => goals_canceling.push(info.clone()),
                    ServerGoalState::Terminated { .. } => {},
                }
            }
        }
        let return_code = if !goals_canceling.is_empty() || !by_id { 0 }
            else if manager.goals.contains_key(&request.goal_info.goal_id) { 3 } else { 2 };
        CancelGoalServiceResponse { return_code, goals_canceling }
    });
    ZActionServer::from_inner(inner.clone()).publish_status();
    let bytes = <CancelGoalServiceResponse as ZMessage>::serialize(&response);
    let reply = query.reply(query.key_expr().clone(), bytes);
    let _ = reply.attachment(attachment).wait();
}

/// Handles incoming result requests.
pub(crate) async fn handle_result_request<A: ZAction>(
    inner: &Arc<InnerServer<A>>,
    query: zenoh::query::Query,
) {
    tracing::debug!("Received result request");
    let Ok(attachment) = super::request_attachment(&query) else { return };
    let Some(payload) = query.payload() else { return };
    let payload = payload.to_bytes();
    let request = match <GetResultRequest as ZMessage>::deserialize(&payload) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to deserialize result request: {}", e);
            return;
        }
    };

    // Check if goal is already terminated, or register a waiter
    let (tx, rx) = tokio::sync::oneshot::channel();
    enum ResultState {
        Terminated,
        Waiting,
        NotFound,
    }

    let (result_state, result_data) = inner.goal_manager.modify(|manager| {
        if let Some(ServerGoalState::Terminated { result, status, .. }) =
            manager.goals.get(&request.goal_id)
        {
            // Goal is already terminated - return result immediately
            (ResultState::Terminated, Some((result.clone(), *status)))
        } else if manager.goals.contains_key(&request.goal_id) {
            // Goal exists but not terminated yet - register waiter
            manager
                .result_futures
                .entry(request.goal_id)
                .or_default()
                .push(tx);
            (ResultState::Waiting, None)
        } else {
            // Goal doesn't exist
            (ResultState::NotFound, None)
        }
    }); // Lock released here

    let (result, status) = match result_state {
        ResultState::Terminated => {
            let (r, s) = result_data.unwrap();
            tracing::debug!(
                "Goal {:?} is already terminated with status {:?}",
                request.goal_id,
                s
            );
            (r, s)
        }
        ResultState::Waiting => {
            // Wait for goal to complete
            tracing::debug!(
                "Goal {:?} not terminated yet, waiting for result...",
                request.goal_id
            );
            match rx.await {
                Ok((r, s)) => {
                    tracing::debug!("Goal {:?} completed with status {:?}", request.goal_id, s);
                    (r, s)
                }
                Err(_) => {
                    tracing::warn!("Result future cancelled for goal {:?}", request.goal_id);
                    if let Some(result) = A::default_result() {
                        (result, super::GoalStatus::Unknown)
                    } else {
                        let _ = query.reply_err("Goal result expired or server stopped").wait();
                        return;
                    }
                }
            }
        }
        ResultState::NotFound => {
            tracing::warn!("Goal {:?} not found", request.goal_id);
            if let Some(result) = A::default_result() {
                (result, super::GoalStatus::Unknown)
            } else {
                let _ = query.reply_err("Unknown goal").wait();
                return;
            }
        }
    };

    // Send result response
    let response = GetResultResponse::<A> {
        status: status as i8,
        result,
    };
    let response_bytes = <GetResultResponse<A> as ZMessage>::serialize(&response);
    let _ = query
        .reply(query.key_expr().clone(), response_bytes)
        .attachment(attachment)
        .wait();
    tracing::debug!("Sent result response");
}
