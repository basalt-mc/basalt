//! System registration types for the plugin API.
//!
//! These types allow plugins to register tick-based systems without
//! depending on the ECS storage engine. The server provides a
//! [`SystemContext`] implementation that wraps the real ECS.

use std::any::{Any, TypeId};
use std::collections::HashSet;
use std::time::Duration;

pub use crate::budget::TickBudget;
pub use crate::components::{EntityId, Phase};

/// Abstract interface for system runners.
///
/// Implemented by `basalt-server` to wrap the ECS. System closures
/// receive this trait object instead of a raw `&mut Ecs`, keeping
/// the ECS as an implementation detail.
///
/// Methods use `TypeId` + `dyn Any` internally. Typed access is
/// provided via free functions ([`get`], [`get_mut`], [`iter`]).
pub trait SystemContext {
    /// Returns a reference to the world for block/collision queries.
    fn world(&self) -> &basalt_world::World;

    /// Spawns a new entity and returns its unique ID.
    fn spawn(&mut self) -> EntityId;

    /// Removes an entity and all its components.
    fn despawn(&mut self, entity: EntityId);

    /// Sets a component value for an entity (type-erased).
    fn set_component(
        &mut self,
        entity: EntityId,
        type_id: TypeId,
        component: Box<dyn Any + Send + Sync>,
    );

    /// Returns all entity IDs that have a component of the given type.
    fn entities_with(&self, type_id: TypeId) -> Vec<EntityId>;

    /// Returns a reference to a component as `dyn Any`.
    fn get_component(&self, entity: EntityId, type_id: TypeId) -> Option<&dyn Any>;

    /// Returns a mutable reference to a component as `dyn Any`.
    fn get_component_mut(&mut self, entity: EntityId, type_id: TypeId) -> Option<&mut dyn Any>;

    /// Returns the CPU budget for the current system invocation.
    ///
    /// Budget-aware systems call this to check remaining time and yield
    /// early when expired. Systems that ignore the budget run to completion.
    fn budget(&self) -> &TickBudget;
}

/// Typed convenience methods on [`SystemContext`].
///
/// Implemented for `dyn SystemContext` so callers can write
/// `ctx.get::<Position>(id)` instead of raw `get_component` + downcast.
pub trait SystemContextExt {
    /// Returns a typed component reference.
    fn get<T: crate::components::Component>(&self, entity: EntityId) -> Option<&T>;

    /// Returns a typed mutable component reference.
    fn get_mut<T: crate::components::Component>(&mut self, entity: EntityId) -> Option<&mut T>;

    /// Sets a typed component value for an entity.
    fn set<T: crate::components::Component>(&mut self, entity: EntityId, component: T);

    /// Returns all entity IDs that have a component of type `T`.
    fn query<T: crate::components::Component>(&self) -> Vec<EntityId>;
}

impl<S: SystemContext + ?Sized> SystemContextExt for S {
    fn get<T: crate::components::Component>(&self, entity: EntityId) -> Option<&T> {
        self.get_component(entity, TypeId::of::<T>())
            .and_then(|any| any.downcast_ref::<T>())
    }

    fn get_mut<T: crate::components::Component>(&mut self, entity: EntityId) -> Option<&mut T> {
        self.get_component_mut(entity, TypeId::of::<T>())
            .and_then(|any| any.downcast_mut::<T>())
    }

    fn set<T: crate::components::Component>(&mut self, entity: EntityId, component: T) {
        self.set_component(entity, TypeId::of::<T>(), Box::new(component));
    }

    fn query<T: crate::components::Component>(&self) -> Vec<EntityId> {
        self.entities_with(TypeId::of::<T>())
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
    /// Optional CPU budget per invocation. `None` means unlimited.
    pub budget: Option<Duration>,
    /// The system function (type-erased).
    pub runner: Box<dyn SystemRunner>,
}

/// Trait for system execution functions.
///
/// Implemented by closures wrapped via [`SystemBuilder`].
pub trait SystemRunner: Send {
    /// Runs the system for one tick with a context.
    fn run(&mut self, ctx: &mut dyn SystemContext);
}

/// Blanket implementation for closures.
impl<F: FnMut(&mut dyn SystemContext) + Send> SystemRunner for F {
    fn run(&mut self, ctx: &mut dyn SystemContext) {
        self(ctx);
    }
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
    pub fn new() -> Self {
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
        for w in &self.writes {
            if other.reads.contains(w) || other.writes.contains(w) {
                return true;
            }
        }
        for w in &other.writes {
            if self.reads.contains(w) {
                return true;
            }
        }
        false
    }
}

impl Default for SystemAccess {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for declaring a system's metadata and component access.
pub struct SystemBuilder {
    name: String,
    phase: Phase,
    every: u64,
    access: SystemAccess,
    budget: Option<Duration>,
}

impl SystemBuilder {
    /// Creates a new system builder with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            phase: Phase::Simulate,
            every: 1,
            access: SystemAccess::new(),
            budget: None,
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
    pub fn reads<T: crate::components::Component>(mut self) -> Self {
        self.access.reads.insert(TypeId::of::<T>());
        self
    }

    /// Declares that this system writes a component type.
    pub fn writes<T: crate::components::Component>(mut self) -> Self {
        self.access.writes.insert(TypeId::of::<T>());
        self
    }

    /// Sets the CPU budget for this system in milliseconds.
    ///
    /// When set, the system can check `ctx.budget().is_expired()` to
    /// yield early. Systems without a budget get an unlimited one.
    pub fn budget_ms(mut self, ms: u64) -> Self {
        self.budget = Some(Duration::from_millis(ms));
        self
    }

    /// Finalizes the builder and registers the system with a runner.
    pub fn run<F: FnMut(&mut dyn SystemContext) + Send + 'static>(
        self,
        runner: F,
    ) -> SystemDescriptor {
        SystemDescriptor {
            name: self.name,
            phase: self.phase,
            every: self.every,
            access: self.access,
            budget: self.budget,
            runner: Box::new(runner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{Position, Velocity};

    #[test]
    fn system_builder_defaults() {
        let desc = SystemBuilder::new("test").run(|_ctx| {});
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
            .run(|_ctx| {});
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
}
