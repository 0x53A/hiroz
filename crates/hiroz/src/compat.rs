//! Platform-specific sync primitives.
//!
//! On native targets this re-exports `parking_lot` types directly.
//! On `wasm32` this provides thin wrappers around `std::sync` that
//! match the `parking_lot` API (e.g. `lock()` returns a guard directly
//! instead of `Result`).

#[cfg(not(target_arch = "wasm32"))]
pub use parking_lot::{Condvar, Mutex, MutexGuard, RwLock};

#[cfg(not(target_arch = "wasm32"))]
pub use tokio_util::sync::CancellationToken;

/// Minimal `CancellationToken` stub for wasm32 (actions are not yet supported).
#[cfg(target_arch = "wasm32")]
pub use self::wasm_cancel::CancellationToken;

#[cfg(target_arch = "wasm32")]
mod wasm_cancel {
    /// Stub `CancellationToken` that compiles but panics at runtime.
    #[derive(Clone)]
    pub struct CancellationToken;

    impl CancellationToken {
        pub fn new() -> Self {
            Self
        }

        pub fn cancel(&self) {}

        pub async fn cancelled(&self) {
            // Never resolves - actions are not supported on wasm32 yet
            std::future::pending::<()>().await
        }

        pub fn child_token(&self) -> Self {
            Self
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use self::wasm_sync::*;

#[cfg(target_arch = "wasm32")]
mod wasm_sync {
    use std::sync;
    use std::time::Duration;

    /// Wrapper around `std::sync::RwLock` that matches `parking_lot::RwLock`'s API.
    pub struct RwLock<T>(sync::RwLock<T>);

    impl<T> RwLock<T> {
        pub fn new(val: T) -> Self {
            Self(sync::RwLock::new(val))
        }

        pub fn read(&self) -> sync::RwLockReadGuard<'_, T> {
            self.0.read().unwrap_or_else(|e| e.into_inner())
        }

        pub fn write(&self) -> sync::RwLockWriteGuard<'_, T> {
            self.0.write().unwrap_or_else(|e| e.into_inner())
        }
    }

    // Re-export the guard type so call-sites can name it.
    pub type MutexGuard<'a, T> = sync::MutexGuard<'a, T>;

    /// Wrapper around `std::sync::Mutex` that matches `parking_lot::Mutex`'s API.
    ///
    /// The key difference: `lock()` returns the guard directly instead of
    /// `Result<MutexGuard>`, recovering from poison by consuming the error.
    pub struct Mutex<T>(sync::Mutex<T>);

    impl<T> Mutex<T> {
        pub fn new(val: T) -> Self {
            Self(sync::Mutex::new(val))
        }

        pub fn lock(&self) -> sync::MutexGuard<'_, T> {
            self.0.lock().unwrap_or_else(|e| e.into_inner())
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
        pub fn wait<'a, T>(&self, guard: &mut sync::MutexGuard<'a, T>) {
            // Safety dance: take the guard out, pass to std Condvar (which consumes
            // it), then put the returned guard back.
            //
            // We use `unsafe` + `ptr::read` / `ptr::write` to move the guard in and
            // out without triggering the Drop that would unlock the mutex.
            unsafe {
                let taken = std::ptr::read(guard);
                let new_guard = self.0.wait(taken).unwrap_or_else(|e| e.into_inner());
                std::ptr::write(guard, new_guard);
            }
        }

        /// Block until notified or `timeout` elapses.
        pub fn wait_for<'a, T>(
            &self,
            guard: &mut sync::MutexGuard<'a, T>,
            timeout: Duration,
        ) -> WaitTimeoutResult {
            unsafe {
                let taken = std::ptr::read(guard);
                let (new_guard, result) = self
                    .0
                    .wait_timeout(taken, timeout)
                    .unwrap_or_else(|e| e.into_inner());
                std::ptr::write(guard, new_guard);
                WaitTimeoutResult(result.timed_out())
            }
        }

        pub fn notify_one(&self) {
            self.0.notify_one();
        }

        pub fn notify_all(&self) {
            self.0.notify_all();
        }
    }
}
