# basalt-api

Public plugin API for the [Basalt](https://github.com/basalt-mc/basalt)
Minecraft server. This crate is the **single dependency** for all Basalt
plugins -- both built-in and external.

## Modules

| Module | Purpose |
|--------|---------|
| `prelude` | Essentials for every plugin (glob import this) |
| `components` | ECS component types: Position, Velocity, Inventory, ... |
| `system` | System registration: SystemContext, Phase, SystemBuilder |
| `command` | Command argument types: Arg, CommandArgs, Validation |
| `types` | Primitive Minecraft types: Uuid, Slot, TextComponent |
| `world` | Block states, collision, block entities |
| `events` | Domain event types: BlockBroken, PlayerMoved, ChatMessage, ... |

## Writing a plugin

Implement the `Plugin` trait and register event handlers, commands, or ECS
systems via the `PluginRegistrar`:

```rust,ignore
use basalt_api::prelude::*;

pub struct MyPlugin;

impl Plugin for MyPlugin {
    fn name(&self) -> &str {
        "my-plugin"
    }

    fn on_enable(&self, registrar: &mut PluginRegistrar) {
        registrar.on::<ChatMessageEvent>(Stage::Post, 0, |event, ctx| {
            let sender = ctx.player().username().to_string();
            println!("{sender}: {}", event.message);
        });
    }
}
```

## Event stages

The event bus dispatches handlers in three stages:

1. **Validate** -- read-only checks, can cancel (permissions, anti-cheat)
2. **Process** -- state mutation, one logical owner per event
3. **Post** -- side effects, no cancel (broadcasting, persistence)

## ECS systems

Register tick-based systems for physics, AI, or other per-tick logic:

```rust,ignore
use basalt_api::prelude::*;
use basalt_api::components::Position;
use basalt_api::system::{SystemContext, Phase};

registrar
    .system("my-system")
    .phase(Phase::Simulate)
    .run(|ctx| {
        // Access entities through SystemContext
    });
```

## Features

- `testing` -- enables `PluginTestHarness` for unit testing plugins
- `raw-packets` -- exposes wire-level packet definitions via `basalt_mc_protocol`

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](../../LICENSE)
for details.
