//! Core ECS types: entity IDs, component storage, and the ECS world.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

/// A unique entity identifier.
///
/// Entities are just IDs — all data lives in component stores.
/// IDs are never reused within a server session.
pub type EntityId = u32;

/// Marker trait for component types stored in the ECS.
///
/// Components must be `Send + Sync + 'static` so they can be
/// accessed from the game loop thread and (in the future) from
/// parallel system threads via rayon.
pub trait Component: Send + Sync + 'static {}

/// Type-erased component store, allowing the [`Ecs`] to hold
/// stores for different component types in a single HashMap.
trait AnyComponentStore: Send + Sync {
    /// Returns `self` as `&dyn Any` for downcasting.
    fn as_any(&self) -> &dyn Any;
    /// Returns `self` as `&mut dyn Any` for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Removes all components for the given entity.
    fn remove(&mut self, entity: EntityId);
    /// Returns the number of components stored.
    fn len(&self) -> usize;
}

/// Typed storage for a single component type.
///
/// One `ComponentStore<T>` exists per registered component type.
/// Backed by a `HashMap<EntityId, T>` for O(1) access by entity ID.
struct ComponentStore<T: Component> {
    data: HashMap<EntityId, T>,
}

impl<T: Component> ComponentStore<T> {
    fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }
}

impl<T: Component + 'static> AnyComponentStore for ComponentStore<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn remove(&mut self, entity: EntityId) {
        self.data.remove(&entity);
    }

    fn len(&self) -> usize {
        self.data.len()
    }
}

/// The ECS world containing all component stores, entities, and systems.
///
/// Owned exclusively by the game loop. Provides entity lifecycle
/// (spawn, despawn), typed component access (get, set, remove),
/// and system scheduling (add_system, run_phase, run_all).
pub struct Ecs {
    /// Component stores indexed by component `TypeId`.
    stores: HashMap<TypeId, Box<dyn AnyComponentStore>>,
    /// Monotonically increasing entity ID counter.
    next_entity_id: AtomicU32,
    /// Set of all living entities.
    alive: Vec<EntityId>,
    /// Registered systems, sorted by phase.
    systems: Vec<crate::system::SystemDescriptor>,
    /// UUID → EntityId index for O(1) lookups by Minecraft UUID.
    /// Updated via `index_uuid` / `despawn`.
    uuid_index: HashMap<basalt_types::Uuid, EntityId>,
}

impl Ecs {
    /// Creates an empty ECS world with no entities, components, or systems.
    pub fn new() -> Self {
        Self {
            stores: HashMap::new(),
            next_entity_id: AtomicU32::new(1),
            alive: Vec::new(),
            systems: Vec::new(),
            uuid_index: HashMap::new(),
        }
    }

    /// Registers a component type so entities can have it.
    ///
    /// Must be called before any `set` for this component type.
    /// Typically called during plugin registration.
    pub fn register_component<T: Component>(&mut self) {
        let type_id = TypeId::of::<T>();
        self.stores
            .entry(type_id)
            .or_insert_with(|| Box::new(ComponentStore::<T>::new()));
    }

    /// Spawns a new entity and returns its unique ID.
    ///
    /// The entity starts with no components. Use [`set`](Self::set)
    /// to attach components after spawning.
    pub fn spawn(&mut self) -> EntityId {
        let id = self.next_entity_id.fetch_add(1, Ordering::Relaxed);
        self.alive.push(id);
        id
    }

    /// Spawns an entity with a specific ID.
    ///
    /// Used when the entity ID is assigned externally (e.g., player
    /// entity IDs from the server's atomic counter). If the ID is
    /// already in use, the existing entity is NOT overwritten.
    pub fn spawn_with_id(&mut self, id: EntityId) {
        if !self.alive.contains(&id) {
            self.alive.push(id);
        }
    }

    /// Despawns an entity, removing all its components and UUID index.
    pub fn despawn(&mut self, entity: EntityId) {
        self.alive.retain(|&e| e != entity);
        self.uuid_index.retain(|_, &mut eid| eid != entity);
        for store in self.stores.values_mut() {
            store.remove(entity);
        }
    }

    /// Returns whether the entity is alive.
    pub fn is_alive(&self, entity: EntityId) -> bool {
        self.alive.contains(&entity)
    }

    /// Returns all living entity IDs.
    pub fn entities(&self) -> &[EntityId] {
        &self.alive
    }

    /// Returns the number of living entities.
    pub fn entity_count(&self) -> usize {
        self.alive.len()
    }

    /// Sets a component value for an entity.
    ///
    /// If the component type is not registered, it is registered
    /// automatically. Overwrites any existing value for this entity.
    pub fn set<T: Component>(&mut self, entity: EntityId, component: T) {
        let type_id = TypeId::of::<T>();
        let store = self
            .stores
            .entry(type_id)
            .or_insert_with(|| Box::new(ComponentStore::<T>::new()));
        let typed = store
            .as_any_mut()
            .downcast_mut::<ComponentStore<T>>()
            .expect("component store type mismatch");
        typed.data.insert(entity, component);
    }

    /// Returns a reference to an entity's component, if it exists.
    pub fn get<T: Component>(&self, entity: EntityId) -> Option<&T> {
        let type_id = TypeId::of::<T>();
        let store = self.stores.get(&type_id)?;
        let typed = store
            .as_any()
            .downcast_ref::<ComponentStore<T>>()
            .expect("component store type mismatch");
        typed.data.get(&entity)
    }

    /// Returns a mutable reference to an entity's component, if it exists.
    pub fn get_mut<T: Component>(&mut self, entity: EntityId) -> Option<&mut T> {
        let type_id = TypeId::of::<T>();
        let store = self.stores.get_mut(&type_id)?;
        let typed = store
            .as_any_mut()
            .downcast_mut::<ComponentStore<T>>()
            .expect("component store type mismatch");
        typed.data.get_mut(&entity)
    }

    /// Removes a component from an entity. Returns the removed value.
    pub fn remove_component<T: Component>(&mut self, entity: EntityId) -> Option<T> {
        let type_id = TypeId::of::<T>();
        let store = self.stores.get_mut(&type_id)?;
        let typed = store
            .as_any_mut()
            .downcast_mut::<ComponentStore<T>>()
            .expect("component store type mismatch");
        typed.data.remove(&entity)
    }

    /// Returns whether an entity has a specific component.
    pub fn has<T: Component>(&self, entity: EntityId) -> bool {
        self.get::<T>(entity).is_some()
    }

    /// Returns an iterator over all `(EntityId, &T)` pairs for a component type.
    pub fn iter<T: Component>(&self) -> impl Iterator<Item = (EntityId, &T)> {
        let type_id = TypeId::of::<T>();
        self.stores
            .get(&type_id)
            .and_then(|store| store.as_any().downcast_ref::<ComponentStore<T>>())
            .into_iter()
            .flat_map(|store| store.data.iter().map(|(&id, comp)| (id, comp)))
    }

    /// Returns a mutable iterator over all `(EntityId, &mut T)` pairs.
    pub fn iter_mut<T: Component>(&mut self) -> impl Iterator<Item = (EntityId, &mut T)> {
        let type_id = TypeId::of::<T>();
        self.stores
            .get_mut(&type_id)
            .and_then(|store| store.as_any_mut().downcast_mut::<ComponentStore<T>>())
            .into_iter()
            .flat_map(|store| store.data.iter_mut().map(|(&id, comp)| (id, comp)))
    }

    /// Returns the number of components stored for a given type.
    pub fn component_count<T: Component>(&self) -> usize {
        let type_id = TypeId::of::<T>();
        self.stores.get(&type_id).map_or(0, |store| store.len())
    }

    // -- System scheduling --

    /// Registers a system for tick-phase execution.
    pub fn add_system(&mut self, system: crate::system::SystemDescriptor) {
        self.systems.push(system);
        self.systems.sort_by_key(|s| s.phase);
    }

    /// Runs all systems for the given tick and phase.
    ///
    /// Systems are temporarily extracted from `self` to avoid a
    /// double mutable borrow (`self.systems` + `&mut self` passed
    /// to each system runner). They are put back after execution.
    pub fn run_phase(&mut self, phase: crate::system::Phase, tick: u64) {
        let mut systems = std::mem::take(&mut self.systems);
        for system in &mut systems {
            if system.phase == phase && tick.is_multiple_of(system.every) {
                system.runner.run(self);
            }
        }
        self.systems = systems;
    }

    /// Runs all phases in order for the given tick.
    pub fn run_all(&mut self, tick: u64) {
        use crate::system::Phase;
        for phase in [
            Phase::Input,
            Phase::Validate,
            Phase::Simulate,
            Phase::Process,
            Phase::Output,
            Phase::Post,
        ] {
            self.run_phase(phase, tick);
        }
    }

    /// Returns the number of registered systems.
    pub fn system_count(&self) -> usize {
        self.systems.len()
    }

    /// Associates a UUID with an entity for O(1) lookup.
    ///
    /// Call this when spawning any entity that has a Minecraft UUID
    /// (players, mobs, items, etc.). The mapping is removed
    /// automatically on [`despawn`](Self::despawn).
    pub fn index_uuid(&mut self, uuid: basalt_types::Uuid, entity: EntityId) {
        self.uuid_index.insert(uuid, entity);
    }

    /// Finds an entity by Minecraft UUID. O(1).
    pub fn find_by_uuid(&self, uuid: basalt_types::Uuid) -> Option<EntityId> {
        self.uuid_index.get(&uuid).copied()
    }
}

impl Default for Ecs {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{Health, Position, Velocity};

    #[test]
    fn spawn_returns_unique_ids() {
        let mut ecs = Ecs::new();
        let e1 = ecs.spawn();
        let e2 = ecs.spawn();
        let e3 = ecs.spawn();
        assert_ne!(e1, e2);
        assert_ne!(e2, e3);
        assert_eq!(ecs.entity_count(), 3);
    }

    #[test]
    fn despawn_removes_entity_and_components() {
        let mut ecs = Ecs::new();
        let e = ecs.spawn();
        ecs.set(
            e,
            Position {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
        );
        ecs.set(
            e,
            Health {
                current: 20.0,
                max: 20.0,
            },
        );

        ecs.despawn(e);
        assert!(!ecs.is_alive(e));
        assert!(ecs.get::<Position>(e).is_none());
        assert!(ecs.get::<Health>(e).is_none());
    }

    #[test]
    fn set_and_get_component() {
        let mut ecs = Ecs::new();
        let e = ecs.spawn();
        ecs.set(
            e,
            Position {
                x: 1.0,
                y: 64.0,
                z: -3.0,
            },
        );

        let pos = ecs.get::<Position>(e).unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 64.0);
        assert_eq!(pos.z, -3.0);
    }

    #[test]
    fn get_mut_modifies_component() {
        let mut ecs = Ecs::new();
        let e = ecs.spawn();
        ecs.set(
            e,
            Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        );

        let pos = ecs.get_mut::<Position>(e).unwrap();
        pos.x = 42.0;

        assert_eq!(ecs.get::<Position>(e).unwrap().x, 42.0);
    }

    #[test]
    fn has_component() {
        let mut ecs = Ecs::new();
        let e = ecs.spawn();
        assert!(!ecs.has::<Position>(e));

        ecs.set(
            e,
            Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        );
        assert!(ecs.has::<Position>(e));
    }

    #[test]
    fn remove_component() {
        let mut ecs = Ecs::new();
        let e = ecs.spawn();
        ecs.set(
            e,
            Health {
                current: 10.0,
                max: 20.0,
            },
        );

        let removed = ecs.remove_component::<Health>(e);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().current, 10.0);
        assert!(!ecs.has::<Health>(e));
    }

    #[test]
    fn iter_components() {
        let mut ecs = Ecs::new();
        let e1 = ecs.spawn();
        let e2 = ecs.spawn();
        ecs.set(
            e1,
            Velocity {
                dx: 1.0,
                dy: 0.0,
                dz: 0.0,
            },
        );
        ecs.set(
            e2,
            Velocity {
                dx: 0.0,
                dy: 1.0,
                dz: 0.0,
            },
        );

        let velocities: Vec<_> = ecs.iter::<Velocity>().collect();
        assert_eq!(velocities.len(), 2);
    }

    #[test]
    fn spawn_with_id() {
        let mut ecs = Ecs::new();
        ecs.spawn_with_id(42);
        assert!(ecs.is_alive(42));
        ecs.set(
            42,
            Position {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
        );
        assert_eq!(ecs.get::<Position>(42).unwrap().y, 64.0);
    }

    #[test]
    fn component_count() {
        let mut ecs = Ecs::new();
        let e1 = ecs.spawn();
        let e2 = ecs.spawn();
        ecs.set(
            e1,
            Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        );
        ecs.set(
            e2,
            Position {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        );
        assert_eq!(ecs.component_count::<Position>(), 2);
        assert_eq!(ecs.component_count::<Health>(), 0);
    }

    #[test]
    fn register_component_is_idempotent() {
        let mut ecs = Ecs::new();
        ecs.register_component::<Position>();
        ecs.register_component::<Position>();
        assert_eq!(ecs.component_count::<Position>(), 0);
    }

    #[test]
    fn get_nonexistent_entity_returns_none() {
        let ecs = Ecs::new();
        assert!(ecs.get::<Position>(999).is_none());
    }

    #[test]
    fn set_overwrites_existing() {
        let mut ecs = Ecs::new();
        let e = ecs.spawn();
        ecs.set(
            e,
            Health {
                current: 20.0,
                max: 20.0,
            },
        );
        ecs.set(
            e,
            Health {
                current: 5.0,
                max: 20.0,
            },
        );
        assert_eq!(ecs.get::<Health>(e).unwrap().current, 5.0);
    }

    #[test]
    fn find_by_uuid_returns_entity() {
        let mut ecs = Ecs::new();
        let uuid = basalt_types::Uuid::from_bytes([1; 16]);
        let e = ecs.spawn();
        ecs.index_uuid(uuid, e);
        assert_eq!(ecs.find_by_uuid(uuid), Some(e));
    }

    #[test]
    fn find_by_uuid_returns_none_for_unknown() {
        let ecs = Ecs::new();
        let uuid = basalt_types::Uuid::from_bytes([1; 16]);
        assert_eq!(ecs.find_by_uuid(uuid), None);
    }

    #[test]
    fn despawn_cleans_uuid_index() {
        let mut ecs = Ecs::new();
        let uuid = basalt_types::Uuid::from_bytes([1; 16]);
        let e = ecs.spawn();
        ecs.index_uuid(uuid, e);
        assert!(ecs.find_by_uuid(uuid).is_some());

        ecs.despawn(e);
        assert!(ecs.find_by_uuid(uuid).is_none());
    }
}
