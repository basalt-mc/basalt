//! Tick loop abstraction for the Basalt server.
//!
//! Runs a closure at a configurable TPS (ticks per second) on a dedicated
//! OS thread. Uses sleep-based timing with drift correction: each tick
//! measures its own execution time and sleeps only for the remainder of
//! the tick budget. If a tick overruns, the sleep is skipped entirely
//! (no debt accumulation).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// A fixed-rate tick loop running on a dedicated OS thread.
///
/// The loop calls a user-provided callback at the configured TPS,
/// passing the current tick count (starting at 0). Timing uses
/// `Instant::now()` for drift correction — if the callback takes
/// less than one tick period, the thread sleeps for the remainder;
/// if it takes longer, the sleep is skipped and a warning is logged.
///
/// Shutdown is cooperative: calling [`stop`](Self::stop) sets an
/// `AtomicBool` flag and joins the thread, so the loop exits cleanly
/// after the current tick completes.
#[allow(dead_code)]
pub(crate) struct TickLoop {
    /// Shared flag — when set to `false`, the loop exits after the
    /// current tick finishes.
    running: Arc<AtomicBool>,
    /// Monotonically increasing tick counter, readable from any thread.
    tick_count: Arc<AtomicU64>,
    /// Handle to the OS thread. Consumed by [`stop`](Self::stop) when
    /// joining.
    handle: Option<JoinHandle<()>>,
}

#[allow(dead_code)]
impl TickLoop {
    /// Spawns a named OS thread that calls `callback` at the given TPS.
    ///
    /// The tick duration is computed as `1_000_000_000 / tps` nanoseconds.
    /// Each iteration records a start instant, invokes the callback with
    /// the current tick count, then sleeps for `tick_duration - elapsed`
    /// (or skips the sleep if the tick overran).
    ///
    /// # Panics
    ///
    /// Panics if the OS thread cannot be spawned (e.g., resource exhaustion).
    pub fn start<F>(name: &str, tps: u32, mut callback: F) -> Self
    where
        F: FnMut(u64) + Send + 'static,
    {
        let running = Arc::new(AtomicBool::new(true));
        let tick_count = Arc::new(AtomicU64::new(0));

        let running_clone = Arc::clone(&running);
        let tick_count_clone = Arc::clone(&tick_count);
        let thread_name = name.to_string();

        let tick_duration = Duration::from_nanos(1_000_000_000 / tps as u64);

        let handle = thread::Builder::new()
            .name(thread_name.clone())
            .spawn(move || {
                while running_clone.load(Ordering::Relaxed) {
                    let start = Instant::now();
                    let tick = tick_count_clone.load(Ordering::Relaxed);

                    callback(tick);

                    let elapsed = start.elapsed();
                    if elapsed < tick_duration {
                        thread::sleep(tick_duration - elapsed);
                    } else {
                        log::warn!(
                            "[{thread_name}] Tick {tick} overran: {elapsed:?} > {tick_duration:?}"
                        );
                    }

                    tick_count_clone.fetch_add(1, Ordering::Relaxed);
                }
            })
            .expect("failed to spawn tick loop thread");

        Self {
            running,
            tick_count,
            handle: Some(handle),
        }
    }

    /// Signals the loop to stop and waits for the thread to exit.
    ///
    /// Sets the `running` flag to `false`, then joins the OS thread.
    /// After this call, [`is_running`](Self::is_running) returns `false`
    /// and the thread handle is consumed.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    /// Returns the number of ticks completed so far.
    ///
    /// The counter starts at 0 and is incremented after each callback
    /// invocation, so the value is always the index of the *next* tick
    /// to run (or the total completed ticks).
    pub fn tick_count(&self) -> u64 {
        self.tick_count.load(Ordering::Relaxed)
    }

    /// Returns `true` if the loop is still running.
    ///
    /// This reads the shared `AtomicBool` — it becomes `false` after
    /// [`stop`](Self::stop) is called, even before the thread has
    /// fully joined.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

impl Drop for TickLoop {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_loop_runs_and_stops() {
        let mut tick_loop = TickLoop::start("test-ticks", 100, |_| {});

        thread::sleep(Duration::from_millis(50));
        tick_loop.stop();

        let count = tick_loop.tick_count();
        assert!(
            (3..=10).contains(&count),
            "expected 3-10 ticks at 100 TPS over 50ms, got {count}"
        );
    }

    #[test]
    fn tick_loop_callback_receives_tick_count() {
        let last_seen = Arc::new(AtomicU64::new(0));
        let last_seen_clone = Arc::clone(&last_seen);

        let mut tick_loop = TickLoop::start("test-callback", 100, move |tick| {
            last_seen_clone.store(tick, Ordering::Relaxed);
        });

        thread::sleep(Duration::from_millis(50));
        tick_loop.stop();

        // The callback stores the tick count *before* the counter is
        // incremented, so the last value seen equals tick_count - 1.
        let total = tick_loop.tick_count();
        let last = last_seen.load(Ordering::Relaxed);
        assert_eq!(
            last,
            total - 1,
            "last callback tick ({last}) should be tick_count - 1 ({total})"
        );
    }

    #[test]
    fn tick_loop_is_running() {
        let mut tick_loop = TickLoop::start("test-running", 100, |_| {});

        assert!(tick_loop.is_running(), "should be running after start");

        tick_loop.stop();

        assert!(!tick_loop.is_running(), "should not be running after stop");
    }
}
