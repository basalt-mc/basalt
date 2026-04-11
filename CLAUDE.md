# Basalt Protocol Library — Claude Guidelines

## Tech Stack

- **Rust** (latest stable, edition 2024)
- **Tokio** (async runtime for `basalt-net`)
- **Criterion** (benchmarks)
- **Proptest** (property-based testing)
- **cargo-deny** (advisory + license audit)

## Architecture

Four crates in a Cargo workspace under `crates/`:

```
basalt-types → basalt-derive → basalt-protocol → basalt-net
```

| Crate | Purpose | Key dependencies |
|-------|---------|-----------------|
| `basalt-types` | Primitive Minecraft types, `Encode`/`Decode`/`EncodedSize` traits | `thiserror` |
| `basalt-derive` | Proc macros for `Encode`/`Decode`/`EncodedSize` | `syn`, `quote`, `proc-macro2` |
| `basalt-protocol` | Packet definitions, version-aware registry, codegen | `basalt-types`, `basalt-derive` |
| `basalt-net` | Async networking, encryption, compression, middleware pipeline | `basalt-protocol`, `tokio`, `aes`, `cfb8`, `flate2` |

- `basalt-types` and `basalt-derive` have no interdependency.
- `basalt-protocol` depends on both.
- `basalt-net` depends on `basalt-protocol`.

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
│   └── basalt-net/
├── Cargo.toml                # Workspace root
├── Cargo.lock
├── CLAUDE.md
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

## Documentation

### Doc comments on public items

Add doc comments to every public function, struct, trait, and type. Keep it to a short description plus parameter/return documentation when non-obvious.

```rust
/// Decodes a VarInt from the given byte slice.
///
/// Returns the decoded value and the number of bytes consumed.
/// Fails if the input is too short or the VarInt exceeds 5 bytes.
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
cargo xt codegen              # (future) Regenerate protocol from minecraft-data
```

### Cargo aliases

Defined in `.cargo/config.toml`:

```bash
cargo t                       # cargo test
cargo c                       # cargo clippy
cargo b                       # cargo bench
cargo xt                      # cargo run --package xtask --
```

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

## CI

GitHub Actions runs on every push to `main` and on pull requests:

1. **Format** — `cargo fmt --all --check`
2. **Clippy** — `cargo clippy --all-targets --all-features -- -D warnings`
3. **Test** — `cargo test --all-features`
4. **Coverage** — `cargo llvm-cov --all-features --fail-under 90` (minimum 90%, posts report on PRs)
5. **Cargo Deny** — advisory + license audit

All five jobs run in parallel. The concurrency group cancels in-progress runs on the same ref.

## Testing strategy

Three levels of testing:

1. **Unit tests** — each type, each packet, known values, edge cases (VarInt max, empty strings, deep NBT)
2. **Property-based tests** (`proptest`) — `decode(encode(x)) == x` for all types and packets
3. **Real packet fixtures** — captured from a vanilla Minecraft client/server, compatibility regression tests

Benchmarks (`criterion`) from day one: encode/decode throughput, allocations per packet, pipeline middleware latency.

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
