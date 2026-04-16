//! System scheduler with dependency graph and tick-phase execution.
//!
//! Systems declare which components they read and write. The scheduler
//! builds a dependency graph at startup and determines which systems
//! can run in parallel (non-conflicting component access) and which
//! must run sequentially (shared write access).
//!
//! Currently execution is sequential. The dependency graph is built
//! for future parallel dispatch via rayon.

use std::any::TypeId;
use std::collections::HashSet;

use crate::ecs::Ecs;

/// Execution phase within a game loop tick.
///
/// Systems are grouped by phase and run in phase order.
/// Within a phase, independent systems can run in parallel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Phase {
    /// Drain input channels and convert to events/state.
    Input,
    /// Validation checks (permissions, anti-cheat). Can cancel.
    Validate,
    /// Active simulation: physics, AI, pathfinding, block updates.
    Simulate,
    /// Logical state mutations from event handlers.
    Process,
    /// Collect diffs, encode packets, push to output channels.
    Output,
    /// Side effects: logs, analytics, persistence.
    Post,
}

/// Component access declaration for a system.
///
/// Tracks which component types a system reads and writes.
/// Two systems conflict if one writes a component the other
/// reads or writes.
#[derive(Debug, Clone)]
pub struct SystemAccess {
    /// Component types this system reads.
    pub reads: HashSet<TypeId>,
    /// Component types this system writes.
    pub writes: HashSet<TypeId>,
}

impl SystemAccess {
    /// Creates an empty access declaration.
    fn new() -> Self {
        Self {
            reads: HashSet::new(),
            writes: HashSet::new(),
        }
    }

    /// Returns whether this system conflicts with another.
    ///
    /// Two systems conflict if one writes a component type that
    /// the other reads or writes.
    pub fn conflicts_with(&self, other: &SystemAccess) -> bool {
        // My writes vs their reads or writes
        for w in &self.writes {
            if other.reads.contains(w) || other.writes.contains(w) {
                return true;
            }
        }
        // Their writes vs my reads
        for w in &other.writes {
            if self.reads.contains(w) {
                return true;
            }
        }
        false
    }
}

/// A registered system with its metadata.
pub struct SystemDescriptor {
    /// Human-readable name for logging.
    pub name: String,
    /// Which tick phase this system runs in.
    pub phase: Phase,
    /// Frequency divisor: runs when `tick_count % every == 0`.
    pub every: u64,
    /// Component access declaration.
    pub access: SystemAccess,
    /// The system function.
    pub runner: Box<dyn SystemRunner>,
}

/// Trait for system execution functions.
///
/// Implemented by closures wrapped via [`SystemScheduler::add`].
pub trait SystemRunner: Send {
    /// Runs the system for one tick.
    fn run(&mut self, ecs: &mut Ecs);
}

/// Blanket implementation for closures.
impl<F: FnMut(&mut Ecs) + Send> SystemRunner for F {
    fn run(&mut self, ecs: &mut Ecs) {
        self(ecs);
    }
}

/// Builder for declaring a system's metadata and component access.
pub struct SystemBuilder {
    name: String,
    phase: Phase,
    every: u64,
    access: SystemAccess,
}

impl SystemBuilder {
    /// Creates a new system builder with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            phase: Phase::Simulate,
            every: 1,
            access: SystemAccess::new(),
        }
    }

    /// Sets which tick phase this system runs in.
    pub fn phase(mut self, phase: Phase) -> Self {
        self.phase = phase;
        self
    }

    /// Sets the frequency divisor.
    ///
    /// The system runs when `tick_count % every == 0`.
    /// Default is 1 (every tick).
    pub fn every(mut self, every: u64) -> Self {
        self.every = every;
        self
    }

    /// Declares that this system reads a component type.
    pub fn reads<T: crate::Component>(mut self) -> Self {
        self.access.reads.insert(TypeId::of::<T>());
        self
    }

    /// Declares that this system writes a component type.
    pub fn writes<T: crate::Component>(mut self) -> Self {
        self.access.writes.insert(TypeId::of::<T>());
        self
    }

    /// Finalizes the builder and registers the system with a runner.
    pub fn run<F: FnMut(&mut Ecs) + Send + 'static>(self, runner: F) -> SystemDescriptor {
        SystemDescriptor {
            name: self.name,
            phase: self.phase,
            every: self.every,
            access: self.access,
            runner: Box::new(runner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{Position, Velocity};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn system_builder_defaults() {
        let desc = SystemBuilder::new("test").run(|_ecs| {});
        assert_eq!(desc.name, "test");
        assert_eq!(desc.phase, Phase::Simulate);
        assert_eq!(desc.every, 1);
    }

    #[test]
    fn system_builder_with_access() {
        let desc = SystemBuilder::new("physics")
            .phase(Phase::Simulate)
            .every(1)
            .reads::<Position>()
            .writes::<Position>()
            .writes::<Velocity>()
            .run(|_ecs| {});
        assert!(desc.access.reads.contains(&TypeId::of::<Position>()));
        assert!(desc.access.writes.contains(&TypeId::of::<Position>()));
        assert!(desc.access.writes.contains(&TypeId::of::<Velocity>()));
    }

    #[test]
    fn access_conflict_detection() {
        let mut a = SystemAccess::new();
        a.writes.insert(TypeId::of::<Position>());

        let mut b = SystemAccess::new();
        b.reads.insert(TypeId::of::<Position>());

        assert!(a.conflicts_with(&b));
        assert!(b.conflicts_with(&a));
    }

    #[test]
    fn no_conflict_for_disjoint_access() {
        let mut a = SystemAccess::new();
        a.reads.insert(TypeId::of::<Position>());

        let mut b = SystemAccess::new();
        b.reads.insert(TypeId::of::<Velocity>());

        assert!(!a.conflicts_with(&b));
    }

    #[test]
    fn read_read_no_conflict() {
        let mut a = SystemAccess::new();
        a.reads.insert(TypeId::of::<Position>());

        let mut b = SystemAccess::new();
        b.reads.insert(TypeId::of::<Position>());

        assert!(!a.conflicts_with(&b));
    }

    #[test]
    fn ecs_runs_systems_in_phase_order() {
        let mut ecs = Ecs::new();
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));

        let o1 = Arc::clone(&order);
        ecs.add_system(
            SystemBuilder::new("post_system")
                .phase(Phase::Post)
                .run(move |_| o1.lock().unwrap().push("post")),
        );

        let o2 = Arc::clone(&order);
        ecs.add_system(
            SystemBuilder::new("simulate_system")
                .phase(Phase::Simulate)
                .run(move |_| o2.lock().unwrap().push("simulate")),
        );

        let o3 = Arc::clone(&order);
        ecs.add_system(
            SystemBuilder::new("input_system")
                .phase(Phase::Input)
                .run(move |_| o3.lock().unwrap().push("input")),
        );

        ecs.run_all(0);

        let executed = order.lock().unwrap();
        assert_eq!(*executed, vec!["input", "simulate", "post"]);
    }

    #[test]
    fn every_divisor_skips_ticks() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = Arc::clone(&counter);

        let mut ecs = Ecs::new();
        ecs.add_system(
            SystemBuilder::new("slow_system")
                .phase(Phase::Simulate)
                .every(5)
                .run(move |_| {
                    c.fetch_add(1, Ordering::Relaxed);
                }),
        );

        for tick in 0..20 {
            ecs.run_all(tick);
        }

        // Ticks 0, 5, 10, 15 → 4 executions
        assert_eq!(counter.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn system_modifies_ecs() {
        let mut ecs = Ecs::new();
        let e = ecs.spawn();
        ecs.set(
            e,
            Velocity {
                dx: 0.0,
                dy: 0.0,
                dz: 0.0,
            },
        );

        ecs.add_system(
            SystemBuilder::new("gravity")
                .phase(Phase::Simulate)
                .writes::<Velocity>()
                .run(|ecs| {
                    let entities: Vec<_> = ecs.iter::<Velocity>().map(|(id, _)| id).collect();
                    for id in entities {
                        if let Some(vel) = ecs.get_mut::<Velocity>(id) {
                            vel.dy -= 0.08;
                        }
                    }
                }),
        );

        ecs.run_all(0);

        let vel = ecs.get::<Velocity>(e).unwrap();
        assert!((vel.dy - (-0.08)).abs() < f64::EPSILON);
    }

    #[test]
    fn phase_ordering() {
        assert!(Phase::Input < Phase::Validate);
        assert!(Phase::Validate < Phase::Simulate);
        assert!(Phase::Simulate < Phase::Process);
        assert!(Phase::Process < Phase::Output);
        assert!(Phase::Output < Phase::Post);
    }
}
