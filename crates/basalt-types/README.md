# basalt-types

Primitive Minecraft protocol types with zero-copy serialization for the
[Basalt](https://github.com/basalt-mc/basalt) Minecraft server.

## Types

This crate defines the foundational wire types used across the Minecraft
protocol:

- **VarInt / VarLong** -- variable-length integer encoding (1-5 / 1-10 bytes)
- **Position** -- packed 64-bit block coordinates (x:26, z:26, y:12)
- **UUID** -- 128-bit identifier encoded as two big-endian `u64` values
- **Slot** -- item stack with optional NBT component data
- **TextComponent** -- rich text for chat, titles, and UI (NBT-encoded)
- **NBT** -- in-house Named Binary Tag implementation (compound, list, tags)
- **Angle** -- single-byte rotation (0-255 maps to 0-360 degrees)
- **BitSet** -- variable-length bit array for chunk masks
- **Identifier** -- namespaced resource identifier (`namespace:path`)
- **Vectors** -- Vec2f, Vec3f, Vec3f64, Vec3i16

## Traits

Three traits define the serialization contract:

- `Encode` -- serialize a value to bytes
- `Decode` -- deserialize a value from bytes
- `EncodedSize` -- compute exact wire size for buffer pre-allocation

All primitive Rust types (`bool`, `u8`-`u64`, `i8`-`i64`, `f32`, `f64`) and
protocol strings implement these traits with big-endian byte order.

## Usage

```rust,ignore
use basalt_types::{VarInt, Encode, Decode, EncodedSize};

let mut buf = Vec::new();
VarInt(300).encode(&mut buf).unwrap();

let (decoded, bytes_read) = VarInt::decode(&buf).unwrap();
assert_eq!(decoded.0, 300);
assert_eq!(bytes_read, VarInt(300).encoded_size());
```

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](../../LICENSE)
for details.
