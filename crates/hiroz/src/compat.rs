//! Platform-specific sync primitives.
//!
//! On native targets this re-exports `parking_lot` types directly.
//! On `wasm32` this provides thin wrappers around `std::sync` that
//! match the `parking_lot` API (e.g. `lock()` returns a guard directly
//! instead of `Result`).

#[cfg(not(target_arch = "wasm32"))]
pub use parking_lot::{Condvar, Mutex, MutexGuard, RwLock};
#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;
#[cfg(not(target_arch = "wasm32"))]
pub use tokio::time::timeout;
pub use tokio_util::sync::CancellationToken;
#[cfg(target_arch = "wasm32")]
pub use zenoh_runtime::wasm_yield::Instant;

#[cfg(target_arch = "wasm32")]
pub async fn timeout<F: std::future::Future>(
    duration: std::time::Duration,
    future: F,
) -> Result<F::Output, ()> {
    use futures::FutureExt;
    let future = future.fuse();
    let timer = zenoh_runtime::wasm_yield::sleep(duration).fuse();
    futures::pin_mut!(future, timer);
    futures::select_biased! {
        value = future => Ok(value),
        _ = timer => Err(()),
    }
}

#[cfg(target_arch = "wasm32")]
pub use self::wasm_sync::*;

#[cfg(target_arch = "wasm32")]
mod wasm_sync {
    use std::{sync, time::Duration};

    /// Wrapper around `std::sync::RwLock` that matches `parking_lot::RwLock`'s API.
    pub struct RwLock<T>(sync::RwLock<T>);

    impl<T> RwLock<T> {
        pub fn new(val: T) -> Self {
            Self(sync::RwLock::new(val))
        }

        pub fn read(&self) -> sync::RwLockReadGuard<'_, T> {
            // Browser main threads cannot park on a contended native lock.
            loop {
                match self.0.try_read() {
                    Ok(guard) => return guard,
                    Err(sync::TryLockError::Poisoned(error)) => return error.into_inner(),
                    Err(sync::TryLockError::WouldBlock) => std::hint::spin_loop(),
                }
            }
        }

        pub fn write(&self) -> sync::RwLockWriteGuard<'_, T> {
            loop {
                match self.0.try_write() {
                    Ok(guard) => return guard,
                    Err(sync::TryLockError::Poisoned(error)) => return error.into_inner(),
                    Err(sync::TryLockError::WouldBlock) => std::hint::spin_loop(),
                }
            }
        }
    }

    // Re-export the guard type so call-sites can name it.
    pub struct MutexGuard<'a, T>(Option<sync::MutexGuard<'a, T>>);

    impl<T> std::ops::Deref for MutexGuard<'_, T> {
        type Target = T;
        fn deref(&self) -> &T {
            self.0.as_deref().expect("guard is present")
        }
    }
    impl<T> std::ops::DerefMut for MutexGuard<'_, T> {
        fn deref_mut(&mut self) -> &mut T {
            self.0.as_deref_mut().expect("guard is present")
        }
    }

    /// Wrapper around `std::sync::Mutex` that matches `parking_lot::Mutex`'s API.
    ///
    /// The key difference: `lock()` returns the guard directly instead of
    /// `Result<MutexGuard>`, recovering from poison by consuming the error.
    pub struct Mutex<T>(sync::Mutex<T>);

    impl<T> Mutex<T> {
        pub fn new(val: T) -> Self {
            Self(sync::Mutex::new(val))
        }

        pub fn lock(&self) -> MutexGuard<'_, T> {
            // Browser event loops cannot use Atomics.wait. These locks protect
            // short synchronous sections only; never hold one across an await.
            loop {
                match self.0.try_lock() {
                    Ok(guard) => return MutexGuard(Some(guard)),
                    Err(sync::TryLockError::Poisoned(error)) => return MutexGuard(Some(error.into_inner())),
                    Err(sync::TryLockError::WouldBlock) => std::hint::spin_loop(),
                }
            }
        }
    }

    /// Result of a timed wait on a `Condvar`, matching `parking_lot::WaitTimeoutResult`.
    pub struct WaitTimeoutResult(bool);

    impl WaitTimeoutResult {
        /// Returns `true` if the wait timed out.
        pub fn timed_out(&self) -> bool {
            self.0
        }
    }

    /// Wrapper around `std::sync::Condvar` that matches `parking_lot::Condvar`'s API.
    ///
    /// Key differences from `std::sync::Condvar`:
    /// - `wait` takes `&mut MutexGuard` instead of consuming it
    /// - `wait_for` wraps `wait_timeout` and returns a `WaitTimeoutResult`
    pub struct Condvar(sync::Condvar);

    impl Condvar {
        pub fn new() -> Self {
            Self(sync::Condvar::new())
        }

        /// Block until notified. Takes `&mut MutexGuard` to match parking_lot API.
        ///
        /// Internally this moves the guard through `std::sync::Condvar::wait` and
        /// writes the new guard back.
        pub fn wait<'a, T>(&self, guard: &mut MutexGuard<'a, T>) {
            let taken = guard.0.take().expect("guard is present");
            guard.0 = Some(self.0.wait(taken).unwrap_or_else(|e| e.into_inner()));
        }

        pub fn wait_for<'a, T>(
            &self,
            guard: &mut MutexGuard<'a, T>,
            timeout: Duration,
        ) -> WaitTimeoutResult {
            let taken = guard.0.take().expect("guard is present");
            let (new_guard, result) = self
                .0
                .wait_timeout(taken, timeout)
                .unwrap_or_else(|e| e.into_inner());
            guard.0 = Some(new_guard);
            WaitTimeoutResult(result.timed_out())
        }

        pub fn notify_one(&self) {
            self.0.notify_one();
        }

        pub fn notify_all(&self) {
            self.0.notify_all();
        }
    }
}

// Action execution must always pass through this platform boundary.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use tokio::{spawn, task::JoinSet, time::sleep};
#[cfg(target_arch = "wasm32")]
pub(crate) use zenoh_runtime::wasm_yield::sleep;

#[cfg(target_arch = "wasm32")]
pub(crate) fn spawn<F>(future: F) -> zenoh_runtime::JoinHandle<F::Output>
where F: std::future::Future + Send + 'static, F::Output: Send + 'static {
    zenoh_runtime::ZRuntime::Application.spawn(future)
}

#[cfg(target_arch = "wasm32")]
pub(crate) struct JoinSet<T: Send + 'static> {
    tasks: futures::stream::FuturesUnordered<zenoh_runtime::JoinHandle<T>>,
}
#[cfg(target_arch = "wasm32")]
impl<T: Send + 'static> JoinSet<T> {
    pub fn new() -> Self { Self { tasks: futures::stream::FuturesUnordered::new() } }
    pub fn spawn<F: std::future::Future<Output=T> + Send + 'static>(&mut self, future: F) {
        self.tasks.push(spawn(future));
    }
    pub async fn join_next(&mut self) -> Option<Result<T, zenoh_runtime::JoinError>> {
        use futures::StreamExt;
        self.tasks.next().await
    }
    pub fn abort_all(&mut self) {
        for task in self.tasks.iter() { task.abort(); }
    }
}
#[cfg(target_arch = "wasm32")]
impl<T: Send + 'static> Drop for JoinSet<T> {
    fn drop(&mut self) { self.abort_all(); }
}
