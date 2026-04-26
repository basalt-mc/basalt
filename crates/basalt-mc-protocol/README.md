# basalt-mc-protocol

Minecraft packet definitions and version-aware packet registry for the
[Basalt](https://github.com/basalt-mc/basalt) Minecraft server.

## Overview

This crate defines all Minecraft protocol packets as Rust structs with
derive-generated `Encode`/`Decode`/`EncodedSize` implementations. Packets are
organized by connection state and direction.

## Connection states

| State | Serverbound | Clientbound |
|-------|-------------|-------------|
| Handshake | Handshake | -- |
| Status | StatusRequest, Ping | StatusResponse, Pong |
| Login | LoginStart, EncryptionResponse, ... | LoginSuccess, ... |
| Configuration | ClientInformation, ... | RegistryData, ... |
| Play | ~70 packets | ~110 packets |

## Packet registry

The `PacketRegistry` provides version-aware dispatch: given a protocol version,
connection state, direction, and packet ID, it decodes raw bytes into a typed
enum variant.

```rust,ignore
use basalt_mc_protocol::{PacketRegistry, ConnectionState, ProtocolVersion};

let registry = PacketRegistry::new(ProtocolVersion::V1_21_4);

// Decode a raw packet by its state, direction, and ID
let packet = registry.decode_serverbound(
    ConnectionState::Play,
    packet_id,
    &payload,
)?;
```

## Registry data

Pre-built registry codec entries (dimension types, biomes, damage types, etc.)
required during the Configuration state are available in the `registry_data`
module.

## Code generation

Packet structs are generated from
[PrismarineJS/minecraft-data](https://github.com/PrismarineJS/minecraft-data)
JSON definitions using the `xtask` codegen tool. The generated code is committed
to the repository for zero build-time cost.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](../../LICENSE)
for details.
