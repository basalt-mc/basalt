//! Plugin trait and registration API.
//!
//! Every server feature — built-in or external — implements the
//! [`Plugin`] trait. Plugins register event handlers and commands
//! during [`on_enable`](Plugin::on_enable).

use crate::command::{Arg, CommandArg, CommandArgs, Validation};
use crate::context::Context;
use crate::events::{BusKind, Event, EventBus, EventRouting, Stage};

/// A server plugin that registers event handlers and lifecycle hooks.
pub trait Plugin: Send + Sync + 'static {
    /// Returns the plugin's identity metadata.
    fn metadata(&self) -> PluginMetadata;

    /// Called when the plugin is enabled. Register event handlers
    /// and commands here.
    fn on_enable(&self, registrar: &mut PluginRegistrar);

    /// Called when the plugin is disabled (server shutdown).
    fn on_disable(&self) {}
}

/// Identity metadata for a plugin.
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    /// Human-readable plugin name.
    pub name: &'static str,
    /// Semver version string.
    pub version: &'static str,
    /// Optional author name.
    pub author: Option<&'static str>,
    /// Plugin names that must be loaded before this plugin.
    pub dependencies: &'static [&'static str],
}

/// Handler function type for commands with typed arguments.
pub type CommandHandler = Box<dyn Fn(&CommandArgs, &dyn Context) + Send + Sync>;

/// A registered command entry.
pub struct CommandEntry {
    /// Command name without the leading `/`.
    pub name: String,
    /// Short description for help listing.
    pub description: String,
    /// Single argument list (used when `variants` is empty).
    pub args: Vec<CommandArg>,
    /// Multiple argument variants for polymorphic commands.
    pub variants: Vec<Vec<CommandArg>>,
    /// The command handler function.
    pub handler: CommandHandler,
}

/// Plugin registration interface for events, commands, and systems.
///
/// Holds mutable references to both the network and game event buses.
/// Handler registration is routed automatically based on the event
/// type's [`EventRouting::BUS`] constant — plugins do not specify
/// which loop handles their events.
///
/// World and recipe fields are trait objects so that basalt-api does not
/// depend on concrete runtime types at the struct level. Call sites
/// coerce concrete types to the trait objects when constructing the
/// registrar.
pub struct PluginRegistrar<'a> {
    /// Event bus for the network loop (movement, chat, commands).
    instant_bus: &'a mut EventBus,
    /// Event bus for the game loop (blocks, world mutations).
    game_bus: &'a mut EventBus,
    /// Collected command entries.
    commands: &'a mut Vec<CommandEntry>,
    /// Collected system descriptors.
    systems: &'a mut Vec<crate::system::SystemDescriptor>,
    /// Shared world handle, available to all plugins.
    world: std::sync::Arc<dyn crate::world::handle::WorldHandle + Send + Sync>,
    /// Mutable recipe registry for plugin customisation.
    recipes: &'a mut dyn crate::recipes::RecipeRegistryHandle,
    /// Stub dispatch context for system-level events fired during
    /// plugin loading (e.g. recipe registry lifecycle). The context
    /// carries `PlayerInfo::stub()` — handlers must rely on the event
    /// payload, not `ctx.player()`.
    bootstrap_ctx: &'a dyn crate::context::Context,
}

impl<'a> PluginRegistrar<'a> {
    /// Creates a new registrar with dual event buses and recipe registry.
    ///
    /// `bootstrap_ctx` is a stub context used only to dispatch
    /// system-level events (today: the recipe registry lifecycle) that
    /// fire before any player exists.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instant_bus: &'a mut EventBus,
        game_bus: &'a mut EventBus,
        commands: &'a mut Vec<CommandEntry>,
        systems: &'a mut Vec<crate::system::SystemDescriptor>,
        world: std::sync::Arc<dyn crate::world::handle::WorldHandle + Send + Sync>,
        recipes: &'a mut dyn crate::recipes::RecipeRegistryHandle,
        bootstrap_ctx: &'a dyn crate::context::Context,
    ) -> Self {
        Self {
            instant_bus,
            game_bus,
            commands,
            systems,
            world,
            recipes,
            bootstrap_ctx,
        }
    }

    /// Returns a shared reference to the world.
    ///
    /// Available to all plugins — use this to capture the world
    /// in system closures for block access, collision checks, etc.
    pub fn world(&self) -> std::sync::Arc<dyn crate::world::handle::WorldHandle + Send + Sync> {
        std::sync::Arc::clone(&self.world)
    }

    /// Returns a [`RecipeRegistrar`](crate::recipes::RecipeRegistrar)
    /// that mutates the registry while dispatching the lifecycle
    /// events on the game bus.
    ///
    /// Plugins call this from [`on_enable`](Plugin::on_enable) to add
    /// or remove recipes. Mutations on the returned wrapper trigger
    /// [`RecipeRegisterEvent`](crate::events::RecipeRegisterEvent),
    /// [`RecipeRegisteredEvent`](crate::events::RecipeRegisteredEvent),
    /// and [`RecipeUnregisteredEvent`](crate::events::RecipeUnregisteredEvent)
    /// so other plugins can observe or veto changes.
    ///
    /// After every plugin's `on_enable` completes, the registry is
    /// frozen behind `Arc<RecipeRegistry>` and shared immutably with
    /// the game loop.
    pub fn recipes(&mut self) -> crate::recipes::RecipeRegistrar<'_> {
        crate::recipes::RecipeRegistrar::new(self.recipes, self.game_bus, self.bootstrap_ctx)
    }

    /// Registers an event handler on the correct bus.
    ///
    /// The target bus is determined at compile time by `E::BUS`:
    /// - [`BusKind::Instant`] → network loop bus
    /// - [`BusKind::Game`] → game loop bus
    pub fn on<E>(
        &mut self,
        stage: Stage,
        priority: i32,
        handler: impl Fn(&mut E, &dyn crate::context::Context) + Send + Sync + 'static,
    ) where
        E: Event + EventRouting + 'static,
    {
        match E::BUS {
            BusKind::Instant => self.instant_bus.on::<E>(stage, priority, handler),
            BusKind::Game => self.game_bus.on::<E>(stage, priority, handler),
        }
    }

    /// Starts building a system for the game loop.
    ///
    /// Returns a [`PluginSystemBuilder`] for fluent configuration of
    /// phase, frequency, component access, and the system runner.
    ///
    /// # Example
    ///
    /// ```ignore
    /// registrar.system("gravity")
    ///     .phase(Phase::Simulate)
    ///     .every(1)
    ///     .writes::<Position>()
    ///     .writes::<Velocity>()
    ///     .run(|ctx| { /* apply gravity */ });
    /// ```
    pub fn system(&mut self, name: &str) -> PluginSystemBuilder<'_, 'a> {
        PluginSystemBuilder {
            registrar: self,
            builder: crate::system::SystemBuilder::new(name),
        }
    }

    /// Starts building a command with typed arguments.
    pub fn command(&mut self, name: &str) -> CommandBuilder<'_, 'a> {
        CommandBuilder {
            registrar: self,
            name: name.to_string(),
            description: String::new(),
            args: Vec::new(),
            variants: Vec::new(),
        }
    }
}

/// Fluent builder for registering a system via a plugin.
///
/// Wraps [`SystemBuilder`](crate::system::SystemBuilder) and pushes the
/// resulting descriptor into the registrar's system list on `run()`.
pub struct PluginSystemBuilder<'r, 'a> {
    registrar: &'r mut PluginRegistrar<'a>,
    builder: crate::system::SystemBuilder,
}

impl<'r, 'a> PluginSystemBuilder<'r, 'a> {
    /// Sets which tick phase this system runs in.
    pub fn phase(mut self, phase: crate::components::Phase) -> Self {
        self.builder = self.builder.phase(phase);
        self
    }

    /// Sets the frequency divisor (runs when `tick % every == 0`).
    pub fn every(mut self, every: u64) -> Self {
        self.builder = self.builder.every(every);
        self
    }

    /// Declares that this system reads a component type.
    pub fn reads<T: crate::components::Component>(mut self) -> Self {
        self.builder = self.builder.reads::<T>();
        self
    }

    /// Declares that this system writes a component type.
    pub fn writes<T: crate::components::Component>(mut self) -> Self {
        self.builder = self.builder.writes::<T>();
        self
    }

    /// Sets the system runner and registers the system.
    pub fn run<F: FnMut(&mut dyn crate::system::SystemContext) + Send + 'static>(self, runner: F) {
        let descriptor = self.builder.run(runner);
        self.registrar.systems.push(descriptor);
    }
}

/// Fluent builder for registering a command with typed arguments.
pub struct CommandBuilder<'r, 'a> {
    registrar: &'r mut PluginRegistrar<'a>,
    name: String,
    description: String,
    args: Vec<CommandArg>,
    variants: Vec<Vec<CommandArg>>,
}

impl<'r, 'a> CommandBuilder<'r, 'a> {
    /// Sets the command description (shown in /help).
    pub fn description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    /// Adds a required argument with default validation.
    pub fn arg(mut self, name: &str, arg_type: Arg) -> Self {
        self.args.push(CommandArg {
            name: name.to_string(),
            arg_type,
            validation: Validation::Auto,
            required: true,
        });
        self
    }

    /// Adds a required argument with custom validation.
    pub fn arg_with(mut self, name: &str, arg_type: Arg, validation: Validation) -> Self {
        self.args.push(CommandArg {
            name: name.to_string(),
            arg_type,
            validation,
            required: true,
        });
        self
    }

    /// Adds an optional argument with default validation.
    pub fn optional_arg(mut self, name: &str, arg_type: Arg) -> Self {
        self.args.push(CommandArg {
            name: name.to_string(),
            arg_type,
            validation: Validation::Auto,
            required: false,
        });
        self
    }

    /// Adds a variant for polymorphic commands.
    ///
    /// Each variant is a separate argument list. The parser tries
    /// variants in order and uses the first one that succeeds.
    pub fn variant(mut self, build: impl FnOnce(VariantBuilder) -> VariantBuilder) -> Self {
        let builder = build(VariantBuilder { args: Vec::new() });
        self.variants.push(builder.args);
        self
    }

    /// Sets the handler and registers the command.
    pub fn handler(self, handler: impl Fn(&CommandArgs, &dyn Context) + Send + Sync + 'static) {
        self.registrar.commands.push(CommandEntry {
            name: self.name,
            description: self.description,
            args: self.args,
            variants: self.variants,
            handler: Box::new(handler),
        });
    }
}

/// Builder for a single variant of a polymorphic command.
pub struct VariantBuilder {
    args: Vec<CommandArg>,
}

impl VariantBuilder {
    /// Adds a required argument to this variant.
    pub fn arg(mut self, name: &str, arg_type: Arg) -> Self {
        self.args.push(CommandArg {
            name: name.to_string(),
            arg_type,
            validation: Validation::Auto,
            required: true,
        });
        self
    }

    /// Adds a required argument with custom validation.
    pub fn arg_with(mut self, name: &str, arg_type: Arg, validation: Validation) -> Self {
        self.args.push(CommandArg {
            name: name.to_string(),
            arg_type,
            validation,
            required: true,
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::NoopContext;

    struct TestPlugin;

    impl Plugin for TestPlugin {
        fn metadata(&self) -> PluginMetadata {
            PluginMetadata {
                name: "test",
                version: "0.1.0",
                author: Some("Test"),
                dependencies: &[],
            }
        }

        fn on_enable(&self, _registrar: &mut PluginRegistrar) {}
    }

    #[test]
    fn plugin_metadata() {
        let meta = TestPlugin.metadata();
        assert_eq!(meta.name, "test");
    }

    #[test]
    fn plugin_on_disable_default_is_noop() {
        TestPlugin.on_disable();
    }

    #[test]
    fn registrar_routes_to_correct_bus() {
        use crate::events::{BlockBrokenEvent, ChatMessageEvent};

        let mut instant_bus = EventBus::new();
        let mut game_bus = EventBus::new();
        let mut commands = Vec::new();
        let mut systems = Vec::new();
        let mut recipes = crate::testing::MockRecipeRegistry::new();
        let world = std::sync::Arc::new(crate::testing::MockWorld::flat())
            as std::sync::Arc<dyn crate::world::handle::WorldHandle + Send + Sync>;
        let ctx = NoopContext;
        {
            let mut registrar = PluginRegistrar::new(
                &mut instant_bus,
                &mut game_bus,
                &mut commands,
                &mut systems,
                std::sync::Arc::clone(&world)
                    as std::sync::Arc<dyn crate::world::handle::WorldHandle + Send + Sync>,
                &mut recipes as &mut dyn crate::recipes::RecipeRegistryHandle,
                &ctx as &dyn crate::context::Context,
            );
            registrar.on::<ChatMessageEvent>(Stage::Post, 0, |_event, _ctx| {});
            registrar.on::<BlockBrokenEvent>(Stage::Process, 0, |_event, _ctx| {});
        }
        assert_eq!(instant_bus.handler_count(), 1);
        assert_eq!(game_bus.handler_count(), 1);
    }

    #[test]
    fn command_builder_with_args() {
        let mut instant_bus = EventBus::new();
        let mut game_bus = EventBus::new();
        let mut commands = Vec::new();
        let mut systems = Vec::new();
        let mut recipes = crate::testing::MockRecipeRegistry::new();
        let world = std::sync::Arc::new(crate::testing::MockWorld::flat())
            as std::sync::Arc<dyn crate::world::handle::WorldHandle + Send + Sync>;
        let ctx = NoopContext;
        {
            let mut registrar = PluginRegistrar::new(
                &mut instant_bus,
                &mut game_bus,
                &mut commands,
                &mut systems,
                std::sync::Arc::clone(&world)
                    as std::sync::Arc<dyn crate::world::handle::WorldHandle + Send + Sync>,
                &mut recipes as &mut dyn crate::recipes::RecipeRegistryHandle,
                &ctx as &dyn crate::context::Context,
            );
            registrar
                .command("tp")
                .description("Teleport")
                .arg("x", Arg::Double)
                .arg("y", Arg::Double)
                .arg("z", Arg::Double)
                .handler(|_args, _ctx| {});
        }
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "tp");
        assert_eq!(commands[0].args.len(), 3);
        assert!(commands[0].variants.is_empty());
    }

    #[test]
    fn command_builder_with_variants() {
        let mut instant_bus = EventBus::new();
        let mut game_bus = EventBus::new();
        let mut commands = Vec::new();
        let mut systems = Vec::new();
        let mut recipes = crate::testing::MockRecipeRegistry::new();
        let world = std::sync::Arc::new(crate::testing::MockWorld::flat())
            as std::sync::Arc<dyn crate::world::handle::WorldHandle + Send + Sync>;
        let ctx = NoopContext;
        {
            let mut registrar = PluginRegistrar::new(
                &mut instant_bus,
                &mut game_bus,
                &mut commands,
                &mut systems,
                std::sync::Arc::clone(&world)
                    as std::sync::Arc<dyn crate::world::handle::WorldHandle + Send + Sync>,
                &mut recipes as &mut dyn crate::recipes::RecipeRegistryHandle,
                &ctx as &dyn crate::context::Context,
            );
            registrar
                .command("tp")
                .description("Teleport")
                .variant(|v| v.arg("destination", Arg::Player))
                .variant(|v| {
                    v.arg("x", Arg::Double)
                        .arg("y", Arg::Double)
                        .arg("z", Arg::Double)
                })
                .handler(|_args, _ctx| {});
        }
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].variants.len(), 2);
        assert_eq!(commands[0].variants[0].len(), 1); // player
        assert_eq!(commands[0].variants[1].len(), 3); // x y z
    }

    #[test]
    fn command_no_args() {
        let mut instant_bus = EventBus::new();
        let mut game_bus = EventBus::new();
        let mut commands = Vec::new();
        let mut systems = Vec::new();
        let mut recipes = crate::testing::MockRecipeRegistry::new();
        let world = std::sync::Arc::new(crate::testing::MockWorld::flat())
            as std::sync::Arc<dyn crate::world::handle::WorldHandle + Send + Sync>;
        let ctx = NoopContext;
        {
            let mut registrar = PluginRegistrar::new(
                &mut instant_bus,
                &mut game_bus,
                &mut commands,
                &mut systems,
                std::sync::Arc::clone(&world)
                    as std::sync::Arc<dyn crate::world::handle::WorldHandle + Send + Sync>,
                &mut recipes as &mut dyn crate::recipes::RecipeRegistryHandle,
                &ctx as &dyn crate::context::Context,
            );
            registrar
                .command("help")
                .description("Show help")
                .handler(|_args, _ctx| {});
        }
        assert_eq!(commands.len(), 1);
        assert!(commands[0].args.is_empty());
        assert!(commands[0].variants.is_empty());
    }

    #[test]
    fn recipes_accessor_exposes_registrar_with_dispatch() {
        use crate::events::RecipeRegisteredEvent;
        use crate::recipes::{OwnedShapedRecipe, RecipeId};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let mut instant_bus = EventBus::new();
        let mut game_bus = EventBus::new();
        let mut commands = Vec::new();
        let mut systems = Vec::new();
        let mut recipes = crate::testing::MockRecipeRegistry::new();
        let world = std::sync::Arc::new(crate::testing::MockWorld::flat())
            as std::sync::Arc<dyn crate::world::handle::WorldHandle + Send + Sync>;
        let ctx = NoopContext;

        let post_seen = Arc::new(AtomicU32::new(0));
        {
            let p = Arc::clone(&post_seen);
            game_bus.on::<RecipeRegisteredEvent>(Stage::Post, 0, move |_, _| {
                p.fetch_add(1, Ordering::Relaxed);
            });
        }

        {
            let mut registrar = PluginRegistrar::new(
                &mut instant_bus,
                &mut game_bus,
                &mut commands,
                &mut systems,
                std::sync::Arc::clone(&world)
                    as std::sync::Arc<dyn crate::world::handle::WorldHandle + Send + Sync>,
                &mut recipes as &mut dyn crate::recipes::RecipeRegistryHandle,
                &ctx as &dyn crate::context::Context,
            );
            let inserted = registrar.recipes().add_shaped(OwnedShapedRecipe {
                id: RecipeId::new("plugin", "demo"),
                width: 1,
                height: 1,
                pattern: vec![Some(1)],
                result_id: 7,
                result_count: 1,
            });
            assert!(inserted);
        }

        assert_eq!(post_seen.load(Ordering::Relaxed), 1);
        assert_eq!(recipes.shaped_count(), 1);
    }
}
