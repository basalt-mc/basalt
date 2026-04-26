//! Core ECS types: entity IDs, component storage, and the ECS world.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use basalt_api::budget::TickBudget;
pub use basalt_api::components::{Component, EntityId};

/// Type-erased component store, allowing the [`Ecs`] to hold
/// stores for different component types in a single HashMap.
pub(crate) trait AnyComponentStore: Send + Sync {
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

/// Creates a new empty typed component store for testing.
#[cfg(test)]
pub(crate) fn new_component_store<T: Component>() -> Box<dyn AnyComponentStore> {
    Box::new(ComponentStore::<T>::new())
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
    /// Stored as `Option` to allow zero-allocation extraction during
    /// parallel dispatch (individual systems are `take()`-n and put back).
    systems: Vec<Option<basalt_api::system::SystemDescriptor>>,
    /// World reference for SystemContext::world(). Set by the server at startup.
    world: Option<std::sync::Arc<basalt_world::World>>,
    /// Precomputed parallel execution groups for the SIMULATE phase.
    /// Built lazily on first tick, invalidated on system registration.
    simulate_cache: Option<crate::schedule::GroupCache>,
    /// Budget for the currently executing system (set before each runner call).
    current_budget: TickBudget,
    /// Per-system timing for the current tick (name, elapsed).
    tick_timings: Vec<(String, Duration)>,
    /// Expected tick duration for overrun detection. `None` disables logging.
    tick_duration: Option<Duration>,
}

impl Ecs {
    /// Creates an empty ECS world with no entities, components, or systems.
    pub fn new() -> Self {
        Self {
            stores: HashMap::new(),
            next_entity_id: AtomicU32::new(1),
            alive: Vec::new(),
            systems: Vec::new(),
            world: None,
            simulate_cache: None,
            current_budget: TickBudget::unlimited(),
            tick_timings: Vec::new(),
            tick_duration: None,
        }
    }

    /// Sets the expected tick duration for overrun detection.
    ///
    /// When set, `run_all` logs a warning if the total tick time exceeds
    /// this duration, including a per-system timing breakdown.
    pub fn set_tick_duration(&mut self, duration: Duration) {
        self.tick_duration = Some(duration);
    }

    /// Sets the world reference for system runners.
    ///
    /// Must be called before `run_all` if any system uses world methods
    /// (e.g. `ctx.get_block()`, `ctx.resolve_movement()`).
    pub fn set_world(&mut self, world: std::sync::Arc<basalt_world::World>) {
        self.world = Some(world);
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
    ///
    /// Invalidates the precomputed parallel group cache so it will
    /// be rebuilt on the next SIMULATE tick.
    pub fn add_system(&mut self, system: basalt_api::system::SystemDescriptor) {
        self.systems.push(Some(system));
        self.systems.sort_by_key(|s| s.as_ref().unwrap().phase);
        self.simulate_cache = None;
    }

    /// Runs all systems for the given tick and phase.
    ///
    /// Systems are temporarily extracted from `self` to avoid a
    /// double mutable borrow (`self.systems` + `&mut self` passed
    /// to each system runner). They are put back after execution.
    pub fn run_phase(&mut self, phase: basalt_api::components::Phase, tick: u64) {
        let track = self.tick_duration.is_some();
        let mut systems = std::mem::take(&mut self.systems);
        for slot in &mut systems {
            if let Some(system) = slot
                && system.phase == phase
                && tick.is_multiple_of(system.every)
            {
                let budget = match system.budget {
                    Some(limit) => TickBudget::new(limit),
                    None => TickBudget::unlimited(),
                };
                self.current_budget = budget;
                system.runner.run(self);
                if track {
                    self.tick_timings
                        .push((system.name.clone(), self.current_budget.elapsed()));
                }
            }
        }
        self.systems = systems;
    }

    /// Runs systems in parallel for a given phase using rayon.
    ///
    /// Uses a precomputed group cache (built once, O(1) lookup per tick).
    /// Systems within a group have no conflicting component access and
    /// are dispatched concurrently. A barrier separates groups.
    ///
    /// Falls back to sequential execution when there is a single group
    /// with a single system (zero overhead in that case).
    pub fn run_phase_parallel(&mut self, phase: basalt_api::components::Phase, tick: u64) {
        // Build cache lazily on first call (or after add_system invalidation)
        if self.simulate_cache.is_none() {
            self.simulate_cache = Some(crate::schedule::GroupCache::build(&self.systems, phase));
        }

        // Take cache out to release borrow on self (zero-alloc pointer swap)
        let cache = self.simulate_cache.take().unwrap();
        let groups = cache.groups_for_tick(tick);

        if groups.is_empty() {
            self.simulate_cache = Some(cache);
            return;
        }

        // Fast path: single group with single system — skip all parallel machinery
        if groups.len() == 1 && groups[0].len() == 1 {
            let idx = groups[0][0];
            self.simulate_cache = Some(cache);
            let mut systems = std::mem::take(&mut self.systems);
            systems[idx].as_mut().unwrap().runner.run(self);
            self.systems = systems;
            return;
        }

        // Clone group indices before returning cache (small: few Vecs of few usizes)
        let groups = groups.to_vec();
        self.simulate_cache = Some(cache);

        // Systems already stored as Vec<Option<_>> — zero-alloc extraction
        let mut systems = std::mem::take(&mut self.systems);

        for group in &groups {
            self.dispatch_group(group, &mut systems);
        }

        self.systems = systems;
    }

    /// Dispatches one parallel group of systems via rayon.
    ///
    /// For each system in the group:
    /// - Write-stores are temporarily removed from the Ecs (exclusive ownership)
    /// - Read-stores are shared via `&dyn` references
    /// - A [`ParallelSystemContext`] is built per system
    ///
    /// After the group completes, write-stores are returned and deferred
    /// spawn/despawn commands are applied.
    fn dispatch_group(
        &mut self,
        group: &[usize],
        system_slots: &mut [Option<basalt_api::system::SystemDescriptor>],
    ) {
        // Collect all TypeIds written by systems in this group.
        // Each write-TypeId belongs to exactly one system (guaranteed by conflict-free grouping).
        let mut write_owners: HashMap<TypeId, usize> = HashMap::new();
        for &idx in group {
            let access = &system_slots[idx].as_ref().unwrap().access;
            for &tid in &access.writes {
                write_owners.insert(tid, idx);
            }
        }

        // Extract write-stores from Ecs — each goes to its owning system
        let mut per_system_writes: HashMap<usize, HashMap<TypeId, Box<dyn AnyComponentStore>>> =
            group.iter().map(|&idx| (idx, HashMap::new())).collect();
        for (&tid, &owner_idx) in &write_owners {
            if let Some(store) = self.stores.remove(&tid) {
                per_system_writes
                    .get_mut(&owner_idx)
                    .unwrap()
                    .insert(tid, store);
            }
        }

        // Build read-store references per system from the remaining stores in self.
        // All write-stores have been removed, so what remains is safe to share as &.
        let mut per_system_reads: HashMap<usize, HashMap<TypeId, &dyn AnyComponentStore>> =
            group.iter().map(|&idx| (idx, HashMap::new())).collect();
        for &idx in group {
            let access = &system_slots[idx].as_ref().unwrap().access;
            for &tid in &access.reads {
                // Skip if this system already has it as a write-store
                if per_system_writes
                    .get(&idx)
                    .is_some_and(|ws| ws.contains_key(&tid))
                {
                    continue;
                }
                if let Some(store) = self.stores.get(&tid) {
                    per_system_reads
                        .get_mut(&idx)
                        .unwrap()
                        .insert(tid, &**store);
                }
            }
        }

        let world_ref = self
            .world
            .as_ref()
            .expect("Ecs::set_world() must be called before running parallel systems");

        // Snapshot alive list and entity counter for parallel contexts.
        // Clone alive so rayon tasks don't borrow self (which we mutate after the scope).
        let local_counter = AtomicU32::new(self.next_entity_id.load(Ordering::Relaxed));

        // Take systems out for the group
        let mut group_systems: Vec<(usize, basalt_api::system::SystemDescriptor)> = group
            .iter()
            .map(|&idx| (idx, system_slots[idx].take().unwrap()))
            .collect();

        // Collect results from parallel execution via a shared mutex.
        // Each entry: (index, descriptor, write_stores, deferred_cmds, name, elapsed)
        type GroupResult = (
            usize,
            basalt_api::system::SystemDescriptor,
            HashMap<TypeId, Box<dyn AnyComponentStore>>,
            Vec<crate::parallel::DeferredCommand>,
            String,
            Duration,
        );
        let results: std::sync::Mutex<Vec<GroupResult>> = std::sync::Mutex::new(Vec::new());

        rayon::scope(|s| {
            for (idx, mut sys) in group_systems.drain(..) {
                let write_stores = per_system_writes.remove(&idx).unwrap_or_default();
                let read_stores = per_system_reads.remove(&idx).unwrap_or_default();
                let sys_name = sys.name.clone();
                let results = &results;
                let counter = &local_counter;

                let sys_budget = match sys.budget {
                    Some(limit) => TickBudget::new(limit),
                    None => TickBudget::unlimited(),
                };

                s.spawn(move |_| {
                    let mut ctx = crate::parallel::ParallelSystemContext::new(
                        write_stores,
                        read_stores,
                        world_ref,
                        counter,
                        sys_name,
                        sys_budget,
                    );
                    let run_start = Instant::now();
                    sys.runner.run(&mut ctx);
                    let elapsed = run_start.elapsed();
                    let name = ctx.system_name().to_string();
                    let (writes_back, deferred) = ctx.into_parts();
                    results
                        .lock()
                        .unwrap()
                        .push((idx, sys, writes_back, deferred, name, elapsed));
                });
            }
        });
        // rayon::scope blocks — all tasks are complete, all borrows released.

        // Sync the entity counter back
        self.next_entity_id
            .store(local_counter.load(Ordering::Relaxed), Ordering::Relaxed);

        // Process results: return systems, stores, apply deferred commands, collect timings
        for (idx, sys, writes_back, deferred, name, elapsed) in results.into_inner().unwrap() {
            self.tick_timings.push((name, elapsed));
            system_slots[idx] = Some(sys);
            for (tid, store) in writes_back {
                self.stores.insert(tid, store);
            }
            for cmd in deferred {
                match cmd {
                    crate::parallel::DeferredCommand::Spawn { entity_id } => {
                        self.alive.push(entity_id);
                    }
                    crate::parallel::DeferredCommand::Despawn { entity_id } => {
                        Ecs::despawn(self, entity_id);
                    }
                }
            }
        }
    }

    /// Runs all phases in order for the given tick.
    ///
    /// The SIMULATE phase uses parallel dispatch via rayon when multiple
    /// non-conflicting systems exist. All other phases run sequentially.
    pub fn run_all(&mut self, tick: u64) {
        use basalt_api::components::Phase;
        // Only start timing when overrun detection is enabled (avoids ~25ns Instant::now cost)
        let tick_start = self.tick_duration.map(|_| Instant::now());
        if tick_start.is_some() {
            self.tick_timings.clear();
        }

        self.run_phase(Phase::Input, tick);
        self.run_phase(Phase::Validate, tick);
        self.run_phase_parallel(Phase::Simulate, tick);
        self.run_phase(Phase::Process, tick);
        self.run_phase(Phase::Output, tick);
        self.run_phase(Phase::Post, tick);

        if let Some(start) = tick_start
            && let Some(limit) = self.tick_duration
        {
            let total = start.elapsed();
            if total > limit {
                log::warn!(
                    "Tick {tick} overrun: {total:.1?} > {limit:.1?} — {}",
                    self.format_timings()
                );
            }
        }
    }

    /// Returns the number of registered systems.
    pub fn system_count(&self) -> usize {
        self.systems.len()
    }

    /// Returns per-system timings collected during the last `run_all` call.
    pub fn tick_timings(&self) -> &[(String, Duration)] {
        &self.tick_timings
    }

    /// Formats the per-system timing breakdown for logging.
    fn format_timings(&self) -> String {
        self.tick_timings
            .iter()
            .map(|(name, elapsed)| format!("{name}={elapsed:.1?}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Default for Ecs {
    fn default() -> Self {
        Self::new()
    }
}

impl Ecs {
    /// Returns a reference to the internal world.
    ///
    /// Used by the `SystemContext` implementation to delegate typed
    /// world methods. Panics if `set_world()` was not called.
    fn world_ref(&self) -> &basalt_world::World {
        self.world
            .as_ref()
            .expect("Ecs::set_world() must be called before running systems")
    }
}

impl basalt_api::world::handle::WorldHandle for Ecs {
    fn get_block(&self, x: i32, y: i32, z: i32) -> u16 {
        self.world_ref().get_block(x, y, z)
    }

    fn set_block(&self, x: i32, y: i32, z: i32, state: u16) {
        self.world_ref().set_block(x, y, z, state);
    }

    fn get_block_entity(
        &self,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<basalt_world::block_entity::BlockEntity> {
        self.world_ref()
            .get_block_entity(x, y, z)
            .map(|r| r.clone())
    }

    fn set_block_entity(
        &self,
        x: i32,
        y: i32,
        z: i32,
        entity: basalt_world::block_entity::BlockEntity,
    ) {
        self.world_ref().set_block_entity(x, y, z, entity);
    }

    fn mark_chunk_dirty(&self, cx: i32, cz: i32) {
        self.world_ref().mark_chunk_dirty(cx, cz);
    }

    fn persist_chunk(&self, cx: i32, cz: i32) {
        self.world_ref().persist_chunk(cx, cz);
    }

    fn dirty_chunks(&self) -> Vec<(i32, i32)> {
        self.world_ref().dirty_chunks()
    }

    fn check_overlap(&self, aabb: &basalt_api::world::collision::Aabb) -> bool {
        basalt_api::world::collision::check_overlap(self.world_ref(), aabb)
    }

    fn ray_cast(
        &self,
        origin: (f64, f64, f64),
        direction: (f64, f64, f64),
        max_distance: f64,
    ) -> Option<basalt_api::world::collision::RayHit> {
        basalt_api::world::collision::ray_cast(self.world_ref(), origin, direction, max_distance)
    }

    fn resolve_movement(
        &self,
        aabb: &basalt_api::world::collision::Aabb,
        dx: f64,
        dy: f64,
        dz: f64,
    ) -> (f64, f64, f64) {
        basalt_api::world::collision::resolve_movement(self.world_ref(), aabb, dx, dy, dz)
    }
}

impl basalt_api::system::SystemContext for Ecs {
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

    fn budget(&self) -> &TickBudget {
        &self.current_budget
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_api::components::{Health, Position, Velocity};

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

    // -- Parallel dispatch tests --

    fn setup_parallel_ecs() -> Ecs {
        let mut ecs = Ecs::new();
        ecs.register_component::<Position>();
        ecs.register_component::<Velocity>();
        ecs.register_component::<Health>();
        let world = std::sync::Arc::new(basalt_world::World::new_memory(42));
        ecs.set_world(world);
        ecs
    }

    #[test]
    fn parallel_two_non_conflicting_systems() {
        let mut ecs = setup_parallel_ecs();
        let e = ecs.spawn();
        ecs.set(
            e,
            Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        );
        ecs.set(
            e,
            Health {
                current: 20.0,
                max: 20.0,
            },
        );

        // System A writes Position (sets x to 42)
        ecs.add_system(
            basalt_api::system::SystemBuilder::new("move_x")
                .phase(basalt_api::components::Phase::Simulate)
                .writes::<Position>()
                .run(|ctx: &mut dyn basalt_api::system::SystemContext| {
                    use basalt_api::system::SystemContextExt;
                    for id in ctx.query::<Position>() {
                        if let Some(pos) = ctx.get_mut::<Position>(id) {
                            pos.x = 42.0;
                        }
                    }
                }),
        );

        // System B writes Health (sets current to 10) — no conflict with A
        ecs.add_system(
            basalt_api::system::SystemBuilder::new("damage")
                .phase(basalt_api::components::Phase::Simulate)
                .writes::<Health>()
                .run(|ctx: &mut dyn basalt_api::system::SystemContext| {
                    use basalt_api::system::SystemContextExt;
                    for id in ctx.query::<Health>() {
                        if let Some(hp) = ctx.get_mut::<Health>(id) {
                            hp.current = 10.0;
                        }
                    }
                }),
        );

        ecs.run_phase_parallel(basalt_api::components::Phase::Simulate, 1);

        assert_eq!(ecs.get::<Position>(e).unwrap().x, 42.0);
        assert_eq!(ecs.get::<Health>(e).unwrap().current, 10.0);
    }

    #[test]
    fn parallel_conflicting_systems_run_sequentially() {
        let mut ecs = setup_parallel_ecs();
        let e = ecs.spawn();
        ecs.set(
            e,
            Velocity {
                dx: 0.0,
                dy: 0.0,
                dz: 0.0,
            },
        );

        // Both write Velocity — they must be in separate groups
        ecs.add_system(
            basalt_api::system::SystemBuilder::new("sys_a")
                .phase(basalt_api::components::Phase::Simulate)
                .writes::<Velocity>()
                .run(|ctx: &mut dyn basalt_api::system::SystemContext| {
                    use basalt_api::system::SystemContextExt;
                    for id in ctx.query::<Velocity>() {
                        if let Some(vel) = ctx.get_mut::<Velocity>(id) {
                            vel.dx += 1.0;
                        }
                    }
                }),
        );

        ecs.add_system(
            basalt_api::system::SystemBuilder::new("sys_b")
                .phase(basalt_api::components::Phase::Simulate)
                .writes::<Velocity>()
                .run(|ctx: &mut dyn basalt_api::system::SystemContext| {
                    use basalt_api::system::SystemContextExt;
                    for id in ctx.query::<Velocity>() {
                        if let Some(vel) = ctx.get_mut::<Velocity>(id) {
                            vel.dx += 2.0;
                        }
                    }
                }),
        );

        ecs.run_phase_parallel(basalt_api::components::Phase::Simulate, 1);

        // Both ran (sequentially in separate groups), total dx = 3.0
        assert_eq!(ecs.get::<Velocity>(e).unwrap().dx, 3.0);
    }

    #[test]
    fn parallel_single_system_uses_fast_path() {
        let mut ecs = setup_parallel_ecs();
        let e = ecs.spawn();
        ecs.set(
            e,
            Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        );

        ecs.add_system(
            basalt_api::system::SystemBuilder::new("only_one")
                .phase(basalt_api::components::Phase::Simulate)
                .writes::<Position>()
                .run(|ctx: &mut dyn basalt_api::system::SystemContext| {
                    use basalt_api::system::SystemContextExt;
                    for id in ctx.query::<Position>() {
                        if let Some(pos) = ctx.get_mut::<Position>(id) {
                            pos.y = 100.0;
                        }
                    }
                }),
        );

        ecs.run_phase_parallel(basalt_api::components::Phase::Simulate, 1);
        assert_eq!(ecs.get::<Position>(e).unwrap().y, 100.0);
    }

    #[test]
    fn parallel_deferred_spawn_applied_after_group() {
        let mut ecs = setup_parallel_ecs();
        let initial_count = ecs.entity_count();

        ecs.add_system(
            basalt_api::system::SystemBuilder::new("spawner_a")
                .phase(basalt_api::components::Phase::Simulate)
                .writes::<Position>()
                .run(|ctx: &mut dyn basalt_api::system::SystemContext| {
                    ctx.spawn();
                }),
        );

        ecs.add_system(
            basalt_api::system::SystemBuilder::new("spawner_b")
                .phase(basalt_api::components::Phase::Simulate)
                .writes::<Health>()
                .run(|ctx: &mut dyn basalt_api::system::SystemContext| {
                    ctx.spawn();
                }),
        );

        ecs.run_phase_parallel(basalt_api::components::Phase::Simulate, 1);

        // Both systems spawned an entity — they appear after the group
        assert_eq!(ecs.entity_count(), initial_count + 2);
    }

    #[test]
    fn parallel_run_all_integrates_correctly() {
        let mut ecs = setup_parallel_ecs();
        let e = ecs.spawn();
        ecs.set(
            e,
            Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        );
        ecs.set(
            e,
            Health {
                current: 20.0,
                max: 20.0,
            },
        );

        ecs.add_system(
            basalt_api::system::SystemBuilder::new("move")
                .phase(basalt_api::components::Phase::Simulate)
                .writes::<Position>()
                .run(|ctx: &mut dyn basalt_api::system::SystemContext| {
                    use basalt_api::system::SystemContextExt;
                    for id in ctx.query::<Position>() {
                        if let Some(pos) = ctx.get_mut::<Position>(id) {
                            pos.x += 1.0;
                        }
                    }
                }),
        );

        ecs.add_system(
            basalt_api::system::SystemBuilder::new("heal")
                .phase(basalt_api::components::Phase::Simulate)
                .writes::<Health>()
                .run(|ctx: &mut dyn basalt_api::system::SystemContext| {
                    use basalt_api::system::SystemContextExt;
                    for id in ctx.query::<Health>() {
                        if let Some(hp) = ctx.get_mut::<Health>(id) {
                            hp.current -= 1.0;
                        }
                    }
                }),
        );

        // run_all calls run_phase_parallel for Simulate
        ecs.run_all(1);

        assert_eq!(ecs.get::<Position>(e).unwrap().x, 1.0);
        assert_eq!(ecs.get::<Health>(e).unwrap().current, 19.0);
    }

    #[test]
    fn parallel_system_reads_while_other_writes() {
        let mut ecs = setup_parallel_ecs();
        let e = ecs.spawn();
        ecs.set(
            e,
            Position {
                x: 5.0,
                y: 0.0,
                z: 0.0,
            },
        );
        ecs.set(
            e,
            Health {
                current: 20.0,
                max: 20.0,
            },
        );

        // System A reads Position, writes Health — uses Position.x to set Health
        ecs.add_system(
            basalt_api::system::SystemBuilder::new("pos_to_hp")
                .phase(basalt_api::components::Phase::Simulate)
                .reads::<Position>()
                .writes::<Health>()
                .run(|ctx: &mut dyn basalt_api::system::SystemContext| {
                    use basalt_api::system::SystemContextExt;
                    for id in ctx.query::<Health>() {
                        let x = ctx.get::<Position>(id).map_or(0.0, |p| p.x);
                        if let Some(hp) = ctx.get_mut::<Health>(id) {
                            hp.current = x as f32;
                        }
                    }
                }),
        );

        ecs.run_phase_parallel(basalt_api::components::Phase::Simulate, 1);

        assert_eq!(ecs.get::<Health>(e).unwrap().current, 5.0);
    }
}
