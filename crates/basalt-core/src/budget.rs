//! Cooperative CPU budget for tick-based systems.
//!
//! Systems receive a [`TickBudget`] via [`SystemContext::budget()`] that
//! tracks elapsed time against a configured limit. Budget-aware systems
//! check [`is_expired()`](TickBudget::is_expired) and yield early when
//! time runs out. Systems that ignore the budget run to completion —
//! enforcement is cooperative, not preemptive.

use std::time::{Duration, Instant};

/// A cooperative CPU budget for one system invocation.
///
/// Created by the dispatcher before each system runs. The system can
/// query remaining time to decide whether to continue processing
/// (e.g., a pathfinding system stops after its budget expires and
/// re-queues remaining requests for the next tick).
///
/// # Example
///
/// ```ignore
/// fn my_system(ctx: &mut dyn SystemContext) {
///     for request in pending_requests() {
///         if ctx.budget().is_expired() {
///             break; // yield, continue next tick
///         }
///         process(request, ctx);
///     }
/// }
/// ```
pub struct TickBudget {
    /// When this budget started (system dispatch time).
    start: Instant,
    /// Maximum allowed duration for this system.
    limit: Duration,
}

impl TickBudget {
    /// Creates a budget that starts now with the given time limit.
    pub fn new(limit: Duration) -> Self {
        Self {
            start: Instant::now(),
            limit,
        }
    }

    /// Creates an unlimited budget (never expires).
    ///
    /// Used for systems that have no configured budget and for
    /// backward compatibility with systems that don't check budgets.
    pub fn unlimited() -> Self {
        Self {
            start: Instant::now(),
            limit: Duration::MAX,
        }
    }

    /// Returns the time remaining before the budget expires.
    ///
    /// Returns [`Duration::ZERO`] if the budget is already expired.
    pub fn remaining(&self) -> Duration {
        self.limit.saturating_sub(self.start.elapsed())
    }

    /// Returns whether the budget has expired.
    pub fn is_expired(&self) -> bool {
        self.start.elapsed() >= self.limit
    }

    /// Returns the time elapsed since the budget was created.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Returns the configured time limit.
    pub fn limit(&self) -> Duration {
        self.limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_budget_never_expires() {
        let budget = TickBudget::unlimited();
        assert!(!budget.is_expired());
        assert!(budget.remaining() > Duration::from_secs(1000));
        assert_eq!(budget.limit(), Duration::MAX);
    }

    #[test]
    fn new_budget_starts_not_expired() {
        let budget = TickBudget::new(Duration::from_millis(100));
        assert!(!budget.is_expired());
        assert!(budget.remaining() > Duration::ZERO);
        assert_eq!(budget.limit(), Duration::from_millis(100));
    }

    #[test]
    fn zero_budget_expires_immediately() {
        let budget = TickBudget::new(Duration::ZERO);
        assert!(budget.is_expired());
        assert_eq!(budget.remaining(), Duration::ZERO);
    }

    #[test]
    fn elapsed_increases() {
        let budget = TickBudget::new(Duration::from_secs(10));
        let e1 = budget.elapsed();
        // Spin briefly
        std::hint::spin_loop();
        let e2 = budget.elapsed();
        assert!(e2 >= e1);
    }
}
