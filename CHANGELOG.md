# Changelog

All notable changes to Basalt are documented here.
## [0.2.1] - 2026-04-26

### Documentation

- Rename basalt-protocol references to basalt-mc-protocol


### Other

- Include Cargo.lock in release commit

- Rename basalt-protocol crate to basalt-mc-protocol

- Update packet_play fuzz target to use basalt_mc_protocol


## [0.2.0] - 2026-04-26

### Block Plugin

- Migrate from basalt-testkit to basalt-api testing feature


### Chat Plugin

- Migrate from basalt-testkit to basalt-api testing feature


### Code Generation

- Xtask recipes subcommand

- Emit recursive switch-on-tag union types natively

- Collapse if-in-match arms for clippy 1.95


### Commands

- Delete basalt-command crate

- Migrate from basalt-testkit to basalt-api testing feature


### Container Plugin

- Migrate chest behaviour to event handlers

- Migrate to typed WorldContext methods

- Migrate from basalt-testkit to basalt-api testing feature


### Core

- Add TickBudget struct and SystemContext budget API

- Add CraftingGrid component

- Add InventoryType, ContainerBacking, Container template

- Add VirtualContainerSlots component and CRAFTING_TABLE block

- Store inventory_type and backing in open container

- Add open() method to ContainerContext trait

- Add PlayerInfo::stub for bootstrap dispatches

- Add KnownRecipes component, RecipeContext trait, UnlockReason

- Delete basalt-core crate after full inlining into basalt-api


### Documentation

- Add contributing guide

- Add security policy

- Document event-first feature development workflow


### ECS

- Parallel system dispatch with rayon thread pool

- Integrate tick budgets with system dispatch and overrun logging

- Skip timing overhead when overrun detection is disabled

- Migrate from basalt-core to basalt-api


### Events

- Delete basalt-events crate

- Drop generic C parameter from EventBus


### Item Plugin

- Migrate from basalt-testkit to basalt-api testing feature


### Lifecycle Plugin

- Migrate from basalt-testkit to basalt-api testing feature


### Movement Plugin

- Migrate from basalt-testkit to basalt-api testing feature


### Networking

- Add packet-write benchmarks for buffer pool decision

- Reuse staging buffers on packet-write hot path

- Extract write_buffered helper from Stage 4

- Make ProtocolStream generic + add prod-path bench

- Pool read frame buffer to eliminate per-packet alloc


### Other

- Add basalt-recipes crate scaffold

- Add RecipePlugin with crafting table support

- Introduce RecipeId, Recipe enum, regenerate codegen ids

- Add find_by_id lookup on RecipeRegistry

- Migrate from basalt-testkit to basalt-api testing feature

- Bump internal crate version specifiers in make release


### Physics Plugin

- Migrate to typed SystemContext::resolve_movement

- Migrate from basalt-testkit to basalt-api testing feature

- Document basalt-api as standalone foundation crate


### Plugin API

- Add crafting events

- Expose container builder and open response

- Expose RecipeRegistry through PluginRegistrar

- Define 9 container and block entity events

- Add 4 crafting events and rename CraftingOutputClickedEvent

- Add context APIs and event fields for plugin migration

- Add recipe registry lifecycle events + RecipeRegistrar

- Add recipe-book lifecycle events + RecipeContext impl

- Add recipe-book fill-request and filled events

- Raw packet pre-dispatch hook for plugins

- Inline event-bus types and migrate consumers

- Fix stale macro references in EventRouting docs

- Convert command module to directory layout

- Add command args module content

- Add Command trait module content

- Add CommandRegistry module content

- Wire command module to local files

- Convert world module to directory layout

- Add collision module content

- Wire local collision module

- Convert inline components and system modules to file modules

- Add component sub-module files from basalt-core

- Add budget, gamemode, player, and testing files from basalt-core

- Wire local modules and drop basalt-core dependency

- Add typed world methods to WorldContext and SystemContext

- Migrate block plugin to typed WorldContext methods

- Remove world() from WorldContext and SystemContext traits

- Gate raw-packets behind feature flag

- Add testing feature with PluginTestHarness and SystemTestContext

- Introduce WorldHandle trait abstraction

- Make WorldContext and SystemContext extend WorldHandle

- Address Task 2 code review findings

- Use trait objects in PluginRegistrar

- Move block, block_entity to basalt-api and invert world dep

- Move recipe data types to basalt-api and invert recipes dep

- Add BlockEntityKind discriminator and dedupe with events

- Use re-exported NoopContext path in bus tests


### Protocol

- Hand-roll SlotDisplay/RecipeDisplay/RecipeBookEntry/IDSet

- Drop hand-rolled RecipeDisplay/SlotDisplay/RecipeBookEntry

- Cache encoded registry data payloads at first use


### Server

- Integrate parallel ECS dispatch with rayon thread pool

- Configurable per-system CPU budgets

- Click action parser and pure click handlers

- Server-authoritative inventory click dispatch

- Widen SetContainerSlot window_id to i32

- Crafting and container integration in game loop

- Remove unused mut in read_container_slot test

- Split match cycle into compute and sync helpers

- Dispatch shift-click batch and crafted events

- Migrate crafting cleanup to ContainerClosedEvent handler

- Build bootstrap context for plugin loading

- Wire recipe-book unlock/lock/on-join + ghost-recipe

- Auto-fill crafting grid on Place Recipe

- Make_all stacking on recipe-book auto-fill

- Use codegen recipe-book packets directly

- Implement chunk batch rate control protocol

- Drain initial chunks deterministically in e2e helper

- Make chunk batch e2e test deterministic

- Typed payload downcast on EncodablePacket

- Collapse messaging layer into byte-strategy taxonomy

- Migrate construction sites and tests to new taxonomy

- Per-player inbound packet rate limit

- Migrate play_handler.rs to basalt_api::command paths

- Migrate state.rs to basalt_api::command paths

- Drop basalt-command dep

- Migrate from basalt-core to basalt-api

- Remove unused ServerContext import in crafting tests

- Move ServerContext from basalt-api to basalt-server


### Storage

- Mark chunks dirty via block entity events

- Migrate to typed WorldContext::mark_chunk_dirty

- Migrate from basalt-testkit to basalt-api testing feature


### Test Kit

- Migrate command parser call to basalt_api::command path

- Drop basalt-command dep

- Migrate from basalt-core to basalt-api


### Types

- Blanket Encode/Decode/EncodedSize impls for Box<T>


### World

- Drop collision module

- Migrate from basalt-testkit to basalt-api testing feature


## [0.1.0] - 2026-04-20

### Chat Plugin

- Update architecture for single loop design


### Code Generation

- Add xtask codegen tool

- Add Configuration/Play states with category split

- Remove generated tests, auto-format with rustfmt, fix imports

- AnonOptionalNbt now generates NbtCompound instead of Option<NbtCompound>


### Commands

- Command registry crate with Command trait

- Command registry crate with Command trait

- Typed argument API with all Minecraft parsers and variants

- Broadcast /say message to all players instead of sender only

- Add typed ArgValue variants and Arg::token_count method

- Simplify /tp handler using typed get_vec3


### Container Plugin

- Add ContainerPlugin for chest interaction via events


### Core

- Basalt-core crate with Context trait and shared types

- Add Gamemode enum for type-safe gamemode handling

- Accept impl Display in PluginLogger instead of &str

- Add player_yaw/player_pitch to Context and preserve rotation in /tp

- Move component types and Phase from basalt-ecs to basalt-core

- Add SystemContext trait with typed entity access

- Add NoopContext shared test implementation

- Add typed broadcast methods to EntityContext

- Add PlayerInfo struct for dispatch context identity


### Derive Macros

- Add attribute parsing for packet, field, and variant

- Implement Encode derive for structs and enums

- Implement Decode derive for structs and enums

- Implement EncodedSize derive for structs and enums

- Add macro entry points and module docs

- Add attribute parsing for field and variant

- Implement Encode derive for structs and enums

- Implement Decode derive for structs and enums

- Implement EncodedSize derive for structs and enums

- Add #[packet] attribute macro

- Support field attributes in enum variant fields

- Add element = "varint" attribute for Vec fields

- Support element_varint attribute in enum variant fields

- Extract shared codegen helpers for field attributes

- Cap Vec::with_capacity in length_varint decode to prevent OOM


### Documentation

- Add project guidelines

- Document coverage job and 90% threshold

- Comprehensive update with codegen, coverage and workflow rules

- Add basalt-server crate to architecture and structure

- Add element attribute, multi-player architecture, helpers

- Add basalt-world crate to architecture

- Add basalt-events crate to architecture

- Document event-driven server architecture

- Update architecture for plugin API and plugin crates

- Add basalt-command crate and command plugin

- Update architecture for basalt-core and command args

- Update architecture for basalt-core and command args

- Document fuzz testing strategy and CI integration

- Add rule to keep README in sync with shipped features

- Add 5 new fuzz targets to CI matrices and documentation

- Document encoding architecture and world decoupling

- Document ChunkPacketCache LRU eviction config

- Document simulation distance and batch persistence

- Document drops plugin and item entities

- Update CLAUDE.md for new architecture

- Document &dyn Context handlers and ServerContext split

- Document DispatchResult, panic handling, PlayerInfo

- Generate initial changelog

- Add version badge to README


### ECS

- Add in-house Entity Component System with system scheduler

- Add SkinData and ChunkView components

- Move SkinData and ChunkView to server crate

- Add DroppedItem component for item entities

- Add PickupDelay component and Inventory::try_insert

- Expand Inventory to 36 slots with protocol conversion

- Make basalt-ecs a pure storage engine

- Update benchmarks for pure storage engine

- Provide World reference to SystemContext via set_world()


### Events

- Event bus crate with staged handler dispatch

- Rename BusKind::Network to BusKind::Instant


### Item Plugin

- Rename DropsPlugin to ItemPlugin


### Networking

- Add Error type

- Implement TCP framing

- Implement Connection typestate

- Implement AES-128 CFB-8 encryption

- Implement zlib compression

- Implement middleware pipeline

- Add Login state and HandshakeResult enum

- Add EncryptedStream with transparent encryption

- Use EncryptedStream in Connection

- Integrate zlib compression into EncryptedStream

- Rename EncryptedStream to ProtocolStream

- Add Configuration and Play typestates

- Update server example with full login → empty world flow

- Use typed Login packet instead of manual RawPayload encoding

- Add PacketWriter trait for testable packet output

- Limit login ack wait loop to prevent client-side DoS

- Use zlib compression level 3 instead of default 6

- Reuse encrypt buffer to avoid per-write allocation

- Log trailing bytes after Play packet decode

- Reject decompressed packets larger than 32 MB to prevent OOM

- Add dual event bus routing for two-loop architecture

- Update file structure for reorganized crates


### Other

- Add Cargo workspace with four crate skeletons

- Generate switch enums from protocol switch fields

- Thread global types through codegen pipeline

- Replace parser with IR-based TypeRegistry

- Skip relative-path switches and emit element varint

- Add missing world plugin crate and fix gitignore

- Add basalt-test-utils crate with PluginTestHarness

- Migrate all plugin tests to PluginTestHarness

- Add first benchmarks for VarInt, NBT, string, and chunk encoding

- Add fuzz targets for protocol type decoders

- Compare encoded bytes in NBT fuzz target instead of struct equality

- Add fuzz_packet_play target for serverbound Play packets

- Add fuzz_text_component target for NBT text parsing

- Add fuzz_position target for packed block coordinates

- Add fuzz_decompress target for zlib packet decompression

- Add fuzz_chunk_deserialize target for BSR chunk format

- Replace criterion with libtest native benchmarks

- Simplify plugin tests with PluginTestHarness

- Rename basalt-test-utils to basalt-testkit

- Improve PluginTestHarness API and clean tests

- Add drops plugin — spawn items on block break


### Physics Plugin

- Add physics plugin with gravity and AABB collision

- Migrate to SystemContext API


### Plugin API

- Public plugin API crate with ServerContext and Plugin trait

- Centralized command registration and DeclareCommands

- Implement Context trait, fluent command builder

- Use Gamemode enum instead of raw u8 in Context trait

- Add Debug and Clone derives to all event structs

- Change ServerContext world from &'static to Arc<World>

- Add system and component registration to PluginRegistrar

- Add block_state to BlockBrokenEvent and SpawnDroppedItem response

- Split Context trait into sub-context domains

- Add PlayerInteractEvent for block right-click

- Remove unsafe ecs_ptr from ServerContext

- Decouple basalt-api from basalt-ecs

- Organize prelude into focused modules

- Use basalt-api as sole plugin dependency

- Remove ComponentRegistration dead code

- Restructure events with domain files and structured types

- Use PlayerInfo in ServerContext and clean Response enum

- Split context.rs into domain sub-modules

- Plugin handlers receive &dyn Context instead of &ServerContext

- Migrate all plugin tests to DispatchResult


### Protocol

- Add Error type and ConnectionState enum

- Add ProtocolVersion enum

- Implement Handshake packets

- Implement PacketRegistry

- Replace hand-written Handshake packets with codegen

- Add Login dispatch to PacketRegistry

- Add generated Configuration and Play packets

- Add Configuration and Play dispatch

- Regenerate all packets without inline tests

- Add minimum registry data builder for Configuration

- Add empty chunk data builder for Play state

- Add all vanilla 1.21.4 damage types

- Add chat_type registry for Configuration

- Move chunk builder to basalt-server


### Server

- Implement Status packets

- Replace hand-written Status packets with codegen

- Add generated Login packets

- Add minimal server example

- Create basalt-server with play loop and packet dispatch

- Add chat system with commands

- Multi-player with shared state and broadcast

- Send 7x7 chunk grid with stone floor at y=99

- Fetch player skins from Mojang API

- Add server-specific Error type

- Fetch skin in parallel with configuration exchange

- Replace RwLock+mpsc with DashMap+broadcast channel

- Introduce PacketContext for handler abstraction

- Dynamic chunk streaming from basalt-world

- Use noise terrain with seed 42

- Block breaking and placing with creative inventory

- Add event types, context, and response queue

- Add ChatHandler plugin

- Add BlockInteractionHandler plugin

- Add movement, world, and lifecycle handler plugins

- Wire event bus into play loop and connection lifecycle

- Add StorageHandler plugin for chunk persistence

- Add built-in plugin crates under plugins/

- Use plugin crates and ServerContext from basalt-api

- Config-driven plugin registration via basalt.toml

- Structured logging with log crate and PluginLogger

- Add CommandPlugin with gameplay and admin commands

- Command dispatch, declare commands tree, and tab-complete

- Rewrite all commands with builder API and variants

- Handle broadcast channel lag instead of silently dropping

- Handle bind and accept errors gracefully instead of panicking

- Reuse shared reqwest::Client for skin fetching

- Remove unsafe lifetime extension for world reference

- Update plugin tests to use Arc<World> instead of OnceLock

- Add keep-alive timeout and client input validation

- Add [server.performance] config section with chunk_cache_max_entries

- Add project README

- Add tick loop, channels, and message types

- Implement two-loop architecture with dedicated threads

- Migrate game loop players to ECS entities

- Add Position and BoundingBox to player entities

- Sync player position to ECS on every movement packet

- Restructure messages and channels for single loop

- Absorb network loop into game loop

- Instant chat/commands in net task

- Rewire server startup and connections

- Reorganize into net/, game/, runtime/ modules

- Move packet encoding from game loop to net tasks

- Add LRU eviction to ChunkPacketCache

- Add simulation distance and batch chunk persistence

- Flush dirty chunks on graceful shutdown

- Item pickup system with collect animation

- Full inventory sync, window click, and item drop

- Chest containers — block entities, window protocol, persistence

- Chest visibility after restart, item drops, double chests, sneak

- Add EntityAction, CloseWindow, and BlockAction messages

- Encode BlockAction and route EntityAction/CloseWindow

- Chest fixes — visibility, drops, double chests, sneak, ContainerView

- Split game loop into 9 domain files

- Dispatch PlayerInteractEvent and remove inline chest logic

- Register ContainerPlugin in server config

- Split e2e tests into domain files

- Split net/task.rs into handler and sender

- Update for PlayerInfo and structured Response variants

- Handle game loop and plugin panics correctly

- Handle panics across all threads and tasks


### Storage

- Region format with lz4 compression

- Use sync_all instead of flush for crash-safe persistence

- Add region file compaction to reclaim dead space


### Test Kit

- Add SystemTestContext for system plugin tests

- Add DispatchResult to hide Response from plugin tests


### Types

- Add Error enum and Result type alias

- Add Encode, Decode, and EncodedSize traits

- Implement Encode/Decode/EncodedSize for primitive types

- Implement VarInt and VarLong

- Add comprehensive doc comments to primitives and varint

- Implement String encode/decode

- Implement ByteArray encode/decode

- Implement Position, BlockPosition, and ChunkPosition

- Implement UUID

- Implement Identifier

- Implement Angle

- Implement BitSet

- Add NBT tag types and compound data structures

- Implement NBT encode

- Implement NBT decode

- Implement TextComponent

- Add Slot, vector types, and Default on Position

- Resolve custom types, mapper and bitflags in codegen

- Add TextComponent::to_nbt() conversion method

- Add Error::Context variant for decode error context

- Add OpaqueBytes newtype for unparsed protocol data

- Reject negative VarInt lengths in string, byte array, and opaque decoders

- Decode Slot component counts instead of consuming all remaining bytes

- Use IndexMap for NbtCompound instead of Vec linear search

- Deprecate Identifier::as_str in favor of Display

- Reject negative item_count in Slot decode


### World

- Flat world generator with paletted containers

- Noise-based terrain generator with biome layers

- Integrate storage for chunk persistence

- Mutable block access and item-to-block mapping

- Separate block mutation from disk persistence

- Correct negative coordinate chunk calculation

- Use HashMap for palette lookup and add direct mode encoding

- Replace global Mutex with DashMap and LRU eviction

- Extract World struct from lib.rs

- Add dirty_chunks() for batch persistence

- Add block_state_to_item_id reverse mapping

- Add chest state helpers and fix block_state_to_item_id


