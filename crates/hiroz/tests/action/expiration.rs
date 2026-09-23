//! Tests for action goal expiration functionality.
//!
//! These tests verify that:
//! 1. Terminated goals expire after the result timeout
//! 2. Accepted/Executing goals can expire if goal timeout is configured
//! 3. Expired goals are properly cleaned up
//! 4. Status is updated when goals expire

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use tokio::time::sleep;

use hiroz::action::state::*;
use hiroz::action::*;
use hiroz::context::ZContextBuilder;
use hiroz::{Builder, Result, define_action};
use serde::{Deserialize, Serialize};

// Simple test action type
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestGoal {
    order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestResult {
    sequence: Vec<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestFeedback {
    current: i32,
}

struct TestAction;

define_action! {
    TestAction,
    action_name: "test_action/Expiration",
    Goal: TestGoal,
    Result: TestResult,
    Feedback: TestFeedback,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_terminated_goal_expiration() -> Result<()> {
    let ctx = ZContextBuilder::default().build()?;
    let node = ctx.create_node("test_expiration_node").build()?;

    // Create a server with short result timeout (1 second)
    let server = node
        .create_action_server::<TestAction>("test_action")
        .with_result_timeout(Duration::from_secs(1))
        .build()?;

    let goal_id = GoalId::new();

    // Simulate accepting and terminating a goal
    server.goal_manager().modify(|manager| {
        let now = Instant::now();
        manager.goals.insert(
            goal_id,
            ServerGoalState::Terminated {
                result: TestResult {
                    sequence: vec![0, 1, 1, 2, 3, 5],
                },
                status: GoalStatus::Succeeded,
                timestamp: now,
                expires_at: Some(now + Duration::from_secs(1)),
            },
        );
    });

    // Verify goal exists
    let goal_count = server.goal_manager().read(|manager| manager.goals.len());
    assert_eq!(goal_count, 1);

    // Wait for expiration time to pass
    sleep(Duration::from_millis(1200)).await;

    // Manually trigger expiration check
    let expired = server.expire_goals();

    // Verify the goal was expired
    // The background service expires even manually handled goals.
    // Explicit expiration is now idempotent after its automatic pass.
    assert!(expired.is_empty());

    // Verify goal was removed
    let goal_count = server.goal_manager().read(|manager| manager.goals.len());
    assert_eq!(goal_count, 0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_executing_goal_expiration_with_timeout() -> Result<()> {
    let ctx = ZContextBuilder::default().build()?;
    let node = ctx.create_node("test_expiration_node2").build()?;

    // Create a server with short goal timeout (1 second)
    let server = node
        .create_action_server::<TestAction>("test_action2")
        .with_goal_timeout(Duration::from_secs(1))
        .build()?;

    let goal_id = GoalId::new();

    // Simulate an executing goal with expiration
    server.goal_manager().modify(|manager| {
        let now = Instant::now();
        manager.goals.insert(
            goal_id,
            ServerGoalState::Executing {
                goal: TestGoal { order: 5 },
                cancel_flag: Arc::new(AtomicBool::new(false)),
                expires_at: Some(now + Duration::from_secs(1)),
            },
        );
    });

    // Verify goal exists and is executing
    let goal_count = server.goal_manager().read(|manager| manager.goals.len());
    assert_eq!(goal_count, 1);

    // Wait for expiration time to pass
    sleep(Duration::from_millis(1200)).await;

    // Manually trigger expiration check
    let expired = server.expire_goals();

    // Verify the executing goal was expired due to timeout
    // The background service expires even manually handled goals.
    // Explicit expiration is now idempotent after its automatic pass.
    assert!(expired.is_empty());

    // Verify goal was removed
    let goal_count = server.goal_manager().read(|manager| manager.goals.len());
    assert_eq!(goal_count, 0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_accepted_goal_expiration_with_timeout() -> Result<()> {
    let ctx = ZContextBuilder::default().build()?;
    let node = ctx.create_node("test_expiration_node3").build()?;

    // Create a server with short goal timeout (1 second)
    let server = node
        .create_action_server::<TestAction>("test_action3")
        .with_goal_timeout(Duration::from_secs(1))
        .build()?;

    let goal_id = GoalId::new();

    // Simulate an accepted goal with expiration
    server.goal_manager().modify(|manager| {
        let now = Instant::now();
        manager.goals.insert(
            goal_id,
            ServerGoalState::Accepted {
                goal: TestGoal { order: 5 },
                timestamp: now,
                expires_at: Some(now + Duration::from_secs(1)),
            },
        );
    });

    // Verify goal exists and is accepted
    let goal_count = server.goal_manager().read(|manager| manager.goals.len());
    assert_eq!(goal_count, 1);

    // Wait for expiration time to pass
    sleep(Duration::from_millis(1200)).await;

    // Manually trigger expiration check
    let expired = server.expire_goals();

    // Verify the accepted goal was expired due to timeout
    // The background service expires even manually handled goals.
    // Explicit expiration is now idempotent after its automatic pass.
    assert!(expired.is_empty());

    // Verify goal was removed
    let goal_count = server.goal_manager().read(|manager| manager.goals.len());
    assert_eq!(goal_count, 0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_no_expiration_without_timeout() -> Result<()> {
    let ctx = ZContextBuilder::default().build()?;
    let node = ctx.create_node("test_expiration_node4").build()?;

    // Create a server WITHOUT goal timeout
    let server = node
        .create_action_server::<TestAction>("test_action4")
        .build()?;

    let goal_id = GoalId::new();

    // Simulate an executing goal WITHOUT expiration (None)
    server.goal_manager().modify(|manager| {
        manager.goals.insert(
            goal_id,
            ServerGoalState::Executing {
                goal: TestGoal { order: 5 },
                cancel_flag: Arc::new(AtomicBool::new(false)),
                expires_at: None, // No expiration
            },
        );
    });

    // Wait a bit
    sleep(Duration::from_millis(1200)).await;

    // Trigger expiration check
    let expired = server.expire_goals();

    // Verify the goal was NOT expired (no timeout configured)
    assert_eq!(expired.len(), 0);

    // Verify goal still exists
    let goal_count = server.goal_manager().read(|manager| manager.goals.len());
    assert_eq!(goal_count, 1);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_multiple_goals_expiration() -> Result<()> {
    let ctx = ZContextBuilder::default().build()?;
    let node = ctx.create_node("test_expiration_node5").build()?;

    // Create a server with short timeouts
    let server = node
        .create_action_server::<TestAction>("test_action5")
        .with_result_timeout(Duration::from_secs(1))
        .with_goal_timeout(Duration::from_secs(1))
        .build()?;

    let goal_id1 = GoalId::new();
    let goal_id2 = GoalId::new();
    let goal_id3 = GoalId::new();

    // Add multiple goals with different states, all expiring
    server.goal_manager().modify(|manager| {
        let now = Instant::now();
        let expires = now + Duration::from_secs(1);

        manager.goals.insert(
            goal_id1,
            ServerGoalState::Terminated {
                result: TestResult {
                    sequence: vec![0, 1],
                },
                status: GoalStatus::Succeeded,
                timestamp: now,
                expires_at: Some(expires),
            },
        );

        manager.goals.insert(
            goal_id2,
            ServerGoalState::Executing {
                goal: TestGoal { order: 3 },
                cancel_flag: Arc::new(AtomicBool::new(false)),
                expires_at: Some(expires),
            },
        );

        manager.goals.insert(
            goal_id3,
            ServerGoalState::Accepted {
                goal: TestGoal { order: 2 },
                timestamp: now,
                expires_at: Some(expires),
            },
        );
    });

    // Verify all goals exist
    let goal_count = server.goal_manager().read(|manager| manager.goals.len());
    assert_eq!(goal_count, 3);

    // Wait for expiration
    sleep(Duration::from_millis(1200)).await;

    // Trigger expiration check
    let expired = server.expire_goals();

    // Verify all goals were expired
    assert!(expired.is_empty(), "background service already expired all goals");

    // Verify all goals were removed
    let goal_count = server.goal_manager().read(|manager| manager.goals.len());
    assert_eq!(goal_count, 0);

    Ok(())
}

// Generated actions provide this hook; reuse the test message implementations.
struct DefaultResultAction;
impl ZAction for DefaultResultAction {
    type Goal = TestGoal;
    type Result = TestResult;
    type Feedback = TestFeedback;
    fn name() -> &'static str { "test_action/DefaultExpiration" }
    fn default_result() -> Option<TestResult> { Some(TestResult { sequence: vec![] }) }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_manual_timeout_signals_handler_and_retains_aborted_result() -> Result<()> {
    let ctx = ZContextBuilder::default().build()?;
    let node = ctx.create_node("manual_timeout_signal").build()?;
    let server = node.create_action_server::<DefaultResultAction>("manual_timeout_signal")
        .with_goal_timeout(Duration::from_millis(100))
        .with_result_timeout(Duration::from_secs(60)).build()?;
    let client = node.create_action_client::<DefaultResultAction>("manual_timeout_signal").build()?;
    let (client_goal, executing) = tokio::join!(
        client.send_goal(TestGoal { order: 4 }),
        async { Ok::<_, zenoh::Error>(server.recv_goal().await?.try_accept()?.execute()) }
    );
    let client_goal = client_goal?;
    let executing = executing?;
    let id = executing.info.goal_id;
    let (status, result) = tokio::time::timeout(Duration::from_secs(2), client_goal.result_with_status())
        .await.expect("manual goal timeout must resolve its pending result")?;
    assert_eq!(status, GoalStatus::Aborted);
    assert!(result.sequence.is_empty());
    assert!(executing.is_cancel_requested(), "timed-out manual handler must receive a stop signal");
    assert_eq!(client.get_result_with_status(id).await?.0, GoalStatus::Aborted);
    assert!(server.expire_goals().is_empty(), "result must retain its own timeout");
    executing.succeed(TestResult { sequence: vec![99] })?;
    let (status, result) = client.get_result_with_status(id).await?;
    assert_eq!(status, GoalStatus::Aborted);
    assert!(result.sequence.is_empty(), "late completion must preserve aborted result");
    server.goal_manager().modify(|manager| {
        if let Some(ServerGoalState::Terminated { expires_at, .. }) = manager.goals.get_mut(&id) {
            *expires_at = Some(Instant::now());
        }
    });
    server.expire_goals();
    assert_eq!(client.get_result_with_status(id).await?.0, GoalStatus::Unknown);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_expiry_transitions_notify_all_waiters_and_preserve_future_goals() -> Result<()> {
    let ctx = ZContextBuilder::default().build()?;
    let node = ctx.create_node("atomic_expiration").build()?;
    let server = node.create_action_server::<DefaultResultAction>("atomic_expiration")
        .with_result_timeout(Duration::from_secs(60)).build()?;
    let mut receivers = Vec::new();
    let future_id = GoalId::new();
    server.goal_manager().modify(|manager| {
        let now = Instant::now();
        for _ in 0..128 {
            let id = GoalId::new();
            manager.goals.insert(id, ServerGoalState::Accepted {
                goal: TestGoal { order: 1 }, timestamp: now, expires_at: Some(now),
            });
            let (tx, rx) = tokio::sync::oneshot::channel();
            manager.result_futures.insert(id, vec![tx]);
            receivers.push(rx);
        }
        manager.goals.insert(future_id, ServerGoalState::Accepted {
            goal: TestGoal { order: 2 }, timestamp: now,
            expires_at: Some(now + Duration::from_secs(60)),
        });
    });
    server.expire_goals();
    for receiver in receivers {
        let (result, status) = receiver.await.expect("expiry must send a terminal result before releasing waiters");
        assert_eq!(status, GoalStatus::Aborted);
        assert!(result.sequence.is_empty());
    }
    server.goal_manager().read(|manager| {
        assert_eq!(manager.goals.len(), 129);
        assert!(matches!(manager.goals.get(&future_id), Some(ServerGoalState::Accepted { .. })));
        assert_eq!(manager.goals.values().filter(|state| matches!(state, ServerGoalState::Terminated { status: GoalStatus::Aborted, .. })).count(), 128);
        assert!(manager.result_futures.is_empty());
    });
    Ok(())
}

fn expire_instance(server: &hiroz::action::server::ZActionServer<DefaultResultAction>, id: GoalId) {
    // Advance both deadlines explicitly, avoiding timing-dependent UUID reuse.
    for _ in 0..2 {
        server.goal_manager().modify(|manager| {
            let deadline = match manager.goals.get_mut(&id).expect("goal exists before expiration") {
                ServerGoalState::Accepted { expires_at, .. }
                | ServerGoalState::Executing { expires_at, .. }
                | ServerGoalState::Terminated { expires_at, .. } => expires_at,
                _ => panic!("unexpected canceling state"),
            };
            *deadline = Some(Instant::now());
        });
        server.expire_goals();
    }
    assert!(!server.goal_manager().read(|manager| manager.goals.contains_key(&id)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_reused_uuid_isolated_from_stale_accepted_and_executing_handles() -> Result<()> {
    use hiroz::action::messages::{GoalService, SendGoalRequest};
    let ctx = ZContextBuilder::default().build()?;
    let node = ctx.create_node("reused_goal_uuid").build()?;
    let server = node.create_action_server::<DefaultResultAction>("reused_goal_uuid").build()?;
    let raw_client = node.create_client::<GoalService<DefaultResultAction>>("reused_goal_uuid/_action/send_goal").build()?;
    let client = node.create_action_client::<DefaultResultAction>("reused_goal_uuid").build()?;
    for execute_original in [false, true] {
        let id = GoalId::new();
        let request = SendGoalRequest::<DefaultResultAction> { goal_id: id, goal: TestGoal { order: 1 } };
        let (response, accepted) = tokio::join!(
            raw_client.call_with_timeout(&request, Duration::from_secs(2)),
            async { server.recv_goal().await?.try_accept() }
        );
        assert!(response?.accepted);
        let original = accepted?;
        let (old_accepted, old_executing) = if execute_original {
            (None, Some(original.execute()))
        } else { (Some(original), None) };
        expire_instance(&server, id);
        let request = SendGoalRequest::<DefaultResultAction> { goal_id: id, goal: TestGoal { order: 2 } };
        let (response, accepted) = tokio::join!(
            raw_client.call_with_timeout(&request, Duration::from_secs(2)),
            async { server.recv_goal().await?.try_accept() }
        );
        assert!(response?.accepted, "UUID may be accepted again after retention expires");
        let replacement = accepted?;
        let stale = old_executing.unwrap_or_else(|| old_accepted.unwrap().execute());
        assert!(stale.is_cancel_requested());
        assert!(stale.try_process_cancel());
        assert!(stale.publish_feedback(TestFeedback { current: 99 }).is_err());
        if execute_original {
            stale.succeed(TestResult { sequence: vec![99] })?;
        } else {
            stale.canceled(TestResult { sequence: vec![99] })?;
        }
        server.goal_manager().read(|manager| {
            let Some(ServerGoalState::Accepted { goal, .. }) = manager.goals.get(&id) else {
                panic!("stale handle altered replacement state");
            };
            assert_eq!(goal.order, 2, "stale execute must not restore the original payload");
        });
        let executing = replacement.execute();
        assert!(!executing.is_cancel_requested());
        executing.publish_feedback(TestFeedback { current: 2 })?;
        executing.succeed(TestResult { sequence: vec![2] })?;
        let (status, result) = client.get_result_with_status(id).await?;
        assert_eq!(status, GoalStatus::Succeeded);
        assert_eq!(result.sequence, vec![2]);
    }
    Ok(())
}
