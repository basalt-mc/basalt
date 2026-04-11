/**
 * Allowed commit scopes for the Basalt protocol library.
 *
 * Crate scopes match the crate directory name under crates/:
 *   - Work on crates/basalt-types/ → scope 'types'
 *   - Work on crates/basalt-derive/ → scope 'derive'
 *
 * Keywords cover cross-cutting concerns that don't belong to a single crate.
 *
 * When you create a new crate or cross-cutting concern, add its scope to
 * the matching array below.
 */

const crates = [
  // Primitive Minecraft types with zero-copy serialization: VarInt, VarLong,
  // Position, UUID, Identifier, NBT, TextComponent, BitSet, ByteArray, etc.
  // Encode/Decode/EncodedSize trait definitions live here.
  // Example: "feat(types): add VarInt encode/decode"
  'types',

  // Proc-macro crate: derive macros for Encode, Decode, EncodedSize.
  // Attributes: #[packet(id)], #[field(varint)], #[field(optional)], etc.
  // Example: "feat(derive): add #[field(rest)] attribute for trailing bytes"
  'derive',

  // Minecraft packet definitions and version-aware packet registry.
  // Shared packet structs, per-version overlays, codegen from minecraft-data.
  // Example: "feat(protocol): add Handshake packet definition"
  'protocol',

  // Async networking layer: TCP connection management, encryption (AES/CFB-8),
  // compression (zlib), framing, typestate connection, middleware pipeline.
  // Example: "feat(net): add Connection typestate for login sequence"
  'net',
];

const keywords = [
  // Root workspace configuration: Cargo.toml workspace settings, workspace-wide
  // dependency versions, cross-crate build configuration.
  // Example: "chore(workspace): update workspace dependency versions"
  'workspace',

  // Criterion benchmarks: encode/decode throughput, allocations per packet,
  // pipeline middleware latency. Benchmark harness and fixtures.
  // Example: "feat(bench): add VarInt encode/decode throughput benchmark"
  'bench',

  // GitHub Actions workflows: fmt, clippy, test, cargo-deny checks.
  // Example: "ci(ci): add cargo-deny advisory check"
  'ci',

  // CLAUDE.md guidelines: conventions, architectural decisions, patterns.
  // Example: "docs(claude): document commit scope convention"
  'claude',

  // Adding, removing, or updating dependencies in Cargo.toml or package.json.
  // Example: "chore(deps): upgrade tokio to 1.40"
  'deps',

  // Documentation files: README.md, inline architectural notes, specs.
  // NOT for doc comments in source code (those ship with the code).
  // Example: "docs(docs): add architecture overview to README"
  'docs',

  // Git configuration: .gitignore, .gitattributes, .gitmodules.
  // Example: "chore(git): add minecraft-data submodule"
  'git',

  // Husky git hooks setup: pre-commit, commit-msg hook scripts
  // and the .husky/ directory.
  // Example: "chore(husky): add cargo fmt check to pre-commit"
  'husky',

  // lint-staged, rustfmt, clippy configuration, cargo-deny config.
  // For the linting/formatting pipeline, not for individual lint fixes.
  // Example: "chore(lint): add clippy nursery lints to workspace"
  'lint',

  // Test infrastructure: test helpers, shared fixtures, proptest strategies,
  // real packet captures. NOT for individual test files (those go under
  // the scope of the crate they test).
  // Example: "feat(test): add proptest strategy for VarInt"
  'test',

  // Cross-cutting developer tooling: commitlint, editor settings, xtask,
  // codegen tool, or other tools that don't have their own keyword.
  // Example: "chore(tooling): configure commitlint scope enum"
  'tooling',
];

export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    'scope-enum': [2, 'always', [...crates, ...keywords]],
    'scope-empty': [2, 'never'],
  },
};
