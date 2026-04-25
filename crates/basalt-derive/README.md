# basalt-derive

Proc-macro crate for the [Basalt](https://github.com/basalt-mc/basalt)
Minecraft server. Generates `Encode`, `Decode`, and `EncodedSize` trait
implementations for protocol packet structs and inner data types.

## Macros

### `#[packet(id = N)]`

Attribute macro for protocol packets. Generates all three trait impls plus a
`PACKET_ID` constant:

```rust,ignore
#[derive(Debug, Clone, PartialEq)]
#[packet(id = 0x00)]
pub struct StatusRequest;
```

### `#[derive(Encode, Decode, EncodedSize)]`

Standard derive macros for non-packet types (inline structs, enums, inner data
structures):

```rust,ignore
#[derive(Debug, Encode, Decode, EncodedSize)]
pub struct PlayerPosition {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub on_ground: bool,
}
```

## Field attributes

| Attribute | Effect |
|-----------|--------|
| `#[field(varint)]` | Encode `i32` as VarInt (1-5 bytes) |
| `#[field(varlong)]` | Encode `i64` as VarLong (1-10 bytes) |
| `#[field(optional)]` | Boolean-prefixed `Option<T>` |
| `#[field(length = "varint")]` | VarInt length prefix for `Vec<T>` |
| `#[field(element = "varint")]` | Encode each element as VarInt |
| `#[field(rest)]` | Consume remaining bytes (last field only) |

## Variant attributes

`#[variant(id = N)]` on enum variants for discriminator-based dispatch. The
discriminant is encoded as a VarInt.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](../../LICENSE)
for details.
