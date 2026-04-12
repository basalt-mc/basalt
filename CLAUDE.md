# Basalt Protocol Library — Claude Guidelines

## Tech Stack

- **Rust** (latest stable, edition 2024)
- **Tokio** (async runtime for `basalt-net`)
- **Criterion** (benchmarks)
- **Proptest** (property-based testing)
- **cargo-deny** (advisory + license audit)
- **cargo-llvm-cov** (code coverage, 90% minimum threshold)

## Architecture

Five crates in a Cargo workspace under `crates/`, plus an `xtask` codegen tool:

```
basalt-types → basalt-derive → basalt-protocol → basalt-net → basalt-server
                                      ↑
                                   xtask (codegen)
```

| Crate | Purpose | Key dependencies |
|-------|---------|-----------------|
| `basalt-types` | Primitive Minecraft types, `Encode`/`Decode`/`EncodedSize` traits | `thiserror` |
| `basalt-derive` | Proc macros for `Encode`/`Decode`/`EncodedSize` | `syn`, `quote`, `proc-macro2` |
| `basalt-protocol` | Packet definitions, version-aware registry, registry data, chunk builder | `basalt-types`, `basalt-derive` |
| `basalt-net` | Async networking, encryption, compression, connection typestate, middleware pipeline | `basalt-protocol`, `tokio`, `aes`, `cfb8`, `flate2` |
| `basalt-server` | Minecraft server: connection lifecycle, play loop, chat, commands, player state | `basalt-net`, `basalt-protocol`, `basalt-types`, `tokio` |
| `xtask` | Code generation from minecraft-data JSON → Rust packet structs | `serde_json` |

- `basalt-types` and `basalt-derive` have no interdependency.
- `basalt-protocol` depends on both.
- `basalt-net` depends on `basalt-protocol`.
- `basalt-server` depends on `basalt-net` — it is the top-level application crate.
- `xtask` is a standalone binary that generates code into `basalt-protocol`.

### basalt-server structure

```
crates/basalt-server/
├── src/
│   ├── lib.rs           # Server struct, public API, accept loop
│   ├── connection.rs    # Per-player lifecycle: handshake → login → config → play
│   ├── play.rs          # Play loop, packet dispatch (sync) + action execution (async)
│   ├── player.rs        # PlayerState: position, rotation, keep-alive tracking
│   └── chat.rs          # Chat message echo, command dispatch (/say, /tp, /gamemode, /help)
├── examples/
│   └── server.rs        # 14-line launcher: Server::new("0.0.0.0:25565").run().await
└── tests/
    └── e2e.rs           # End-to-end tests: status ping, login flow, chat, commands
```

The play loop separates sync and async concerns:
- `dispatch_packet()` — pure sync function that updates `PlayerState` and returns a `PacketAction` enum
- `execute_action()` — async function that sends response packets (chat echo, command feedback)

This keeps the state-update logic unit-testable without a TCP connection.

## Architectural principles

### Zero-copy and minimal allocations

Serialization works on `&[u8]` / `&mut Vec<u8>` — sync byte slices, no async. `EncodedSize` enables exact buffer pre-allocation. No unnecessary cloning. Async happens only in `basalt-net`.

### Per-crate ownership

Each crate owns its types, errors, tests, and benchmarks. There is no shared `common` crate. Error types are per-crate: `basalt_types::Error`, `basalt_protocol::Error`, `basalt_net::Error`. Higher crates wrap lower errors via `#[from]`.

### Sync by default, async at the boundary

`basalt-types` and `basalt-protocol` are fully synchronous. Async is introduced only in `basalt-net` for IO. This keeps the core testable without async runtimes.

### Multi-version without duplication

Packets shared across Minecraft versions live as a single struct in `packets/`. Only changed packets get version-specific structs in `versions/v1_XX/`. Packet ID mappings are always per-version.

### NBT is in-house

No `fastnbt`, `simdnbt`, or `serde` for NBT. The protocol uses a predictable subset; a custom implementation integrates natively with `Encode`/`Decode`.

## Project structure

```
basalt/
├── .cargo/
│   └── config.toml          # Cargo aliases (t, c, b, xt)
├── .github/
│   ├── ISSUE_TEMPLATE/
│   │   ├── feature.yml
│   │   └── bug.yml
│   ├── PULL_REQUEST_TEMPLATE.md
│   └── workflows/
│       └── ci.yml
├── .husky/
│   ├── commit-msg
│   └── pre-commit
├── crates/
│   ├── basalt-types/
│   ├── basalt-derive/
│   ├── basalt-protocol/
│   ├── basalt-net/
│   └── basalt-server/        # Minecraft server: connection lifecycle, play loop, chat, commands
├── minecraft-data/           # Git submodule — PrismarineJS/minecraft-data
├── xtask/                    # Codegen tool
│   └── src/
│       ├── main.rs           # Entry point, config, orchestration
│       ├── types.rs          # ProtocolType IR, ResolvedField, PacketDef
│       ├── registry.rs       # TypeRegistry: JSON → ProtocolType resolution
│       ├── codegen.rs        # ProtocolType → Rust source code generation
│       ├── play.rs           # Play state category-based file splitting
│       └── helpers.rs        # to_pascal_case, to_snake_case, format_file
├── Cargo.toml                # Workspace root
├── Cargo.lock
├── CLAUDE.md
├── Makefile                  # Common dev commands
├── LICENSE
├── commitlint.config.js
├── deny.toml
├── package.json
├── pnpm-lock.yaml
└── rustfmt.toml
```

## Crate structure

Each crate follows this layout:

```
crates/basalt-<name>/
├── Cargo.toml
├── src/
│   ├── lib.rs                # Public API, re-exports
│   ├── error.rs              # Crate-specific error type
│   └── <module>.rs           # One file per logical unit
└── benches/
    └── <name>.rs             # Criterion benchmarks
```

- Tests are co-located: `#[cfg(test)] mod tests` at the bottom of each file.
- Integration tests go in `tests/` at the crate root when needed.
- No `mod.rs` files — use `<module>.rs` with `mod <submodule>;` style.

## Codegen pipeline (xtask)

The `xtask` crate generates Rust packet definitions from the PrismarineJS/minecraft-data `protocol.json`. The pipeline is:

```
protocol.json → TypeRegistry → ProtocolType (IR) → Rust source code
```

### Key components

- **`TypeRegistry`** (`registry.rs`): merges per-direction types with global types, resolves type references, detects switch groups for enum generation. Created once per direction (toServer/toClient).
- **`ProtocolType`** (`types.rs`): intermediate representation enum. Every protocol field maps to one variant (VarInt, String, Array, InlineStruct, SwitchEnum, Bitfield, Opaque, etc.). The `to_rust()` method converts to `(rust_type_string, field_attribute)`.
- **`codegen.rs`**: walks the IR tree to emit Rust structs, inline helper structs, switch enums, direction dispatch enums, and import lines.
- **`play.rs`**: splits the ~180 Play state packets into 6 category files (entity, world, player, inventory, chat, misc).

### Type support

The codegen resolves ALL minecraft-data protocol types with 0 warnings:

| JSON type | Rust output |
|-----------|------------|
| `varint`, `ContainerID`, `optvarint`, `soundSource` | `i32` + `#[field(varint)]` |
| `varlong` | `i64` + `#[field(varlong)]` |
| Primitives (`u8`-`u64`, `i8`-`i64`, `f32`, `f64`, `bool`, `string`) | Direct Rust equivalent |
| `UUID`, `position`, `Slot`, `vec2f`, `vec3f`, `vec3f64`, `vec3i16` | Basalt types |
| `anonymousNbt` | `NbtCompound` |
| `anonOptionalNbt` | `Option<NbtCompound>` + `#[field(optional)]` |
| `ByteArray`, `buffer` | `Vec<u8>` + `#[field(length = "varint")]` |
| `restBuffer` | `Vec<u8>` + `#[field(rest)]` (last field only) |
| `option` | `Option<T>` + `#[field(optional)]` |
| `array` | `Vec<T>` + `#[field(length = "varint")]` |
| `mapper`, `bitflags` | Underlying integer type |
| `bitfield` | `u8`/`u16`/`u32`/`u64` based on total bit count |
| `container` (inline) | Generated `InlineStruct` with Encode/Decode/EncodedSize derives |
| `switch` | Generated `SwitchEnum` with `#[variant(id = N)]` attributes |
| `void` | Field filtered out (no wire data) |
| `native`, `registryEntryHolder`, `topBitSetTerminatedArray`, `entityMetadataLoop` | `Vec<u8>` opaque fallback (no warning) |
| Custom types (`SpawnInfo`, `ChatType`, `tags`, etc.) | Resolved from merged type context |

### Switch enum generation

When multiple switch fields share the same `compareTo` discriminator:
1. The discriminator field is absorbed into the enum (not emitted separately)
2. Each discriminator value becomes an enum variant with its specific fields
3. Variant fields support `#[field(...)]` attributes (varint, optional, etc.)
4. A `Default` impl is generated (first variant)
5. Works for both trailing switches (all at the end) and interleaved switches (normal fields after the switch group, e.g., `use_entity` where `sneaking: bool` follows)

Switches with relative-path comparisons (`../action/...`) or non-void defaults are not converted to enums — they fall back to `Vec<u8>`.

### Running codegen

```bash
cargo xt codegen              # Regenerate all packets
# or
make codegen                  # Same + cargo fmt
```

After running codegen, the generated files in `crates/basalt-protocol/src/packets/` must be committed. CI runs a codegen drift check on PRs to catch uncommitted changes.

## Derive macros

### `#[packet(id = N)]`

Applied to packet structs. Generates `Encode`, `Decode`, `EncodedSize` impls and a `PACKET_ID: i32` constant. Does NOT encode the packet ID on the wire — framing handles that.

### `#[derive(Encode, Decode, EncodedSize)]`

For non-packet types (inline structs, switch enums). Supports both structs and enums.

### `#[field(...)]` attributes

| Attribute | Meaning | Used on |
|-----------|---------|---------|
| `varint` | Encode as VarInt (1-5 bytes) | `i32` fields |
| `varlong` | Encode as VarLong (1-10 bytes) | `i64` fields |
| `optional` | Boolean-prefixed optional value | `Option<T>` fields |
| `length = "varint"` | VarInt length prefix for collections | `Vec<T>` fields |
| `rest` | Consume all remaining bytes (must be last field) | `Vec<u8>` fields |

All attributes work on both struct fields and enum variant fields.

### `#[variant(id = N)]` attribute

Applied to enum variants for discriminator-based dispatch. The discriminant is encoded as a VarInt.

## Documentation

### Doc comments on public items

Add doc comments to **every** public function, struct, trait, enum, and type. Every `fn`, every `impl`, every `struct`, every `enum` — no exceptions.

Keep it to a short description plus parameter/return documentation when non-obvious. Describe **what** the thing does, **why** it exists, the **wire format** when relevant, the **protocol usage context**, and **error cases**.

```rust
/// Decodes a VarInt from the given byte slice.
///
/// VarInts use the MSB of each byte as a continuation bit, encoding
/// i32 values in 1-5 bytes. Used throughout the Minecraft protocol
/// for packet IDs, string lengths, array counts, and entity metadata.
///
/// Fails with `Error::VarIntTooLarge` if more than 5 bytes are read
/// without finding a terminating byte (MSB = 0).
pub fn decode(buf: &[u8]) -> Result<(Self, usize)> {
    // ...
}
```

**Do not** add:
- File-level comments restating the module name
- `# Examples` sections — tests serve that purpose
- Redundant type documentation (`/// A VarInt` on `struct VarInt`)

### Inline comments

Comment blocks with non-obvious intent. Do **not** comment every line.

```rust
// Good — explains why
// Minecraft VarInts use the MSB as a continuation bit
let has_more = byte & 0x80 != 0;

// Bad — restates the code
let has_more = byte & 0x80 != 0; // check if MSB is set
```

Rules:
- Single-line `//` comments only
- Comment **blocks of logic**, not individual trivial lines
- Explain the **why**, never the **what** when the what is clear

## Local development

```bash
cargo check                   # Type-check all crates
cargo test                    # Run all tests
cargo clippy                  # Lint
cargo fmt                     # Format
cargo bench                   # Run benchmarks
cargo deny check              # Audit advisories + licenses
cargo xt codegen              # Regenerate protocol packets
make coverage                 # Run coverage (must be ≥ 90%)
make check                    # fmt + clippy + test in one command
```

### Cargo aliases

Defined in `.cargo/config.toml`:

```bash
cargo t                       # cargo test
cargo c                       # cargo clippy
cargo b                       # cargo bench
cargo xt                      # cargo run --package xtask --
```

### Pre-push checklist

**ALWAYS** run this sequence before every `git push`. No exceptions:

```bash
cargo fmt --all --check                                           # 1. Format
cargo clippy --all-targets --all-features -- -D warnings          # 2. Lint
cargo test                                                        # 3. Tests
cargo llvm-cov --all-features --ignore-filename-regex "(packets/|examples/)"  # 4. Coverage ≥ 90%
```

If coverage drops below 90%, **add tests before pushing**. Do NOT push with failing coverage — CI will reject it anyway. Generated code (`packets/`) and examples are excluded from coverage.

## Branching

When implementing a feature or fix, create a branch before committing:

```
feat/<short-description>      # for features
fix/<short-description>       # for bug fixes
chore/<short-description>     # for maintenance tasks
docs/<short-description>      # for documentation
refactor/<short-description>  # for refactors without behavior change
test/<short-description>      # for test additions
```

## Commit Convention

Conventional Commits enforced by commitlint with a **strict scope-enum** — the scope is mandatory and must be in the allowed list (see `commitlint.config.js` for the full list with descriptions and examples).

### Scope format

Scopes follow the crate structure. Bare crate names for crate work, keywords for cross-cutting concerns:

```
<crate>            → crate-level work (types, derive, protocol, net)
<crate>/<module>   → module-specific work (types/varint, net/connection)
<keyword>          → cross-cutting concern (deps, ci, lint, tooling, ...)
```

### Examples

```
feat(types): add VarInt encode/decode
feat(derive): add #[packet(id)] attribute
fix(protocol): correct Handshake packet field order
chore(deps): upgrade tokio to 1.40
ci(ci): add cargo-deny advisory check
docs(claude): document commit scope convention
chore(tooling): configure commitlint scope enum
refactor(tooling): replace parser with IR-based TypeRegistry
```

### Adding new scopes

When you create a new crate or cross-cutting concern, add its scope to the matching array in `commitlint.config.js` with a comment explaining when to use it. The commit that creates the directory is also the commit that declares its scope.

### Forbidden patterns

- **No sub-paths as scopes** (`types/varint` → use `types`)
- **No scopes for things that don't exist yet** — add the scope in the same PR that creates the crate/concern
- **No scope-less commits** — every commit must have a scope from the allowed list

**Never** add a `Co-Authored-By: Claude` trailer to commits. **Never** mention Claude in PR titles, descriptions, or issue comments.

### Commit message body

The subject line is the **what** in one sentence. When a change has a non-obvious **why**, add a **short commit description** (3-8 lines, separated from the subject by a blank line). The body explains *why*, not *what*.

```
refactor(types): split NBT implementation into submodules

The single nbt.rs file exceeded 800 lines and mixed parsing, encoding,
and type definitions. Splitting into nbt/mod.rs, nbt/decode.rs,
nbt/encode.rs, and nbt/types.rs makes each piece independently testable.
```

A body is **encouraged** for refactors, multi-file changes, and non-obvious decisions. **Optional** for trivial fixes. Detailed walkthroughs belong in the **PR description**, not in commit messages.

## Issues and PRs

### Issues

**Always** create a GitHub issue before starting implementation. Use the templates in `.github/ISSUE_TEMPLATE/` (feature.yml or bug.yml). Fill in ALL template fields with substantive content — Context, Problem, Proposed approach, Scope, Benefits, Non-goals. Never create a minimal or lazy issue body.

### Pull requests

Write detailed PR bodies with a Summary section (bullet points) and a Test plan section (checklist). Reference the issue with `Closes #N`. Never use triple backticks in `gh` CLI bodies — they break GitHub rendering. Use `--body-file` with heredocs instead.

## CI

GitHub Actions runs on every push to `main` and on pull requests:

1. **Format** — `cargo fmt --all --check`
2. **Clippy** — `cargo clippy --all-targets --all-features -- -D warnings`
3. **Test** — `cargo test --all-features`
4. **Coverage** — `cargo llvm-cov --all-features --fail-under-lines 90 --ignore-filename-regex "(examples|packets/)"` (minimum 90%)
5. **Codegen drift** — (PRs only) re-runs codegen and checks for uncommitted changes in `packets/`
6. **Cargo Deny** — advisory + license audit

All jobs run in parallel. The concurrency group cancels in-progress runs on the same ref.

### Coverage rules

- **90% minimum line coverage** on all non-generated code
- Generated packet files (`packets/`) are excluded — they have no inline tests
- Example files (`examples/`) are excluded
- `xtask/src/main.rs` is excluded implicitly (binary entry point, 0% coverage is expected)
- When adding new code, add tests to maintain coverage. If coverage drops, add tests before pushing.

## Testing strategy

Three levels of testing:

1. **Unit tests** — each type, each packet, known values, edge cases (VarInt max, empty strings, deep NBT)
2. **Property-based tests** (`proptest`) — `decode(encode(x)) == x` for all types and packets
3. **Real packet fixtures** — captured from a vanilla Minecraft client/server, compatibility regression tests

Benchmarks (`criterion`) from day one: encode/decode throughput, allocations per packet, pipeline middleware latency.

### Server testing

The server example (`crates/basalt-net/examples/server.rs`) implements a minimal Minecraft 1.21.4 server with:
- Status flow (server list ping)
- Login flow (offline mode)
- Configuration (registry data: dimension types, biomes, 49 damage types, painting/wolf variants)
- Play (Login packet, spawn position, empty chunk, player position, keep-alive loop)

Test by running `cargo run --package basalt-net --example server` and connecting with a Minecraft 1.21.4 client to `localhost:25565`. The player spawns in creative mode in a void world at (0, 100, 0).

Use SniffCraft (MITM proxy in `sniffcraft/`) to capture and validate packets between a real client and server.

## Key rules

1. **Zero-copy where possible**: `Encode`/`Decode` on byte slices, `EncodedSize` for pre-allocation, no unnecessary cloning.
2. **Sync core, async boundary**: only `basalt-net` uses async. Everything else is sync and testable without a runtime.
3. **Per-crate errors**: each crate defines its own `Error` type. Higher crates wrap lower errors via `#[from]`.
4. **No serde for protocol types**: `Encode`/`Decode` are the serialization traits. Serde is not used for wire format.
5. **NBT in-house**: no external NBT crate. Custom implementation tuned for the protocol subset.
6. **Exhaustive pattern matching**: packet registries return typed enums per connection state. No `Box<dyn Any>`.
7. **Multi-version via delta/overlay**: shared packets + per-version overrides. No duplication.
8. **Benchmarks from day one**: every crate with performance-sensitive code has Criterion benchmarks.
9. **Clippy is strict**: `-D warnings` in CI. No `#[allow]` without a comment explaining why.
10. **Generated code is committed**: codegen output lives in the repo, visible in PRs, no build-time cost.
11. **Coverage before push**: always verify ≥ 90% locally before pushing. No exceptions.
12. **Issues before code**: always create a detailed GitHub issue (using templates) before starting implementation.
13. **Doc on everything**: every fn, struct, enum, trait gets a doc comment. Describe what, why, wire format, error cases.
14. **IR-based codegen**: the xtask pipeline uses a `ProtocolType` IR — never go directly from JSON to Rust strings.
