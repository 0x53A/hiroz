//! Action server implementation for ROS 2 actions.
//!
//! This module provides the server-side functionality for ROS 2 actions,
//! allowing nodes to accept goals from action clients, execute them,
//! provide feedback, and return results.

use crate::compat::Instant;

use std::{
    collections::HashMap,
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::compat::CancellationToken;
use zenoh::{Result, Wait};

use super::{
    GoalId, GoalInfo, GoalStatus, ZAction,
    messages::*,
    state::{SafeGoalManager, ServerGoalState},
};
use crate::{
    Builder, entity::TypeInfo, msg::ZMessage,
    topic_name::qualify_topic_name,
};

/// Private implementation holding the actual server state.
/// This is wrapped by the public `ZActionServer` handle.
pub(crate) struct InnerServer<A: ZAction> {
    pub(crate) goal_server: Arc<crate::service::ZServer<GoalService<A>>>,
    pub(crate) result_server: Arc<crate::service::ZServer<ResultService<A>>>,
    pub(crate) cancel_server: Arc<crate::service::ZServer<CancelService<A>>>,
    pub(crate) feedback_pub:
        Arc<crate::pubsub::ZPub<FeedbackMessage<A>, <FeedbackMessage<A> as ZMessage>::Serdes>>,
    pub(crate) status_pub:
        Arc<crate::pubsub::ZPub<StatusMessage, <StatusMessage as ZMessage>::Serdes>>,
    pub(crate) goal_manager: Arc<SafeGoalManager<A>>,
    /// Token to cancel the default result handler when switching to full driver mode
    pub(crate) result_handler_token: CancellationToken,
    pub(crate) driver_started: AtomicBool,
    pub(crate) goal_info: crate::compat::Mutex<HashMap<GoalId, GoalInfo>>,
    // Private identity distinguishes accepted instances when a UUID is reused.
    goal_instances: crate::compat::Mutex<HashMap<GoalId, Arc<()>>>,
    pub(crate) cancel_pending: crate::compat::Mutex<std::collections::HashSet<GoalId>>,
}

/// Drop guard that triggers shutdown when the last server handle is dropped.
pub(crate) struct ShutdownGuard {
    pub(crate) token: CancellationToken,
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        tracing::debug!("ZActionServer handle dropped, triggering shutdown");
        self.token.cancel();
    }
}

/// Builder for creating an action server.
///
/// The `ZActionServerBuilder` allows you to configure timeouts and QoS settings
/// for different action communication channels before building the server.
///
/// # Examples
///
/// ```no_run
/// # use hiroz::action::*;
/// # use std::time::Duration;
/// # use hiroz_msgs::action_tutorials_interfaces::action::Fibonacci;
/// # let node: hiroz::node::ZNode = todo!();
/// let server = node.create_action_server::<Fibonacci>("fibonacci")
///     .with_result_timeout(Duration::from_secs(30))
///     .build()?;
/// # Ok::<(), zenoh::Error>(())
/// ```
pub struct ZActionServerBuilder<'a, A: ZAction> {
    /// The name of the action.
    pub action_name: String,
    /// Reference to the node that will own this server.
    pub node: &'a crate::node::ZNode,
    /// Timeout for result requests.
    pub result_timeout: Duration,
    /// Optional timeout for goal execution.
    pub goal_timeout: Option<Duration>,
    /// QoS profile for the goal service.
    pub goal_service_qos: Option<crate::qos::QosProfile>,
    /// QoS profile for the result service.
    pub result_service_qos: Option<crate::qos::QosProfile>,
    /// QoS profile for the cancel service.
    pub cancel_service_qos: Option<crate::qos::QosProfile>,
    /// QoS profile for the feedback topic.
    pub feedback_topic_qos: Option<crate::qos::QosProfile>,
    /// QoS profile for the status topic.
    pub status_topic_qos: Option<crate::qos::QosProfile>,
    /// Override for goal (send_goal) type info; uses `A::send_goal_type_info()` if None.
    pub goal_type_info: Option<TypeInfo>,
    /// Override for result (get_result) type info; uses `A::get_result_type_info()` if None.
    pub result_type_info: Option<TypeInfo>,
    /// Override for feedback type info; uses `A::feedback_type_info()` if None.
    pub feedback_type_info: Option<TypeInfo>,
    pub _phantom: std::marker::PhantomData<A>,
}

impl<'a, A: ZAction> ZActionServerBuilder<'a, A> {
    pub fn with_result_timeout(mut self, timeout: Duration) -> Self {
        self.result_timeout = timeout;
        self
    }

    pub fn with_goal_timeout(mut self, timeout: Duration) -> Self {
        self.goal_timeout = Some(timeout);
        self
    }

    pub fn with_goal_service_qos(mut self, qos: crate::qos::QosProfile) -> Self {
        self.goal_service_qos = Some(qos);
        self
    }

    pub fn with_result_service_qos(mut self, qos: crate::qos::QosProfile) -> Self {
        self.result_service_qos = Some(qos);
        self
    }

    pub fn with_cancel_service_qos(mut self, qos: crate::qos::QosProfile) -> Self {
        self.cancel_service_qos = Some(qos);
        self
    }

    pub fn with_feedback_topic_qos(mut self, qos: crate::qos::QosProfile) -> Self {
        self.feedback_topic_qos = Some(qos);
        self
    }

    pub fn with_status_topic_qos(mut self, qos: crate::qos::QosProfile) -> Self {
        self.status_topic_qos = Some(qos);
        self
    }

    /// Override the goal type info used for graph registration.
    ///
    /// By default `A::send_goal_type_info()` is used. Set this to supply a
    /// runtime-determined type hash (e.g. from Python message classes).
    pub fn with_goal_type_info(mut self, info: TypeInfo) -> Self {
        self.goal_type_info = Some(info);
        self
    }

    /// Override the result type info used for graph registration.
    pub fn with_result_type_info(mut self, info: TypeInfo) -> Self {
        self.result_type_info = Some(info);
        self
    }

    /// Override the feedback type info used for graph registration.
    pub fn with_feedback_type_info(mut self, info: TypeInfo) -> Self {
        self.feedback_type_info = Some(info);
        self
    }
}

impl<'a, A: ZAction> ZActionServerBuilder<'a, A> {
    pub fn new(action_name: &str, node: &'a crate::node::ZNode) -> Self {
        Self {
            action_name: action_name.to_string(),
            node,
            result_timeout: Duration::from_secs(10),
            goal_timeout: None,
            goal_service_qos: None,
            result_service_qos: None,
            cancel_service_qos: None,
            feedback_topic_qos: None,
            status_topic_qos: None,
            goal_type_info: None,
            result_type_info: None,
            feedback_type_info: None,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<'a, A: ZAction> Builder for ZActionServerBuilder<'a, A> {
    type Output = ZActionServer<A>;

    fn build(self) -> Result<Self::Output> {
        // Apply remapping to action name
        let action_name = self.node.remap_rules.apply(&self.action_name);

        // Validate action name
        if action_name.is_empty() {
            return Err(zenoh::Error::from("Action name cannot be empty"));
        }

        // Qualify action name like a topic name
        let qualified_action_name = qualify_topic_name(
            &action_name,
            &self.node.entity.namespace,
            &self.node.entity.name,
        )?;

        tracing::debug!(
            "Action name: '{}', namespace: '{}', qualified: '{}'",
            action_name,
            self.node.entity.namespace,
            qualified_action_name
        );

        // ROS 2 action naming conventions
        let goal_service_name = format!("{}/_action/send_goal", qualified_action_name);
        let result_service_name = format!("{}/_action/get_result", qualified_action_name);
        let cancel_service_name = format!("{}/_action/cancel_goal", qualified_action_name);
        let feedback_topic_name = format!("{}/_action/feedback", qualified_action_name);
        let status_topic_name = format!("{}/_action/status", qualified_action_name);

        // Create goal server using node API for proper graph registration
        // Use override if provided, otherwise fall back to the action's static type info.
        let goal_type_info = Some(self.goal_type_info.unwrap_or_else(A::send_goal_type_info));
        let mut goal_server_builder = self
            .node
            .create_service_impl::<GoalService<A>>(&goal_service_name, goal_type_info);
        if let Some(qos) = self.goal_service_qos {
            goal_server_builder.entity.qos = qos.to_protocol_qos();
        }
        let goal_server = goal_server_builder.build()?;

        // Create result server using node API for proper graph registration
        let result_type_info = Some(
            self.result_type_info
                .unwrap_or_else(A::get_result_type_info),
        );
        let mut result_server_builder = self
            .node
            .create_service_impl::<ResultService<A>>(&result_service_name, result_type_info);
        if let Some(qos) = self.result_service_qos {
            result_server_builder.entity.qos = qos.to_protocol_qos();
        }
        let result_server = result_server_builder.build()?;
        tracing::debug!("Created result server for: {}", result_service_name);

        // Create cancel server using node API for proper graph registration
        // Use the action's cancel_goal_type_info for proper ROS 2 interop
        let cancel_type_info = Some(A::cancel_goal_type_info());
        let mut cancel_server_builder = self
            .node
            .create_service_impl::<CancelService<A>>(&cancel_service_name, cancel_type_info);
        if let Some(qos) = self.cancel_service_qos {
            cancel_server_builder.entity.qos = qos.to_protocol_qos();
        }
        let cancel_server = cancel_server_builder.build()?;

        // Create feedback publisher using node API for proper graph registration
        let feedback_type_info = Some(
            self.feedback_type_info
                .unwrap_or_else(A::feedback_type_info),
        );
        let mut feedback_pub_builder = self
            .node
            .create_pub_impl::<FeedbackMessage<A>>(&feedback_topic_name, feedback_type_info);
        if let Some(qos) = self.feedback_topic_qos {
            feedback_pub_builder.entity.qos = qos.to_protocol_qos();
        }
        // Keep attachments enabled for RMW-Zenoh compatibility
        let feedback_pub = feedback_pub_builder.build()?;

        // Create status publisher using node API for proper graph registration
        // Use the action's status_type_info for proper ROS 2 interop
        let status_type_info = Some(A::status_type_info());
        let mut status_pub_builder = self
            .node
            .create_pub_impl::<StatusMessage>(&status_topic_name, status_type_info);
        let status_qos = self.status_topic_qos.unwrap_or(crate::qos::QosProfile {
            durability: crate::qos::QosDurability::TransientLocal,
            history: crate::qos::QosHistory::KeepLast(std::num::NonZeroUsize::new(1).unwrap()),
            ..Default::default()
        });
        status_pub_builder.entity.qos = status_qos.to_protocol_qos();
        // Keep attachments enabled for RMW-Zenoh compatibility
        let status_pub = status_pub_builder.build()?;

        let goal_manager = Arc::new(SafeGoalManager::new(self.result_timeout, self.goal_timeout));

        let cancellation_token = CancellationToken::new();
        let result_handler_token = CancellationToken::new();

        // Create the inner server
        let inner = Arc::new(InnerServer {
            goal_server: Arc::new(goal_server),
            result_server: Arc::new(result_server),
            cancel_server: Arc::new(cancel_server),
            feedback_pub: Arc::new(feedback_pub),
            status_pub: Arc::new(status_pub),
            goal_manager,
            result_handler_token: result_handler_token.clone(),
            driver_started: AtomicBool::new(false),
            goal_info: crate::compat::Mutex::new(HashMap::new()),
            goal_instances: crate::compat::Mutex::new(HashMap::new()),
            cancel_pending: crate::compat::Mutex::new(std::collections::HashSet::new()),
        });

        // Spawn background task to handle result requests (default mode for manual goal handling)
        // This task will be cancelled if with_handler() is called
        let weak_inner = Arc::downgrade(&inner);
        let global_shutdown = cancellation_token.clone();
        crate::compat::spawn(async move {
            let mut tasks = crate::compat::JoinSet::new();
            let mut expiration = Box::pin(crate::compat::sleep(Duration::from_millis(100)));
            loop {
                let Some(inner) = weak_inner.upgrade() else { break };
                tokio::select! {
                    biased;
                    _ = global_shutdown.cancelled() => break,
                    Some(_) = tasks.join_next() => {},
                    _ = &mut expiration => {
                        ZActionServer::from_inner(inner.clone()).expire_goals();
                        expiration = Box::pin(crate::compat::sleep(Duration::from_millis(100)));
                    },
                    query = inner.result_server.queue().recv_async() => {
                        tasks.spawn(async move {
                            super::driver::handle_result_request(&inner, query).await;
                        });
                    },
                }
            }
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
        });

        // Note: cancel requests are NOT handled by a background task in polling mode.
        // In polling mode (Python), cancel requests are processed on-demand via
        // GoalHandle::try_process_cancel(), called from the is_cancel_requested getter.
        // This avoids competing with explicit recv_cancel() calls in Rust code.
        // In driver mode (with_handler), the driver loop handles cancel requests.

        Ok(ZActionServer {
            inner,
            _shutdown: Arc::new(ShutdownGuard {
                token: cancellation_token,
            }),
        })
    }
}

/// Action server handle using the Handle Pattern.
///
/// This is a lightweight, cloneable handle that wraps the actual server implementation.
/// When all handles are dropped, the server automatically shuts down.
pub struct ZActionServer<A: ZAction> {
    inner: Arc<InnerServer<A>>,
    /// Drop guard that triggers shutdown when the last handle is dropped
    _shutdown: Arc<ShutdownGuard>,
}

impl<A: ZAction> std::fmt::Debug for ZActionServer<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZActionServer")
            .field("goal_server", &self.inner.goal_server)
            .finish_non_exhaustive()
    }
}

impl<A: ZAction> Clone for ZActionServer<A> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _shutdown: self._shutdown.clone(),
        }
    }
}

// Internal helper for driver to create server handles and access inner fields
impl<A: ZAction> ZActionServer<A> {
    pub(crate) fn from_inner(inner: Arc<InnerServer<A>>) -> Self {
        // Create a dummy shutdown guard that doesn't do anything
        // The driver doesn't control the server lifetime
        let dummy_token = CancellationToken::new();
        Self {
            inner,
            _shutdown: Arc::new(ShutdownGuard { token: dummy_token }),
        }
    }
}

// Provide convenient access to inner fields via getter methods
impl<A: ZAction> ZActionServer<A> {
    fn goal_server(&self) -> &Arc<crate::service::ZServer<GoalService<A>>> {
        &self.inner.goal_server
    }

    fn result_server(&self) -> &Arc<crate::service::ZServer<ResultService<A>>> {
        &self.inner.result_server
    }

    pub(crate) fn cancel_server(&self) -> &Arc<crate::service::ZServer<CancelService<A>>> {
        &self.inner.cancel_server
    }

    fn feedback_pub(
        &self,
    ) -> &Arc<crate::pubsub::ZPub<FeedbackMessage<A>, <FeedbackMessage<A> as ZMessage>::Serdes>>
    {
        &self.inner.feedback_pub
    }

    fn status_pub(
        &self,
    ) -> &Arc<crate::pubsub::ZPub<StatusMessage, <StatusMessage as ZMessage>::Serdes>> {
        &self.inner.status_pub
    }

    /// Access the goal manager for advanced use cases and testing.
    ///
    /// # Warning
    ///
    /// This is a low-level API that gives direct access to the goal state.
    /// Use with caution as it bypasses the normal goal handle abstractions.
    pub fn goal_manager(&self) -> &Arc<SafeGoalManager<A>> {
        &self.inner.goal_manager
    }

    fn result_handler_token(&self) -> &CancellationToken {
        &self.inner.result_handler_token
    }
}

impl<A: ZAction> ZActionServer<A> {
    pub(crate) fn publish_status(&self) {
        // Build status list while holding lock, then release before publishing
        let status_list: Vec<GoalStatusInfo> = self.goal_manager().read(|manager| {
            manager
                .goals
                .iter()
                .map(|(goal_id, state)| {
                    let status = match state {
                        ServerGoalState::Accepted { .. } if self.inner.cancel_pending.lock().contains(goal_id) => GoalStatus::Canceling,
                        ServerGoalState::Accepted { .. } => GoalStatus::Accepted,
                        ServerGoalState::Executing { cancel_flag, .. } if cancel_flag.load(Ordering::Relaxed) => GoalStatus::Canceling,
                        ServerGoalState::Executing { .. } => GoalStatus::Executing,
                        ServerGoalState::Canceling { .. } => GoalStatus::Canceling,
                        ServerGoalState::Terminated { status, .. } => *status,
                    };
                    GoalStatusInfo {
                        goal_info: self.inner.goal_info.lock().get(goal_id).cloned().unwrap_or_else(|| GoalInfo::new(*goal_id)),
                        status,
                    }
                })
                .collect()
        }); // Lock released here

        // Publish without holding lock
        let msg = StatusMessage { status_list };
        // FIXME: address the result
        let _ = self.status_pub().publish(&msg);
    }

    pub async fn recv_goal(&self) -> Result<GoalHandle<A, Requested>> {
        let query = self.goal_server().queue().recv_async().await;
        super::request_attachment(&query)?;
        let payload = query.payload().ok_or_else(|| zenoh::Error::from("Action request missing payload"))?.to_bytes();
        let request = <SendGoalRequest<A> as ZMessage>::deserialize(&payload)
            .map_err(|e| zenoh::Error::from(e.to_string()))?;

        Ok(GoalHandle {
            goal: request.goal,
            info: GoalInfo::new(request.goal_id),
            server: self.clone(),
            query: Some(query),
            cancel_flag: None,
            instance: None,
            _state: PhantomData,
        })
    }

    pub async fn recv_cancel(&self) -> Result<(CancelGoalServiceRequest, zenoh::query::Query)> {
        let query = self.cancel_server().queue().recv_async().await;
        super::request_attachment(&query)?;
        let payload = query.payload().ok_or_else(|| zenoh::Error::from("Action request missing payload"))?.to_bytes();
        let request = <CancelGoalServiceRequest as ZMessage>::deserialize(&payload)
            .map_err(|e| zenoh::Error::from(e.to_string()))?;
        Ok((request, query))
    }

    pub fn is_cancel_request_ready(&self) -> bool {
        !self.cancel_server().queue().is_empty()
    }

    /// Marks a goal as canceling by setting its atomic cancel flag.
    /// This is a lock-free operation that can be called from any thread.
    pub fn request_cancel(&self, goal_id: GoalId) -> bool {
        self.goal_manager().read(|manager| {
            if let Some(ServerGoalState::Executing { cancel_flag, .. }) =
                manager.goals.get(&goal_id)
            {
                cancel_flag.store(true, Ordering::Relaxed);
                true
            } else {
                false
            }
        })
    }

    pub async fn recv_result_request(&self) -> Result<(GoalId, zenoh::query::Query)> {
        let query = self.result_server().queue().recv_async().await;
        super::request_attachment(&query)?;
        let payload = query.payload().ok_or_else(|| zenoh::Error::from("Action request missing payload"))?.to_bytes();
        let request = <ResultRequest as ZMessage>::deserialize(&payload)
            .map_err(|e| zenoh::Error::from(e.to_string()))?;
        Ok((request.goal_id, query))
    }

    // FIXME: check the necessity
    pub fn send_goal_response_low(
        &self,
        query: &zenoh::query::Query,
        response: &GoalResponse,
    ) -> Result<()> {
        let response_bytes = <GoalResponse as ZMessage>::serialize(response);
        let attachment = super::request_attachment(&query)?;
        query
            .reply(query.key_expr().clone(), response_bytes)
            .attachment(attachment)
            .wait()
    }

    // FIXME: check the necessity
    pub async fn recv_cancel_request_low(
        &self,
    ) -> Result<(CancelGoalServiceRequest, zenoh::query::Query)> {
        let query = self.cancel_server().queue().recv_async().await;
        super::request_attachment(&query)?;
        let payload = query.payload().ok_or_else(|| zenoh::Error::from("Action request missing payload"))?.to_bytes();
        let request = <CancelGoalServiceRequest as ZMessage>::deserialize(&payload)
            .map_err(|e| zenoh::Error::from(e.to_string()))?;
        Ok((request, query))
    }

    pub fn send_cancel_response_low(
        &self,
        query: &zenoh::query::Query,
        response: &CancelGoalServiceResponse,
    ) -> Result<()> {
        let response_bytes = <CancelGoalServiceResponse as ZMessage>::serialize(response);
        let attachment = super::request_attachment(&query)?;
        query
            .reply(query.key_expr().clone(), response_bytes)
            .attachment(attachment)
            .wait()
    }

    // FIXME: check the necessity
    pub fn send_result_response_low(
        &self,
        query: &zenoh::query::Query,
        response: &GetResultResponse<A>,
    ) -> Result<()> {
        let response_bytes = <GetResultResponse<A> as ZMessage>::serialize(response);
        let attachment = super::request_attachment(&query)?;
        query
            .reply(query.key_expr().clone(), response_bytes)
            .attachment(attachment)
            .wait()
    }

    /// Attaches an automatic goal handler to the server.
    ///
    /// This method transitions the server from "manual mode" (where you call `recv_goal()`)
    /// to "automatic mode" (where goals are handled by the provided callback).
    ///
    /// Starts a goal/cancel driver while preserving the shared result handler.
    /// Calling this a second time on the same server panics.
    ///
    /// # Arguments
    ///
    /// * `handler` - Callback function that will be invoked for each accepted goal
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hiroz::action::*;
    /// # use hiroz_msgs::action_tutorials_interfaces::{FibonacciResult, action::Fibonacci};
    /// # let server: hiroz::action::server::ZActionServer<Fibonacci> = todo!();
    /// let server = server.with_handler(|executing: hiroz::action::server::ExecutingGoal<Fibonacci>| async move {
    ///     executing.succeed(FibonacciResult { sequence: vec![1, 1, 2, 3] }).unwrap();
    /// });
    /// ```
    pub fn with_handler<F, Fut>(self, handler: F) -> Self
    where
        F: Fn(GoalHandle<A, Executing>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        // Start a single goal/cancel driver. Results remain owned by the original
        // background task, so switching modes cannot lose an in-flight query.
        assert!(!self.inner.driver_started.swap(true, Ordering::AcqRel), "action handler already installed");
        self.result_handler_token().cancel();

        // 2. Start the full driver loop that handles all protocol events
        let weak_inner = Arc::downgrade(&self.inner);
        let shutdown_token = self._shutdown.token.clone();
        crate::compat::spawn(async move {
            crate::action::driver::run_driver_loop(weak_inner, shutdown_token, handler).await;
        });

        self
    }

    // Caller holds the goal manager lock: expiry and the terminal transition
    // must be one operation so waiters never observe an intermediate removal.
    fn abort_locked(
        &self,
        manager: &mut super::state::GoalManagerInternal<A>,
        id: GoalId,
        now: Instant,
    ) -> Option<(Vec<tokio::sync::oneshot::Sender<(A::Result, GoalStatus)>>, A::Result)> {
        match manager.goals.get(&id) {
            None | Some(ServerGoalState::Terminated { .. }) => return None,
            Some(ServerGoalState::Executing { cancel_flag, .. }) => {
                // Manual handlers have no executor-owned timeout future. Signal
                // their cooperative stop check before retiring the active state.
                cancel_flag.store(true, Ordering::Relaxed);
            }
            _ => {}
        }
        self.inner.cancel_pending.lock().remove(&id);
        let Some(result) = A::default_result() else {
            manager.goals.remove(&id);
            manager.result_futures.remove(&id);
            self.inner.goal_info.lock().remove(&id);
            self.inner.goal_instances.lock().remove(&id);
            return None;
        };
        manager.goals.insert(id, ServerGoalState::Terminated {
            result: result.clone(), status: GoalStatus::Aborted,
            timestamp: now, expires_at: Some(now + manager.result_timeout),
        });
        Some((manager.result_futures.remove(&id).unwrap_or_default(), result))
    }

    // Called while the manager lock is held, like all identity-map updates.
    fn is_current_instance(&self, id: GoalId, instance: Option<&Arc<()>>) -> bool {
        let instances = self.inner.goal_instances.lock();
        instance.zip(instances.get(&id)).is_some_and(|(handle, current)| Arc::ptr_eq(handle, current))
    }

    pub(crate) fn abort_unfinished(&self, id: GoalId, instance: &Arc<()>) {
        let notification = self.goal_manager().modify(|manager| {
            if !self.is_current_instance(id, Some(instance)) { return None; }
            self.abort_locked(manager, id, Instant::now())
        });
        if let Some((waiters, result)) = notification {
            for tx in waiters { let _ = tx.send((result.clone(), GoalStatus::Aborted)); }
        }
        self.publish_status();
    }

    /// Expires goals that have passed their expiration time.
    ///
    /// Timed-out Accepted/Executing goals become Aborted and retain their default
    /// result for the result timeout. Custom actions without a default result are
    /// removed instead. Terminated goals are removed once their results expire.
    ///
    /// Goals without expiration times (when timeouts are not configured) are never expired.
    ///
    /// # Returns
    ///
    /// Returns a vector of `GoalId`s that were expired.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hiroz::action::*;
    /// # use hiroz_msgs::action_tutorials_interfaces::action::Fibonacci;
    /// # let server: hiroz::action::server::ZActionServer<Fibonacci> = todo!();
    /// let expired = server.expire_goals();
    /// println!("Expired {} goals", expired.len());
    /// ```
    pub fn expire_goals(&self) -> Vec<GoalId> {
        let (expired, notifications) = self.goal_manager().modify(|manager| {
            let now = Instant::now();
            let expired: Vec<_> = manager.goals.iter().filter_map(|(id, state)| {
                let (deadline, active) = match state {
                    ServerGoalState::Accepted { expires_at, .. }
                    | ServerGoalState::Executing { expires_at, .. } => (expires_at, true),
                    ServerGoalState::Terminated { expires_at, .. } => (expires_at, false),
                    ServerGoalState::Canceling { .. } => return None,
                };
                deadline.filter(|deadline| now >= *deadline).map(|_| (*id, active))
            }).collect();
            let mut notifications = Vec::new();
            for (id, active) in &expired {
                if *active {
                    if let Some(notification) = self.abort_locked(manager, *id, now) {
                        notifications.push(notification);
                    }
                } else {
                    manager.goals.remove(id);
                    manager.result_futures.remove(id);
                    self.inner.goal_info.lock().remove(id);
                    self.inner.goal_instances.lock().remove(id);
                    self.inner.cancel_pending.lock().remove(id);
                }
            }
            (expired.into_iter().map(|(id, _)| id).collect::<Vec<_>>(), notifications)
        });
        // Wake result consumers and publish only after releasing the state lock.
        for (waiters, result) in notifications {
            for tx in waiters { let _ = tx.send((result.clone(), GoalStatus::Aborted)); }
        }
        if !expired.is_empty() { self.publish_status(); }
        expired
    }

    /// Sets the result timeout for this server.
    ///
    /// This configures how long the server will keep terminated goals
    /// before they expire. The background result service checks expiration
    /// periodically; `expire_goals()` can also force an immediate check.
    ///
    /// # Arguments
    ///
    /// * `timeout` - The result timeout duration
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hiroz::action::*;
    /// # use std::time::Duration;
    /// # use hiroz_msgs::action_tutorials_interfaces::action::Fibonacci;
    /// # let server: hiroz::action::server::ZActionServer<Fibonacci> = todo!();
    /// server.set_result_timeout(Duration::from_secs(30));
    /// ```
    pub fn set_result_timeout(&self, timeout: Duration) {
        self.goal_manager().modify(|manager| {
            manager.result_timeout = timeout;
        });
    }

    /// Gets the current result timeout for this server.
    ///
    /// # Returns
    ///
    /// The result timeout duration
    pub fn result_timeout(&self) -> Duration {
        self.goal_manager().read(|manager| manager.result_timeout)
    }
}

// --- State Markers for Type-State Pattern ---
/// Marker type representing a goal that has been requested but not yet accepted or rejected.
pub struct Requested;

/// Marker type representing a goal that has been accepted but not yet executing.
pub struct Accepted;

/// Marker type representing a goal that is currently executing.
pub struct Executing;

// Type aliases for convenience
/// A goal handle in the "Requested" state.
pub type RequestedGoal<A> = GoalHandle<A, Requested>;

/// A goal handle in the "Accepted" state.
pub type AcceptedGoal<A> = GoalHandle<A, Accepted>;

/// A goal handle in the "Executing" state.
pub type ExecutingGoal<A> = GoalHandle<A, Executing>;

// Type-state pattern for goal lifecycle with PhantomData markers
/// A type-safe goal handle that uses compile-time state tracking.
///
/// The `GoalHandle` is generic over the action type `A` and the state `State`.
/// Different methods are available depending on the current state, enforced at compile time.
///
/// # Type States
///
/// - `GoalHandle<A, Requested>`: Can be accepted or rejected
/// - `GoalHandle<A, Accepted>`: Can be executed
/// - `GoalHandle<A, Executing>`: Can publish feedback and be terminated
///
/// # Examples
///
/// ```no_run
/// # use hiroz::action::*;
/// # use hiroz_msgs::action_tutorials_interfaces::{FibonacciResult, action::Fibonacci};
/// # let server: std::sync::Arc<server::ZActionServer<Fibonacci>> = todo!();
/// # async {
/// let requested = server.recv_goal().await?;
/// let accepted = requested.accept();
/// let executing = accepted.execute();
/// executing.succeed(FibonacciResult { sequence: vec![] })?;
/// # Ok::<(), zenoh::Error>(())
/// # };
/// ```
pub struct GoalHandle<A: ZAction, State> {
    /// The goal data.
    pub goal: A::Goal,
    /// The goal metadata.
    pub info: GoalInfo,
    pub(crate) server: ZActionServer<A>,
    pub(crate) query: Option<zenoh::query::Query>,
    pub(crate) cancel_flag: Option<Arc<AtomicBool>>,
    pub(crate) instance: Option<Arc<()>>,
    pub(crate) _state: PhantomData<State>,
}

// --- State-specific implementations ---

/// Methods available only for goals in the "Requested" state.
impl<A: ZAction> GoalHandle<A, Requested> {
    /// Access the goal data.
    pub fn goal(&self) -> &A::Goal {
        &self.goal
    }

    /// Access the goal info.
    pub fn info(&self) -> &GoalInfo {
        &self.info
    }

    /// Accept this goal and transition to the "Accepted" state.
    ///
    /// This sends an acceptance response to the client and updates the server state.
    pub fn accept(self) -> GoalHandle<A, Accepted> {
        self.try_accept().expect("duplicate action goal UUID")
    }

    /// Accept a goal, rejecting duplicate UUIDs without replacing existing state.
    pub fn try_accept(mut self) -> Result<GoalHandle<A, Accepted>> {
        if let Some(query) = &self.query { super::request_attachment(query)?; }
        self.info = GoalInfo::new(self.info.goal_id);
        let instance = Arc::new(());
        let inserted = self.server.goal_manager().modify(|manager| {
            if manager.goals.contains_key(&self.info.goal_id) { return false; }
            self.server.inner.goal_info.lock().insert(self.info.goal_id, self.info.clone());
            self.server.inner.goal_instances.lock().insert(self.info.goal_id, instance.clone());
            let expires_at = manager.goal_timeout.map(|timeout| Instant::now() + timeout);
            manager.goals.insert(self.info.goal_id, ServerGoalState::Accepted {
                goal: self.goal.clone(), timestamp: Instant::now(), expires_at,
            });
            true
        });
        if !inserted {
            self.reject()?;
            return Err(zenoh::Error::from("duplicate action goal UUID"));
        }

        // Send acceptance response
        // Use timestamp from GoalInfo which is already in sec/nanosec format
        let response = SendGoalResponse {
            accepted: true,
            stamp_sec: self.info.stamp.sec,
            stamp_nanosec: self.info.stamp.nanosec,
        };
        let response_bytes = <SendGoalResponse as ZMessage>::serialize(&response);

        if let Some(query) = self.query.take() {
            let attachment = super::request_attachment(&query)?;
            // FIXME: address the result
            let _ = query
                .reply(query.key_expr().clone(), response_bytes)
                .attachment(attachment)
                .wait();
        }

        // Publish status update
        self.server.publish_status();

        Ok(GoalHandle {
            goal: self.goal,
            info: self.info,
            server: self.server,
            query: None,
            cancel_flag: None,
            instance: Some(instance),
            _state: PhantomData,
        })
    }

    /// Reject this goal.
    ///
    /// This sends a rejection response to the client. The goal will not be executed.
    pub fn reject(mut self) -> Result<()> {
        // Send rejection response
        let response = GoalResponse {
            accepted: false,
            stamp_sec: 0,
            stamp_nanosec: 0,
        };
        let response_bytes = <GoalResponse as ZMessage>::serialize(&response);

        if let Some(query) = self.query.take() {
            // FIXME: Address the unwrap usage
            let attachment = super::request_attachment(&query)?;
            let _ = query
                .reply(query.key_expr().clone(), response_bytes)
                .attachment(attachment)
                .wait();
        }
        Ok(())
    }
}

/// Methods available only for goals in the "Accepted" state.
impl<A: ZAction> GoalHandle<A, Accepted> {
    /// Access the goal data.
    pub fn goal(&self) -> &A::Goal {
        &self.goal
    }

    /// Access the goal info.
    pub fn info(&self) -> &GoalInfo {
        &self.info
    }

    /// Begin executing this goal and transition to the "Executing" state.
    ///
    /// This updates the server state to executing and publishes a status update.
    pub fn execute(self) -> GoalHandle<A, Executing> {
        // Create cancel flag
        let cancel_flag = Arc::new(AtomicBool::new(false));

        // Preserve the deadline from acceptance. A delayed handle must never
        // overwrite a terminal state created by expiration.
        self.server.goal_manager().modify(|manager| {
            if !self.server.is_current_instance(self.info.goal_id, self.instance.as_ref()) {
                cancel_flag.store(true, Ordering::Relaxed);
                return;
            }
            let Some(ServerGoalState::Accepted { expires_at, .. }) = manager.goals.get(&self.info.goal_id) else {
                cancel_flag.store(true, Ordering::Relaxed);
                return;
            };
            let expires_at = *expires_at;
            cancel_flag.store(self.server.inner.cancel_pending.lock().remove(&self.info.goal_id), Ordering::Relaxed);
            manager.goals.insert(self.info.goal_id, ServerGoalState::Executing {
                goal: self.goal.clone(), cancel_flag: cancel_flag.clone(), expires_at,
            });
        });

        self.server.publish_status();

        GoalHandle {
            goal: self.goal,
            info: self.info,
            server: self.server,
            query: None,
            cancel_flag: Some(cancel_flag),
            instance: self.instance,
            _state: PhantomData,
        }
    }
}

/// Methods available only for goals in the "Executing" state.
impl<A: ZAction> GoalHandle<A, Executing> {
    /// Access the goal data.
    pub fn goal(&self) -> &A::Goal {
        &self.goal
    }

    /// Access the goal info.
    pub fn info(&self) -> &GoalInfo {
        &self.info
    }

    /// Wait until at least `count` feedback subscribers are active, or `timeout` elapses.
    ///
    /// Call this before the first `publish_feedback` to ensure the client's feedback
    /// subscriber is registered before publishing starts.  Returns `true` if the
    /// required number of subscribers became active within the timeout.
    pub async fn wait_for_feedback_subscriber(
        &self,
        count: usize,
        timeout: std::time::Duration,
    ) -> bool {
        self.server
            .feedback_pub()
            .wait_for_subscription(count, timeout)
            .await
    }

    /// Publish feedback for this goal.
    ///
    /// Feedback can be published multiple times during goal execution to inform
    /// the client of progress.
    pub fn publish_feedback(&self, feedback: A::Feedback) -> Result<()> {
        let active = self.server.goal_manager().read(|manager| {
            self.server.is_current_instance(self.info.goal_id, self.instance.as_ref())
                && matches!(manager.goals.get(&self.info.goal_id), Some(ServerGoalState::Executing { .. }))
        });
        if !active { return Err(zenoh::Error::from("action goal is no longer executing")); }
        let msg = FeedbackMessage {
            goal_id: self.info.goal_id,
            feedback,
        };
        self.server.feedback_pub().publish(&msg)
    }

    /// Check if cancellation has been requested for this goal.
    ///
    /// This is a lock-free operation that can be called frequently from the
    /// goal execution loop. It also becomes true when the server aborts this
    /// goal after its configured execution timeout.
    ///
    /// # Returns
    ///
    /// `true` if a cancel request has been received, `false` otherwise.
    pub fn is_cancel_requested(&self) -> bool {
        self.cancel_flag
            .as_ref()
            .map(|flag| flag.load(Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// Check for and process any pending cancel request for this goal (polling mode).
    ///
    /// This is a non-blocking operation that drains the shared cancel queue via the
    /// shared server selector, applying each request to every matching goal.
    /// Returns `true` if a cancel was requested for this goal (either via the flag
    /// already set, or a newly routed request processed here).
    ///
    /// Fixes the silent-drop bug where a cancel for goal B would be lost if goal A's
    /// handle polled first and found a goal ID mismatch. Each goal now has its own
    /// dedicated channel; `drain()` routes all pending messages before we check ours.
    pub fn try_process_cancel(&self) -> bool {
        if !self.server.goal_manager().read(|_| self.server.is_current_instance(self.info.goal_id, self.instance.as_ref())) {
            return true;
        }
        while let Some(query) = self.server.cancel_server().queue().try_recv() {
            super::driver::handle_cancel_request(&self.server.inner, query);
        }
        self.is_cancel_requested()
    }

    /// Mark this goal as succeeded with the given result.
    ///
    /// This transitions the goal to a terminal state and consumes the handle.
    pub fn succeed(self, result: A::Result) -> Result<()> {
        self.terminate(result, GoalStatus::Succeeded)
    }

    /// Mark this goal as aborted with the given result.
    ///
    /// This transitions the goal to a terminal state and consumes the handle.
    pub fn abort(self, result: A::Result) -> Result<()> {
        self.terminate(result, GoalStatus::Aborted)
    }

    /// Mark this goal as canceled with the given result.
    ///
    /// This transitions the goal to a terminal state and consumes the handle.
    pub fn canceled(self, result: A::Result) -> Result<()> {
        self.terminate(result, GoalStatus::Canceled)
    }

    fn terminate(self, result: A::Result, status: GoalStatus) -> Result<()> {
        // An expired goal cannot be recreated by a late handler completion.
        let futures_to_notify = self.server.goal_manager().modify(|manager| {
            if !self.server.is_current_instance(self.info.goal_id, self.instance.as_ref())
                || !manager.goals.contains_key(&self.info.goal_id)
                || matches!(manager.goals.get(&self.info.goal_id), Some(ServerGoalState::Terminated { .. })) {
                return Vec::new();
            }
            let now = Instant::now();
            let expires_at = Some(now + manager.result_timeout);
            manager.goals.insert(
                self.info.goal_id,
                ServerGoalState::Terminated {
                    result: result.clone(),
                    status,
                    timestamp: now,
                    expires_at,
                },
            );

            // Take all waiting result futures for this goal
            manager
                .result_futures
                .remove(&self.info.goal_id)
                .unwrap_or_default()
        }); // Drop the lock before notifying futures and publishing status

        // Notify all waiting result futures
        for tx in futures_to_notify {
            let _ = tx.send((result.clone(), status));
        }

        self.server.publish_status();
        Ok(())
    }
}
