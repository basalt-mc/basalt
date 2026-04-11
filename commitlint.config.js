/**
 * Allowed commit scopes for the Basalt protocol library.
 *
 * Structured scopes use the crate/module path:
 *   - Work on crates/basalt-types/src/varint.rs → scope 'types/varint'
 *   - Work on crates/basalt-net/src/connection.rs → scope 'net/connection'
 *
 * Bare crate scopes (types, derive, protocol, net) are used for
 * cross-module changes within a crate.
 *
 * Keywords cover cross-cutting concerns that don't belong to a single crate.
 *
 * When you create a new module or cross-cutting concern, add its scope to
 * the matching array below.
 */

const types = [
  // Cross-module changes within basalt-types.
  // Example: "feat(types): add new protocol type"
  'types',

  // Encode/Decode/EncodedSize trait definitions and Result type alias.
  // Example: "refactor(types/traits): change Decode signature"
  'types/traits',

  // Error enum and error handling.
  // Example: "feat(types/error): add new error variant"
  'types/error',

  // VarInt (i32, 1-5 bytes) and VarLong (i64, 1-10 bytes) variable-length
  // integer encoding. The most used types in the protocol.
  // Example: "fix(types/varint): handle edge case in max-length encoding"
  'types/varint',

  // Primitive type implementations: bool, u8-u64, i8-i64, f32, f64.
  // Big-endian encoding for all fixed-size numerics.
  // Example: "feat(types/primitives): add u128 support"
  'types/primitives',

  // VarInt-prefixed UTF-8 string with 32767 byte max length.
  // Example: "fix(types/string): handle max-length edge case"
  'types/string',

  // VarInt-prefixed raw byte sequence (Vec<u8>).
  // Example: "feat(types/byte-array): add length validation"
  'types/byte-array',

  // Packed 64-bit block position (x:26, z:26, y:12), BlockPosition,
  // and ChunkPosition coordinate types.
  // Example: "fix(types/position): sign extension for negative y"
  'types/position',

  // 128-bit UUID encoded as two big-endian u64 values.
  // Example: "feat(types/uuid): add v4 generation"
  'types/uuid',

  // Namespaced identifier in namespace:path format.
  // Example: "fix(types/identifier): validate empty namespace"
  'types/identifier',

  // Single-byte rotation angle (0-255 maps to 0-360 degrees).
  // Example: "feat(types/angle): add radians conversion"
  'types/angle',

  // Variable-length bit array for chunk light masks and bitmasks.
  // Example: "fix(types/bit-set): handle empty BitSet encoding"
  'types/bit-set',

  // In-house NBT implementation: tag types, encode, decode.
  // Example: "feat(types/nbt): add SNBT string parsing"
  'types/nbt',

  // Rich text component for chat, titles, and UI text. Encoded as NBT.
  // Example: "feat(types/text): add translation fallback"
  'types/text',

  // Item stack type for inventories and entity equipment.
  // Example: "feat(types/slot): parse item components"
  'types/slot',

  // Vector types: Vec2f, Vec3f, Vec3f64, Vec3i16.
  // Example: "feat(types/vectors): add Vec4f"
  'types/vectors',
];

const derive = [
  // Cross-module changes within basalt-derive.
  // Example: "refactor(derive): reorganize module structure"
  'derive',

  // The #[packet(id = N)] attribute macro that generates Encode/Decode/
  // EncodedSize + PACKET_ID constant.
  // Example: "feat(derive/packet): support tuple struct packets"
  'derive/packet',

  // Attribute parsing: #[field(varint)], #[variant(id = N)], etc.
  // Example: "feat(derive/attrs): add #[field(count)] attribute"
  'derive/attrs',

  // Encode derive implementation for structs and enums.
  // Example: "fix(derive/encode): handle optional VarInt fields"
  'derive/encode',

  // Decode derive implementation for structs and enums.
  // Example: "fix(derive/decode): validate buffer bounds"
  'derive/decode',

  // EncodedSize derive implementation.
  // Example: "fix(derive/size): account for optional prefix byte"
  'derive/size',
];

const protocol = [
  // Cross-module changes within basalt-protocol.
  // Example: "feat(protocol): add new connection state"
  'protocol',

  // Packet definitions: handshake, status, login, configuration, play.
  // Example: "feat(protocol/packets): regenerate from minecraft-data 1.21.4"
  'protocol/packets',

  // Version-aware packet registry for ID-based dispatch.
  // Example: "feat(protocol/registry): add Configuration dispatch"
  'protocol/registry',

  // ConnectionState enum and related types.
  // Example: "feat(protocol/state): add Transfer state"
  'protocol/state',

  // ProtocolVersion enum and version negotiation.
  // Example: "feat(protocol/version): add 1.21.5 support"
  'protocol/version',

  // Error type for protocol-level errors.
  // Example: "feat(protocol/error): add PacketTooLarge variant"
  'protocol/error',
];

const net = [
  // Cross-module changes within basalt-net.
  // Example: "refactor(net): reorganize module structure"
  'net',

  // TCP framing: VarInt length-prefixed read/write.
  // Example: "fix(net/framing): handle partial VarInt reads"
  'net/framing',

  // ProtocolStream: TCP stream with transparent encryption and compression.
  // Example: "feat(net/stream): add write buffering"
  'net/stream',

  // Connection typestate: Handshake → Status/Login → Configuration → Play.
  // Example: "feat(net/connection): add Configuration state"
  'net/connection',

  // AES-128 CFB-8 cipher pair for protocol encryption.
  // Example: "perf(net/crypto): optimize CFB-8 with SIMD"
  'net/crypto',

  // Zlib compression for packet payloads.
  // Example: "feat(net/compression): add configurable compression level"
  'net/compression',

  // Middleware pipeline for packet-level hooks.
  // Example: "feat(net/pipeline): add async middleware support"
  'net/pipeline',
];

const keywords = [
  // Root workspace configuration: Cargo.toml workspace settings, workspace-wide
  // dependency versions, cross-crate build configuration.
  // Example: "chore(workspace): update workspace dependency versions"
  'workspace',

  // Criterion benchmarks: encode/decode throughput, allocations per packet,
  // pipeline middleware latency.
  // Example: "feat(bench): add VarInt encode/decode throughput benchmark"
  'bench',

  // GitHub Actions workflows: fmt, clippy, test, cargo-deny checks.
  // Example: "ci(ci): add cargo-deny advisory check"
  'ci',

  // Code generation: xtask codegen tool, minecraft-data parsing, generated
  // packet definitions.
  // Example: "feat(codegen): add Configuration and Play states"
  'codegen',

  // Code coverage: cargo-llvm-cov configuration, coverage thresholds.
  // Example: "ci(coverage): add coverage job with 90% threshold"
  'coverage',

  // CLAUDE.md guidelines: conventions, architectural decisions, patterns.
  // Example: "docs(claude): document commit scope convention"
  'claude',

  // Adding, removing, or updating dependencies in Cargo.toml or package.json.
  // Example: "chore(deps): upgrade tokio to 1.40"
  'deps',

  // Documentation files: README.md, specs, architectural notes.
  // Example: "docs(docs): add architecture overview to README"
  'docs',

  // Git configuration: .gitignore, .gitattributes, .gitmodules.
  // Example: "chore(git): add minecraft-data submodule"
  'git',

  // Husky git hooks setup: pre-commit, commit-msg hook scripts.
  // Example: "chore(husky): add cargo fmt check to pre-commit"
  'husky',

  // lint-staged, rustfmt, clippy configuration, cargo-deny config.
  // Example: "chore(lint): add clippy nursery lints to workspace"
  'lint',

  // Test infrastructure: test helpers, shared fixtures, proptest strategies.
  // Example: "feat(test): add proptest strategy for VarInt"
  'test',

  // Cross-cutting developer tooling: commitlint, Makefile, editor settings.
  // Example: "chore(tooling): configure commitlint scope enum"
  'tooling',
];

export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    'scope-enum': [2, 'always', [...types, ...derive, ...protocol, ...net, ...keywords]],
    'scope-empty': [2, 'never'],
  },
};
