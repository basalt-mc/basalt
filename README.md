# Basalt

A Minecraft server written from scratch in Rust. No wrappers, no JVM, no legacy code. A clean, modern foundation for building custom Minecraft experiences.

[![Version](https://img.shields.io/github/v/release/basalt-mc/basalt?label=version&color=orange)](https://github.com/basalt-mc/basalt/releases)
[![CI](https://github.com/basalt-mc/basalt/actions/workflows/ci.yml/badge.svg)](https://github.com/basalt-mc/basalt/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/badge/coverage-%3E90%25-brightgreen)](https://github.com/basalt-mc/basalt)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

---

## Why Basalt?

**Lightweight.** No JVM, no garbage collector. Basalt starts in under a second and runs on a fraction of the memory a Java server needs. Your hardware goes to the game, not the runtime.

**Modular.** Every feature is a plugin: chat, commands, block interactions, chunk streaming, persistence. Disable what you don't need, zero overhead. An auth-only server loads three plugins. A full survival server loads them all. Same binary, different config.

**Reliable.** Rust's type system catches entire categories of bugs at compile time. No null pointer crashes at 3 AM. No memory leaks after a week of uptime. No GC pauses during peak hours.

**Hackable.** The plugin API is the same for built-in and external plugins. There's no backdoor. Register event handlers, declare typed commands with tab-completion, access the world. If a built-in plugin does it, your plugin can too.

**Tested.** 1100+ tests, property-based fuzzing on protocol decoders, 90%+ code coverage enforced in CI. The protocol layer is generated from [PrismarineJS/minecraft-data](https://github.com/PrismarineJS/minecraft-data), so packet definitions stay in sync with the real protocol.

---

## Status

Basalt is in **active early development**. Production-ready for hobbyist servers and plugin experimentation; not yet for high-traffic public servers.

**What you can do today:**

- Connect a vanilla Minecraft 1.21.4 client
- Explore procedurally generated terrain (hills, water, beaches)
- Break and place blocks — the world persists to disk
- Chat with other players in real time
- Use built-in commands: `/tp`, `/gamemode`, `/say`, `/help`, `/kick`, `/list`, `/stop`
- Open chests (single + double), drop items into the world
- Craft items at a crafting table (full recipe matching engine)
- Enable or disable any feature via plugin config — disabled = zero cost

**What's coming next:**

- Mobs, projectiles, other entities
- Combat (damage, fall damage, PvP)
- Online mode (real Mojang authentication)
- Multi-version support (1.21.0 – 1.21.11)
- Redstone and block updates

---

## Roadmap

**Multi-Version Protocol Support** — One server, many client versions. Each connection picks a protocol adapter at handshake time that translates packet IDs and registry data; game logic stays version-agnostic. Players on 1.21.0 through 1.21.11 share the same world.

**Entity simulation** — Mobs, projectiles, dropped items with AI running on parallel ticks. The game loop simulates the world; players never wait on simulation lag.

**Online mode** — Real Mojang authentication and end-to-end encryption.

---

## Quick Start

**Run a server**

```bash
git clone https://github.com/basalt-mc/basalt.git
cd basalt
cargo run --release --package basalt-server --example server
```

Connect with a Minecraft 1.21.4 client to `localhost:25565`.

**Develop a plugin**

```bash
cargo new --lib my-plugin
cd my-plugin
cargo add basalt-api
```

Implement the `Plugin` trait, register event handlers and commands, link into a server. Full example: [basalt-example-plugin](https://github.com/basalt-mc/basalt-example-plugin).

Requirements: Rust 1.85+.

---

## Plugin Development

Basalt's plugin API ships as a single crate on crates.io: [`basalt-api`](https://crates.io/crates/basalt-api). External plugins add it as a dependency and get the same surface as built-in plugins — there's no privileged backdoor.

```rust
use basalt_api::prelude::*;

pub struct GreeterPlugin;

impl Plugin for GreeterPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "greeter",
            version: "0.1.0",
            author: Some("you"),
            dependencies: &[],
        }
    }

    fn on_enable(&self, registrar: &mut PluginRegistrar) {
        registrar.on::<ChatMessageEvent>(Stage::Process, 0, |event, ctx| {
            if event.message.starts_with("!hello") {
                ctx.chat().broadcast(&format!("Hi {}!", ctx.player().username()));
            }
        });
    }
}
```

Full API docs: [docs.rs/basalt-api](https://docs.rs/basalt-api). The plugin lifecycle (`Validate` → `Process` → `Post`), typed command args with tab-completion, and handle-based world access are all documented there.

---

## Under the Hood

For the technically curious:

- **Zero-copy protocol**: `Encode`/`Decode` traits work on raw byte slices. `EncodedSize` enables exact buffer pre-allocation. No serde, no intermediate representations.
- **Concurrent world access**: Chunks are stored in a `DashMap` with per-shard locking. Players streaming different chunks never block each other.
- **Code-generated packets**: The `xtask` pipeline reads PrismarineJS/minecraft-data JSON and generates Rust structs with derive macros for every packet. 180+ Play-state packets, zero hand-written boilerplate.
- **In-house NBT**: Custom implementation tuned for the protocol subset. No external crate dependency.
- **Fuzz-tested decoders**: libfuzzer targets for every protocol type that accepts untrusted input. Already caught a real OOM bug in NBT list decoding.

---

## License

[Apache-2.0](LICENSE)
