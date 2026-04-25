//! Parallel system context for concurrent ECS system dispatch.
//!
//! Provides [`ParallelSystemContext`] which implements [`SystemContext`]
//! using partitioned component store access, enabling multiple systems
//! to access disjoint component stores concurrently within a rayon scope.
//!
//! The key insight: within a parallel group, the dependency graph guarantees
//! that no two systems write the same component type. Write-stores are given
//! in exclusive ownership; read-stores are shared via `&dyn` references.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use basalt_api::budget::TickBudget;
use basalt_api::components::EntityId;
use basalt_api::system::SystemContext;

use crate::ecs::AnyComponentStore;

/// A deferred mutation to apply after a parallel group completes.
///
/// Systems running in parallel cannot directly spawn/despawn entities
/// because the alive list is shared. Instead, mutations are queued
/// and applied sequentially between groups.
pub(crate) enum DeferredCommand {
    /// Register a newly spawned entity in the alive list.
    Spawn { entity_id: EntityId },
    /// Remove an entity and all its components.
    Despawn { entity_id: EntityId },
}

/// System context for parallel execution within a rayon scope.
///
/// Each system in a parallel group receives its own context instance with:
/// - Exclusive access to its declared write-stores (moved out of the shared map)
/// - Shared read access to read-only stores (via references)
/// - A deferred command buffer for spawn/despawn operations
///
/// The lifetime `'scope` ties references to the enclosing rayon scope,
/// ensuring all borrowed data outlives the parallel tasks.
pub(crate) struct ParallelSystemContext<'scope> {
    /// Stores this system can write (exclusive ownership for the duration).
    write_stores: HashMap<TypeId, Box<dyn AnyComponentStore>>,
    /// Stores this system can only read (shared references).
    read_stores: HashMap<TypeId, &'scope dyn AnyComponentStore>,
    /// World reference for `SystemContext::world()`.
    world: &'scope basalt_world::World,
    /// Shared atomic counter for entity ID allocation.
    next_entity_id: &'scope AtomicU32,
    /// Deferred mutations collected during execution.
    pub(crate) deferred: Vec<DeferredCommand>,
    /// System name for error messages.
    system_name: String,
    /// CPU budget for this system invocation.
    budget: TickBudget,
}

impl<'scope> ParallelSystemContext<'scope> {
    /// Creates a new parallel context for one system.
    pub(crate) fn new(
        write_stores: HashMap<TypeId, Box<dyn AnyComponentStore>>,
        read_stores: HashMap<TypeId, &'scope dyn AnyComponentStore>,
        world: &'scope basalt_world::World,
        next_entity_id: &'scope AtomicU32,
        system_name: String,
        budget: TickBudget,
    ) -> Self {
        Self {
            write_stores,
            read_stores,
            world,
            next_entity_id,
            deferred: Vec::new(),
            system_name,
            budget,
        }
    }

    /// Returns the system name.
    pub(crate) fn system_name(&self) -> &str {
        &self.system_name
    }

    /// Consumes the context, returning write stores and deferred commands.
    pub(crate) fn into_parts(
        self,
    ) -> (
        HashMap<TypeId, Box<dyn AnyComponentStore>>,
        Vec<DeferredCommand>,
    ) {
        (self.write_stores, self.deferred)
    }
}

impl SystemContext for ParallelSystemContext<'_> {
    fn world(&self) -> &basalt_world::World {
        self.world
    }

    fn spawn(&mut self) -> EntityId {
        let id = self.next_entity_id.fetch_add(1, Ordering::Relaxed);
        self.deferred.push(DeferredCommand::Spawn { entity_id: id });
        id
    }

    fn despawn(&mut self, entity: EntityId) {
        self.deferred
            .push(DeferredCommand::Despawn { entity_id: entity });
    }

    fn set_component(
        &mut self,
        entity: EntityId,
        type_id: TypeId,
        component: Box<dyn Any + Send + Sync>,
    ) {
        if let Some(store) = self.write_stores.get_mut(&type_id) {
            store.set_any(entity, component);
        } else {
            panic!(
                "system '{}' called set_component for type {:?} without declaring writes access",
                self.system_name, type_id
            );
        }
    }

    fn entities_with(&self, type_id: TypeId) -> Vec<EntityId> {
        // Check write stores first (we have exclusive access, may have mutations)
        if let Some(store) = self.write_stores.get(&type_id) {
            return store.entity_ids();
        }
        // Then check read stores
        if let Some(store) = self.read_stores.get(&type_id) {
            return store.entity_ids();
        }
        // No store found — system did not declare access to this type
        Vec::new()
    }

    fn get_component(&self, entity: EntityId, type_id: TypeId) -> Option<&dyn Any> {
        // Check write stores first (may have fresh mutations)
        if let Some(store) = self.write_stores.get(&type_id) {
            return store.get_any(entity);
        }
        // Then check read stores
        if let Some(store) = self.read_stores.get(&type_id) {
            return store.get_any(entity);
        }
        None
    }

    fn get_component_mut(&mut self, entity: EntityId, type_id: TypeId) -> Option<&mut dyn Any> {
        if let Some(store) = self.write_stores.get_mut(&type_id) {
            return store.get_any_mut(entity);
        }
        panic!(
            "system '{}' called get_component_mut for type {:?} without declaring writes access",
            self.system_name, type_id
        );
    }

    fn budget(&self) -> &TickBudget {
        &self.budget
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_api::system::SystemContextExt;
    use std::sync::atomic::AtomicU32;

    #[derive(Debug, Clone, PartialEq)]
    struct Position {
        x: f64,
        y: f64,
    }
    impl basalt_api::components::Component for Position {}

    fn make_store_with<T: basalt_api::components::Component>(
        entries: Vec<(EntityId, T)>,
    ) -> Box<dyn AnyComponentStore> {
        let mut store = crate::ecs::new_component_store::<T>();
        for (id, val) in entries {
            store.set_any(id, Box::new(val));
        }
        store
    }

    fn make_world() -> basalt_world::World {
        basalt_world::World::new_memory(42)
    }

    #[test]
    fn get_component_from_write_store() {
        let world = make_world();
        let counter = AtomicU32::new(100);
        let mut write_stores: HashMap<TypeId, Box<dyn AnyComponentStore>> = HashMap::new();
        write_stores.insert(
            TypeId::of::<Position>(),
            make_store_with(vec![(1, Position { x: 5.0, y: 10.0 })]),
        );

        let ctx = ParallelSystemContext::new(
            write_stores,
            HashMap::new(),
            &world,
            &counter,
            "test".to_string(),
            TickBudget::unlimited(),
        );

        let pos = ctx.get::<Position>(1).unwrap();
        assert_eq!(pos.x, 5.0);
        assert_eq!(pos.y, 10.0);
    }

    #[test]
    fn get_component_from_read_store() {
        let world = make_world();
        let counter = AtomicU32::new(100);

        let pos_store = make_store_with(vec![(1, Position { x: 3.0, y: 7.0 })]);

        let mut read_stores: HashMap<TypeId, &dyn AnyComponentStore> = HashMap::new();
        read_stores.insert(TypeId::of::<Position>(), &*pos_store);

        let ctx = ParallelSystemContext::new(
            HashMap::new(),
            read_stores,
            &world,
            &counter,
            "test".to_string(),
            TickBudget::unlimited(),
        );

        let pos = ctx.get::<Position>(1).unwrap();
        assert_eq!(pos.x, 3.0);
    }

    #[test]
    fn get_component_mut_modifies_write_store() {
        let world = make_world();
        let counter = AtomicU32::new(100);
        let mut write_stores: HashMap<TypeId, Box<dyn AnyComponentStore>> = HashMap::new();
        write_stores.insert(
            TypeId::of::<Position>(),
            make_store_with(vec![(1, Position { x: 0.0, y: 0.0 })]),
        );

        let mut ctx = ParallelSystemContext::new(
            write_stores,
            HashMap::new(),
            &world,
            &counter,
            "test".to_string(),
            TickBudget::unlimited(),
        );

        let pos = ctx.get_mut::<Position>(1).unwrap();
        pos.x = 42.0;

        let pos = ctx.get::<Position>(1).unwrap();
        assert_eq!(pos.x, 42.0);
    }

    #[test]
    fn spawn_allocates_unique_ids_and_defers() {
        let world = make_world();
        let counter = AtomicU32::new(100);
        let mut ctx = ParallelSystemContext::new(
            HashMap::new(),
            HashMap::new(),
            &world,
            &counter,
            "test".to_string(),
            TickBudget::unlimited(),
        );

        let id1 = ctx.spawn();
        let id2 = ctx.spawn();
        assert_ne!(id1, id2);
        assert_eq!(ctx.deferred.len(), 2);
        assert!(
            matches!(ctx.deferred[0], DeferredCommand::Spawn { entity_id } if entity_id == id1)
        );
    }

    #[test]
    fn despawn_defers_command() {
        let world = make_world();
        let counter = AtomicU32::new(100);
        let mut ctx = ParallelSystemContext::new(
            HashMap::new(),
            HashMap::new(),
            &world,
            &counter,
            "test".to_string(),
            TickBudget::unlimited(),
        );

        ctx.despawn(5);
        assert_eq!(ctx.deferred.len(), 1);
        assert!(matches!(
            ctx.deferred[0],
            DeferredCommand::Despawn { entity_id: 5 }
        ));
    }

    #[test]
    fn entities_with_queries_write_stores() {
        let world = make_world();
        let counter = AtomicU32::new(100);

        let mut write_stores: HashMap<TypeId, Box<dyn AnyComponentStore>> = HashMap::new();
        write_stores.insert(
            TypeId::of::<Position>(),
            make_store_with(vec![
                (1, Position { x: 0.0, y: 0.0 }),
                (2, Position { x: 1.0, y: 1.0 }),
            ]),
        );

        let ctx = ParallelSystemContext::new(
            write_stores,
            HashMap::new(),
            &world,
            &counter,
            "test".to_string(),
            TickBudget::unlimited(),
        );

        let entities = ctx.entities_with(TypeId::of::<Position>());
        assert_eq!(entities.len(), 2);
    }

    #[test]
    #[should_panic(expected = "without declaring writes access")]
    fn set_component_panics_on_undeclared_type() {
        let world = make_world();
        let counter = AtomicU32::new(100);
        let mut ctx = ParallelSystemContext::new(
            HashMap::new(),
            HashMap::new(),
            &world,
            &counter,
            "test".to_string(),
            TickBudget::unlimited(),
        );

        ctx.set_component(
            1,
            TypeId::of::<Position>(),
            Box::new(Position { x: 0.0, y: 0.0 }),
        );
    }

    #[test]
    #[should_panic(expected = "without declaring writes access")]
    fn get_component_mut_panics_on_undeclared_type() {
        let world = make_world();
        let counter = AtomicU32::new(100);

        let mut ctx = ParallelSystemContext::new(
            HashMap::new(),
            HashMap::new(),
            &world,
            &counter,
            "test".to_string(),
            TickBudget::unlimited(),
        );

        ctx.get_component_mut(1, TypeId::of::<Position>());
    }

    #[test]
    fn into_write_stores_returns_mutated_stores() {
        let world = make_world();
        let counter = AtomicU32::new(100);
        let mut write_stores: HashMap<TypeId, Box<dyn AnyComponentStore>> = HashMap::new();
        write_stores.insert(
            TypeId::of::<Position>(),
            make_store_with(vec![(1, Position { x: 0.0, y: 0.0 })]),
        );

        let mut ctx = ParallelSystemContext::new(
            write_stores,
            HashMap::new(),
            &world,
            &counter,
            "test".to_string(),
            TickBudget::unlimited(),
        );

        // Mutate through context
        let pos = ctx.get_mut::<Position>(1).unwrap();
        pos.x = 99.0;

        // Get stores back and verify mutation persisted
        let (stores, _deferred) = ctx.into_parts();
        let store = stores.get(&TypeId::of::<Position>()).unwrap();
        let pos = store
            .get_any(1)
            .unwrap()
            .downcast_ref::<Position>()
            .unwrap();
        assert_eq!(pos.x, 99.0);
    }
}
