//! Core ECS types: entity IDs, component storage, and the ECS world.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

pub use basalt_core::{Component, EntityId};

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
    /// Returns a component as `&dyn Any` by entity ID.
    fn get_any(&self, entity: EntityId) -> Option<&dyn Any>;
    /// Returns a component as `&mut dyn Any` by entity ID.
    fn get_any_mut(&mut self, entity: EntityId) -> Option<&mut dyn Any>;
    /// Returns all entity IDs in this store.
    fn entity_ids(&self) -> Vec<EntityId>;
    /// Inserts a type-erased component for an entity.
    fn set_any(&mut self, entity: EntityId, value: Box<dyn Any + Send + Sync>);
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

    fn get_any(&self, entity: EntityId) -> Option<&dyn Any> {
        self.data.get(&entity).map(|v| v as &dyn Any)
    }

    fn get_any_mut(&mut self, entity: EntityId) -> Option<&mut dyn Any> {
        self.data.get_mut(&entity).map(|v| v as &mut dyn Any)
    }

    fn entity_ids(&self) -> Vec<EntityId> {
        self.data.keys().copied().collect()
    }

    fn set_any(&mut self, entity: EntityId, value: Box<dyn Any + Send + Sync>) {
        if let Ok(typed) = value.downcast::<T>() {
            self.data.insert(entity, *typed);
        }
    }
}

/// Type-erased component store for dynamic component registration.
///
/// Used when components are set via `SystemContext::set_component`
/// without prior `register_component` call. Stores values as
/// `Box<dyn Any + Send + Sync>` instead of typed `T`.
struct ErasedComponentStore {
    data: HashMap<EntityId, Box<dyn Any + Send + Sync>>,
}

impl ErasedComponentStore {
    fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }
}

impl AnyComponentStore for ErasedComponentStore {
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
    fn get_any(&self, entity: EntityId) -> Option<&dyn Any> {
        self.data.get(&entity).map(|v| &**v as &dyn Any)
    }
    fn get_any_mut(&mut self, entity: EntityId) -> Option<&mut dyn Any> {
        self.data.get_mut(&entity).map(|v| &mut **v as &mut dyn Any)
    }
    fn entity_ids(&self) -> Vec<EntityId> {
        self.data.keys().copied().collect()
    }
    fn set_any(&mut self, entity: EntityId, value: Box<dyn Any + Send + Sync>) {
        self.data.insert(entity, value);
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
}

impl Ecs {
    /// Creates an empty ECS world with no entities, components, or systems.
    pub fn new() -> Self {
        Self {
            stores: HashMap::new(),
            next_entity_id: AtomicU32::new(1),
            alive: Vec::new(),
            systems: Vec::new(),
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

    /// Despawns an entity, removing all its components.
    pub fn despawn(&mut self, entity: EntityId) {
        self.alive.retain(|&e| e != entity);
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
    pub fn run_phase(&mut self, phase: basalt_core::Phase, tick: u64) {
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
        use basalt_core::Phase;
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
}

impl Default for Ecs {
    fn default() -> Self {
        Self::new()
    }
}

impl basalt_core::SystemContext for Ecs {
    fn world(&self) -> &basalt_world::World {
        unimplemented!("raw Ecs does not own a World — use the server's SystemContext wrapper")
    }

    fn spawn(&mut self) -> EntityId {
        Ecs::spawn(self)
    }

    fn despawn(&mut self, entity: EntityId) {
        Ecs::despawn(self, entity);
    }

    fn set_component(
        &mut self,
        entity: EntityId,
        type_id: TypeId,
        component: Box<dyn Any + Send + Sync>,
    ) {
        if let Some(store) = self.stores.get_mut(&type_id) {
            store.set_any(entity, component);
        } else {
            let mut store = ErasedComponentStore::new();
            store.set_any(entity, component);
            self.stores.insert(type_id, Box::new(store));
        }
    }

    fn entities_with(&self, type_id: TypeId) -> Vec<EntityId> {
        self.stores
            .get(&type_id)
            .map(|store| store.entity_ids())
            .unwrap_or_default()
    }

    fn get_component(&self, entity: EntityId, type_id: TypeId) -> Option<&dyn Any> {
        self.stores.get(&type_id)?.get_any(entity)
    }

    fn get_component_mut(&mut self, entity: EntityId, type_id: TypeId) -> Option<&mut dyn Any> {
        self.stores.get_mut(&type_id)?.get_any_mut(entity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_core::{Health, Position, Velocity};

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
}
