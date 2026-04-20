# Contributing to Basalt

Thanks for your interest in contributing to Basalt! This document covers everything you need to get started.

## Getting Started

### Prerequisites

- Rust stable (latest)
- Node.js 18+ (for commitlint/husky)
- pnpm

### Setup

```bash
git clone https://github.com/basalt-mc/basalt.git
cd basalt
pnpm install          # installs commitlint + husky hooks
cargo build           # verify everything compiles
cargo test            # run the test suite
```

### Useful commands

```bash
make check            # fmt + clippy + test (the pre-push checklist)
make coverage         # run coverage (must be >= 90%)
make codegen          # regenerate protocol packets from minecraft-data
cargo xt codegen      # same as above without fmt
```

## Development Workflow

1. **Create an issue** — describe what you're building or fixing using the templates in `.github/ISSUE_TEMPLATE/`
2. **Create a branch** — use the naming convention: `feat/`, `fix/`, `refactor/`, `chore/`, `docs/`, `test/`
3. **Implement** — write code, tests, and doc comments
4. **Verify locally** — run `make check` and `make coverage` before pushing
5. **Open a PR** — reference the issue with `Closes #N`, write a detailed description

## Commit Convention

We use [Conventional Commits](https://www.conventionalcommits.org/) enforced by commitlint. Every commit must have a type and a scope:

```
type(scope): description
```

**Types:** `feat`, `fix`, `refactor`, `perf`, `docs`, `chore`, `ci`, `test`, `bench`

**Scopes:** must be from the allowed list in `commitlint.config.js`. Common scopes:
- Crate names: `types`, `derive`, `protocol`, `net`, `server`, `world`, `ecs`, `api`, `core`, `command`, `events`, `storage`, `testkit`
- Plugin names: `chat`, `block`, `command`, `world`, `lifecycle`, `movement`, `physics`, `item`, `container`, `storage`
- Cross-cutting: `workspace`, `deps`, `ci`, `docs`, `tooling`

Sub-module scopes are also available (e.g., `types/varint`, `net/connection`). See `commitlint.config.js` for the full list.

## Code Standards

### Testing

- **90% minimum coverage** — CI rejects anything below this threshold
- Unit tests go in `#[cfg(test)] mod tests` at the bottom of each file
- Plugin tests use `PluginTestHarness` from `basalt-testkit`
- Property-based tests with `proptest` for encode/decode roundtrips

### Documentation

Every public item (function, struct, enum, trait) must have a doc comment. Describe what it does, why it exists, and error cases when relevant.

### Style

- `cargo fmt` — enforced in CI
- `cargo clippy` with `-D warnings` — zero warnings allowed
- No `unsafe` blocks
- Keep files under ~400 lines

### Plugin Rules

Plugins depend only on `basalt-api`. Never import internal crates (`basalt-ecs`, `basalt-core`, `basalt-world`) directly in plugin code.

## Architecture

See `CLAUDE.md` for a comprehensive architecture guide including:
- Crate dependency graph
- Server threading model
- Event system stages
- Plugin development patterns
- ECS design

## Need Help?

Open an issue with the question label, or check existing issues for context on ongoing work.
