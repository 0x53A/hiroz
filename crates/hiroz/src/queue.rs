//! Bounded queue implementation for ROS depth QoS behavior.
//!
//! This module provides a thread-safe bounded queue that drops the OLDEST element
//! when full, matching the expected behavior of ROS 2 depth QoS.

use std::collections::VecDeque;
use std::time::Duration;

use event_listener::Event;
use crate::compat::{Condvar, Mutex};

/// A bounded queue that drops the OLDEST element when full (ROS depth QoS behavior).
///
/// Unlike channels that block or drop the newest element when full, this queue
/// maintains the most recent N elements, where N is the capacity.
pub struct BoundedQueue<T> {
    data: Mutex<VecDeque<T>>,
    /// Condvar for blocking recv operations
    not_empty: Condvar,
    /// Runtime-agnostic async notification
    event: Event,
    /// Maximum capacity (usize::MAX = unlimited for KeepAll)
    capacity: usize,
}

impl<T> BoundedQueue<T> {
    /// Create a new bounded queue with the specified capacity.
    ///
    /// A capacity of `usize::MAX` effectively makes the queue unbounded (KeepAll).
    pub fn new(capacity: usize) -> Self {
        Self {
            data: Mutex::new(VecDeque::with_capacity(capacity.min(1024))),
            not_empty: Condvar::new(),
            event: Event::new(),
            capacity,
        }
    }

    /// Push an item to the queue, dropping the OLDEST if at capacity.
    ///
    /// Returns `true` if an item was dropped, `false` otherwise.
    pub fn push(&self, item: T) -> bool {
        let mut data = self.data.lock();
        let dropped = if data.len() >= self.capacity {
            data.pop_front();
            true
        } else {
            false
        };
        data.push_back(item);
        self.not_empty.notify_one();
        self.event.notify(1);
        dropped
    }

    /// Blocking receive - waits until an item is available.
    pub fn recv(&self) -> T {
        let mut data = self.data.lock();
        while data.is_empty() {
            self.not_empty.wait(&mut data);
        }
        data.pop_front()
            .expect("queue should not be empty after wait")
    }

    /// Receive with timeout.
    ///
    /// Returns `Some(item)` if an item was received within the timeout,
    /// `None` if the timeout expired.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<T> {
        let started = crate::compat::Instant::now();
        let mut data = self.data.lock();
        loop {
            if let Some(item) = data.pop_front() {
                return Some(item);
            }
            // A notification does not guarantee an item: wakes can be spurious,
            // or another receiver can take it before this lock is reacquired.
            // Recheck the predicate without restarting the original deadline.
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return None;
            }
            self.not_empty.wait_for(&mut data, remaining);
        }
    }

    /// Non-blocking receive.
    ///
    /// Returns `Some(item)` if an item was available, `None` otherwise.
    pub fn try_recv(&self) -> Option<T> {
        self.data.lock().pop_front()
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.data.lock().is_empty()
    }

    /// Get the current number of items in the queue.
    pub fn len(&self) -> usize {
        self.data.lock().len()
    }

    /// Async receive - waits until an item is available.
    ///
    /// This method is cancel-safe: if the future is dropped before completion,
    /// no item will be lost.
    pub async fn recv_async(&self) -> T {
        loop {
            // Register listener before checking to avoid race
            let listener = self.event.listen();

            // Check if there's an item available
            if let Some(item) = self.try_recv() {
                return item;
            }

            // Wait for notification
            listener.await;
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod timeout_tests {
    use super::*;
    use std::{sync::Arc, thread, time::Instant};

    #[test]
    fn timed_receive_ignores_empty_notifications() {
        let queue = Arc::new(BoundedQueue::new(1));
        let receiver = queue.clone();
        let started = Arc::new(std::sync::Barrier::new(2));
        let waiter_started = started.clone();
        let waiter = thread::spawn(move || {
            waiter_started.wait();
            receiver.recv_timeout(Duration::from_millis(500))
        });
        started.wait();
        // Model spurious notifications or a competing receiver taking the item
        // before this receiver reacquires the queue lock.
        for _ in 0..5 {
            thread::sleep(Duration::from_millis(10));
            queue.not_empty.notify_all();
        }
        queue.push(73);
        assert_eq!(waiter.join().unwrap(), Some(73));
    }

    #[test]
    fn timed_receive_empty_notifications_do_not_extend_deadline() {
        let queue = Arc::new(BoundedQueue::<u8>::new(1));
        let receiver = queue.clone();
        let waiter = thread::spawn(move || {
            let start = Instant::now();
            let value = receiver.recv_timeout(Duration::from_millis(80));
            (value, start.elapsed())
        });
        for _ in 0..15 {
            thread::sleep(Duration::from_millis(10));
            queue.not_empty.notify_all();
        }
        let (value, elapsed) = waiter.join().unwrap();
        assert_eq!(value, None);
        assert!(elapsed >= Duration::from_millis(75), "returned early: {elapsed:?}");
        assert!(elapsed < Duration::from_millis(200), "deadline restarted: {elapsed:?}");
    }
}
