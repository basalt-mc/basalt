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

**Tested.** 600+ tests, property-based fuzzing on protocol decoders, 90%+ code coverage enforced in CI. The protocol layer is generated from [PrismarineJS/minecraft-data](https://github.com/PrismarineJS/minecraft-data), so packet definitions stay in sync with the real protocol.

---

## Status

Basalt is in **active early development**. It's not ready for production, but it works.

**What works today:**

- Full protocol flow: handshake, login, configuration, play (Minecraft 1.21.4)
- World generation with Perlin noise terrain (hills, water, beaches)
- Block breaking and placing with persistence to disk
- Chat, commands (`/tp`, `/gamemode`, `/say`, `/help`, `/kick`, `/list`, `/stop`)
- Multi-player: player join/leave, movement broadcast, skin loading
- Chunk streaming on movement with LRU cache and configurable memory limit
- Plugin system with three-stage event bus (Validate, Process, Post)

**What's missing:**

- Entities (mobs, items, projectiles)
- Combat and survival mechanics
- Inventory and crafting
- Redstone and block updates
- Multi-version support (only 1.21.4 for now)
- Authentication (offline mode only)

---

## Roadmap

Two major architecture pieces are designed and ready to implement:

### Server Runtime Architecture

Transform Basalt from a reactive server (packet in, response out) into a tick-based game server with active simulation. A dedicated **network loop** handles player-facing responsiveness (movement, chat, commands) while a separate **game loop** runs physics, AI, and world simulation at 20 TPS. Heavy systems (pathfinding, AI evaluation) are parallelized on a thread pool. Even under heavy simulation load, players can still move and chat without lag.

### Multi-Version Protocol Support

Accept clients from 1.21.0 through 1.21.11 simultaneously. Each connection gets a protocol adapter selected at handshake time that translates packet IDs, block states, and registry data. The game logic runs one version; only the wire format adapts. Built on a code-generation pipeline that produces per-version packet definitions, ID mappings, and block state tables from minecraft-data.

---

## Quick Start

**Requirements:** Rust 1.85+ (edition 2024), Git.

```bash
# Clone
git clone https://github.com/basalt-mc/basalt.git
cd basalt

# Build
cargo build --release

# Run
cargo run --release --package basalt-server --example server
```

Connect with a Minecraft 1.21.4 client to `localhost:25565`.

---

## Configuration

Create a `basalt.toml` in the working directory. All fields are optional; missing values use sensible defaults.

```toml
[server]
bind = "0.0.0.0:25565"
log_level = "info"       # trace, debug, info, warn, error

[server.performance]
chunk_cache_max_entries = 4096  # ~768 MB max. Each chunk ~ 192 KB.

[world]
seed = 42
storage = "read-write"   # "none" | "read-only" | "read-write"

[plugins]
chat = true
command = true
block = true
world = true
lifecycle = true
movement = true
```

Disable plugins you don't need. A lobby server might only enable `chat`, `command`, and `lifecycle`.

---

## Project Structure

```
basalt/
  crates/
    basalt-types      # Protocol primitives (VarInt, NBT, Slot, Position...)
    basalt-derive      # Proc macros for Encode/Decode
    basalt-protocol    # Packet definitions, generated from minecraft-data
    basalt-net         # Async networking (TCP, encryption, compression)
    basalt-events      # Generic event bus with staged dispatch
    basalt-core        # Shared traits (Context, Gamemode, PluginLogger)
    basalt-command     # Typed command arguments and parsing
    basalt-api         # Standalone plugin API (Plugin trait, events, WorldHandle)
    basalt-world       # World runtime, chunk cache, paletted containers
    basalt-recipes     # Recipe registry, vanilla recipe data
    basalt-storage     # BSR region file format with LZ4 compression
    basalt-server      # Server runtime: game loop, net tasks, ServerContext
  plugins/
    chat/              # Chat broadcast
    command/           # /tp, /gamemode, /say, /help, /stop, /kick, /list
    block/             # Block breaking and placing
    world/             # Chunk streaming
    storage/           # Chunk persistence
    lifecycle/         # Join/leave broadcast
    movement/          # Position broadcast
    physics/           # Gravity, AABB collision
    item/              # Item drops on block break
    container/         # Chest interaction, double chests
  fuzz/                # Fuzz targets for protocol decoders
  xtask/               # Code generation from minecraft-data
```

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
