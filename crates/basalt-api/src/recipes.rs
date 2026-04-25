//! Plugin-facing wrapper around the recipe registry that dispatches
//! registry-lifecycle events.
//!
//! Plugins receive a [`RecipeRegistrar`] from
//! [`PluginRegistrar::recipes`](crate::PluginRegistrar::recipes) inside
//! `Plugin::on_enable`. Every mutation goes through the wrapper so the
//! 3 lifecycle events fire on the game bus:
//!
//! - [`RecipeRegisterEvent`](crate::events::RecipeRegisterEvent)
//!   (Validate, cancellable) — fires before each insert.
//! - [`RecipeRegisteredEvent`](crate::events::RecipeRegisteredEvent)
//!   (Post) — fires after a successful insert.
//! - [`RecipeUnregisteredEvent`](crate::events::RecipeUnregisteredEvent)
//!   (Post) — fires after each removal.
//!
//! The wrapper does **not** expose the underlying registry's vanilla
//! data through events: `RecipeRegistry::with_vanilla` runs before any
//! handler is registered, so retroactively dispatching 1557 events
//! would only spam handlers without serving a use case.

use crate::events::{Event, EventBus};

// Re-export the underlying recipe types so plugins can refer to them
// from `basalt_api::recipes` without depending on `basalt-recipes`
// directly.
pub use basalt_recipes::{
    OwnedShapedRecipe, OwnedShapelessRecipe, Recipe, RecipeId, RecipeRegistry,
};

use crate::context::ServerContext;
use crate::events::{RecipeRegisterEvent, RecipeRegisteredEvent, RecipeUnregisteredEvent};

/// Plugin-facing handle to the recipe registry with event dispatch.
///
/// Holds mutable references to the registry and the game event bus,
/// plus a shared reference to a stub dispatch context. Every mutation
/// method dispatches the appropriate lifecycle event and respects
/// Validate-stage cancellation.
pub struct RecipeRegistrar<'a> {
    registry: &'a mut RecipeRegistry,
    bus: &'a mut EventBus,
    ctx: &'a ServerContext,
}

impl<'a> RecipeRegistrar<'a> {
    /// Constructs a new registrar wrapper.
    ///
    /// Internal — called by [`PluginRegistrar::recipes`](crate::PluginRegistrar::recipes).
    pub(crate) fn new(
        registry: &'a mut RecipeRegistry,
        bus: &'a mut EventBus,
        ctx: &'a ServerContext,
    ) -> Self {
        Self { registry, bus, ctx }
    }

    /// Registers a shaped recipe.
    ///
    /// Dispatches [`RecipeRegisterEvent`] at Validate. If a handler
    /// cancels the event, the registry is left untouched and this
    /// method returns `false`. Otherwise the recipe is inserted and
    /// [`RecipeRegisteredEvent`] is dispatched at Post; returns `true`.
    pub fn add_shaped(&mut self, recipe: OwnedShapedRecipe) -> bool {
        let id = recipe.id.clone();
        let mut event = RecipeRegisterEvent {
            recipe: Recipe::Shaped(recipe),
            cancelled: false,
        };
        self.bus.dispatch(&mut event, self.ctx);
        if event.is_cancelled() {
            return false;
        }
        match event.recipe {
            Recipe::Shaped(r) => self.registry.add_shaped(r),
            Recipe::Shapeless(_) => {
                // Handlers must not change the recipe variant. Defensive
                // fallthrough preserves invariants without panicking.
                return false;
            }
        }
        let mut post = RecipeRegisteredEvent { recipe_id: id };
        self.bus.dispatch(&mut post, self.ctx);
        true
    }

    /// Registers a shapeless recipe.
    ///
    /// Same dispatch semantics as [`add_shaped`](Self::add_shaped).
    /// The caller is responsible for sorting `recipe.ingredients`
    /// ascending — required for correct matching.
    pub fn add_shapeless(&mut self, recipe: OwnedShapelessRecipe) -> bool {
        let id = recipe.id.clone();
        let mut event = RecipeRegisterEvent {
            recipe: Recipe::Shapeless(recipe),
            cancelled: false,
        };
        self.bus.dispatch(&mut event, self.ctx);
        if event.is_cancelled() {
            return false;
        }
        match event.recipe {
            Recipe::Shapeless(r) => self.registry.add_shapeless(r),
            Recipe::Shaped(_) => return false,
        }
        let mut post = RecipeRegisteredEvent { recipe_id: id };
        self.bus.dispatch(&mut post, self.ctx);
        true
    }

    /// Removes the recipe with the given id, dispatching
    /// [`RecipeUnregisteredEvent`] at Post on success.
    ///
    /// Returns `true` if a recipe was removed, `false` if the id was
    /// not registered.
    pub fn remove_by_id(&mut self, id: &RecipeId) -> bool {
        if self.registry.remove_by_id(id).is_some() {
            let mut event = RecipeUnregisteredEvent {
                recipe_id: id.clone(),
            };
            self.bus.dispatch(&mut event, self.ctx);
            true
        } else {
            false
        }
    }

    /// Removes every recipe (shaped and shapeless) producing the given
    /// `result_id`. Dispatches one [`RecipeUnregisteredEvent`] per
    /// removed entry. Returns the number of recipes removed.
    pub fn remove_by_result(&mut self, result_id: i32) -> usize {
        let removed = self.registry.remove_by_result(result_id);
        let count = removed.len();
        for recipe_id in removed {
            let mut event = RecipeUnregisteredEvent { recipe_id };
            self.bus.dispatch(&mut event, self.ctx);
        }
        count
    }

    /// Removes every recipe and dispatches one
    /// [`RecipeUnregisteredEvent`] per removed entry.
    pub fn clear(&mut self) {
        let removed = self.registry.clear();
        for recipe_id in removed {
            let mut event = RecipeUnregisteredEvent { recipe_id };
            self.bus.dispatch(&mut event, self.ctx);
        }
    }

    /// Returns a read-only view of the underlying registry.
    ///
    /// Useful for plugins that need to enumerate or query the registry
    /// without mutating it (e.g. count vanilla recipes, check
    /// existence of an id before registering).
    pub fn registry(&self) -> &RecipeRegistry {
        self.registry
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::events::Stage;

    use super::*;

    fn ctx() -> ServerContext {
        ServerContext::new(
            Arc::new(basalt_world::World::new_memory(42)),
            basalt_core::player::PlayerInfo::stub(),
        )
    }

    fn shaped(path: &str) -> OwnedShapedRecipe {
        OwnedShapedRecipe {
            id: RecipeId::new("plugin", path),
            width: 1,
            height: 1,
            pattern: vec![Some(1)],
            result_id: 42,
            result_count: 1,
        }
    }

    fn shapeless(path: &str) -> OwnedShapelessRecipe {
        OwnedShapelessRecipe {
            id: RecipeId::new("plugin", path),
            ingredients: vec![1, 2],
            result_id: 99,
            result_count: 1,
        }
    }

    #[test]
    fn add_shaped_dispatches_register_then_registered() {
        let mut registry = RecipeRegistry::empty();
        let mut bus = EventBus::new();
        let ctx = ctx();

        let validate_seen: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
        let post_seen = Arc::new(AtomicU32::new(0));

        {
            let v = Arc::clone(&validate_seen);
            bus.on::<RecipeRegisterEvent, ServerContext>(Stage::Validate, 0, move |_, _| {
                v.fetch_add(1, Ordering::Relaxed);
            });
        }
        {
            let p = Arc::clone(&post_seen);
            bus.on::<RecipeRegisteredEvent, ServerContext>(Stage::Post, 0, move |_, _| {
                p.fetch_add(1, Ordering::Relaxed);
            });
        }

        let mut registrar = RecipeRegistrar::new(&mut registry, &mut bus, &ctx);
        let inserted = registrar.add_shaped(shaped("magic_sword"));

        assert!(inserted);
        assert_eq!(validate_seen.load(Ordering::Relaxed), 1);
        assert_eq!(post_seen.load(Ordering::Relaxed), 1);
        assert_eq!(registry.shaped_count(), 1);
    }

    #[test]
    fn add_shaped_cancellation_skips_insert_and_post() {
        let mut registry = RecipeRegistry::empty();
        let mut bus = EventBus::new();
        let ctx = ctx();

        bus.on::<RecipeRegisterEvent, ServerContext>(Stage::Validate, 0, |event, _| {
            event.cancel();
        });

        let post_seen = Arc::new(AtomicU32::new(0));
        {
            let p = Arc::clone(&post_seen);
            bus.on::<RecipeRegisteredEvent, ServerContext>(Stage::Post, 0, move |_, _| {
                p.fetch_add(1, Ordering::Relaxed);
            });
        }

        let mut registrar = RecipeRegistrar::new(&mut registry, &mut bus, &ctx);
        let inserted = registrar.add_shaped(shaped("forbidden"));

        assert!(
            !inserted,
            "cancellation should make add_shaped return false"
        );
        assert_eq!(post_seen.load(Ordering::Relaxed), 0);
        assert_eq!(registry.shaped_count(), 0);
    }

    #[test]
    fn add_shapeless_round_trip() {
        let mut registry = RecipeRegistry::empty();
        let mut bus = EventBus::new();
        let ctx = ctx();

        let mut registrar = RecipeRegistrar::new(&mut registry, &mut bus, &ctx);
        assert!(registrar.add_shapeless(shapeless("bread")));
        assert!(
            registrar
                .registry()
                .contains(&RecipeId::new("plugin", "bread"))
        );
    }

    #[test]
    fn remove_by_id_dispatches_unregistered() {
        let mut registry = RecipeRegistry::empty();
        registry.add_shaped(shaped("temp"));

        let mut bus = EventBus::new();
        let ctx = ctx();
        let unreg_seen = Arc::new(AtomicU32::new(0));
        {
            let u = Arc::clone(&unreg_seen);
            bus.on::<RecipeUnregisteredEvent, ServerContext>(Stage::Post, 0, move |_, _| {
                u.fetch_add(1, Ordering::Relaxed);
            });
        }

        let mut registrar = RecipeRegistrar::new(&mut registry, &mut bus, &ctx);
        let id = RecipeId::new("plugin", "temp");
        assert!(registrar.remove_by_id(&id));
        assert_eq!(unreg_seen.load(Ordering::Relaxed), 1);
        assert!(!registry.contains(&id));
    }

    #[test]
    fn remove_by_id_missing_does_not_dispatch() {
        let mut registry = RecipeRegistry::empty();
        let mut bus = EventBus::new();
        let ctx = ctx();
        let unreg_seen = Arc::new(AtomicU32::new(0));
        {
            let u = Arc::clone(&unreg_seen);
            bus.on::<RecipeUnregisteredEvent, ServerContext>(Stage::Post, 0, move |_, _| {
                u.fetch_add(1, Ordering::Relaxed);
            });
        }

        let mut registrar = RecipeRegistrar::new(&mut registry, &mut bus, &ctx);
        assert!(!registrar.remove_by_id(&RecipeId::new("plugin", "missing")));
        assert_eq!(unreg_seen.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn remove_by_result_dispatches_per_removed() {
        let mut registry = RecipeRegistry::empty();
        registry.add_shaped(shaped("a"));
        registry.add_shaped(shaped("b"));
        // Both produce result_id 42 (shaped helper).

        let mut bus = EventBus::new();
        let ctx = ctx();
        let unreg_seen = Arc::new(AtomicU32::new(0));
        {
            let u = Arc::clone(&unreg_seen);
            bus.on::<RecipeUnregisteredEvent, ServerContext>(Stage::Post, 0, move |_, _| {
                u.fetch_add(1, Ordering::Relaxed);
            });
        }

        let mut registrar = RecipeRegistrar::new(&mut registry, &mut bus, &ctx);
        assert_eq!(registrar.remove_by_result(42), 2);
        assert_eq!(unreg_seen.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn clear_dispatches_per_recipe() {
        let mut registry = RecipeRegistry::empty();
        registry.add_shaped(shaped("a"));
        registry.add_shapeless(shapeless("b"));

        let mut bus = EventBus::new();
        let ctx = ctx();
        let unreg_seen = Arc::new(AtomicU32::new(0));
        {
            let u = Arc::clone(&unreg_seen);
            bus.on::<RecipeUnregisteredEvent, ServerContext>(Stage::Post, 0, move |_, _| {
                u.fetch_add(1, Ordering::Relaxed);
            });
        }

        let mut registrar = RecipeRegistrar::new(&mut registry, &mut bus, &ctx);
        registrar.clear();
        assert_eq!(unreg_seen.load(Ordering::Relaxed), 2);
        assert_eq!(registry.shaped_count(), 0);
        assert_eq!(registry.shapeless_count(), 0);
    }

    #[test]
    fn registry_view_exposes_underlying_state() {
        let mut registry = RecipeRegistry::empty();
        let mut bus = EventBus::new();
        let ctx = ctx();
        let mut registrar = RecipeRegistrar::new(&mut registry, &mut bus, &ctx);
        assert_eq!(registrar.registry().shaped_count(), 0);
        registrar.add_shaped(shaped("only"));
        assert_eq!(registrar.registry().shaped_count(), 1);
    }
}
