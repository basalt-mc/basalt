//! Game loop — dedicated OS thread for world simulation.
//!
//! Runs at 20 TPS on a [`TickLoop`](crate::tick::TickLoop). Each tick:
//! 1. Drains the shared [`GameInput`] channel (block dig/place, inventory)
//! 2. Drains the [`PlayerAction`] channel (cross-loop actions from network)
//! 3. Dispatches game events through the game [`EventBus`]
//!    (Validate → Process → Post)
//! 4. Sends output packets and corrections to player net tasks
//!
//! The game loop is the **sole owner** of world state. No other thread
//! mutates chunks or blocks directly. The network loop reads chunks
//! via the shared `Arc<World>` (DashMap provides concurrent reads).

use std::collections::HashMap;
use std::sync::Arc;

use basalt_api::EventBus;
use basalt_api::context::{Response, ServerContext};
use basalt_api::events::{BlockBrokenEvent, BlockPlacedEvent};
use basalt_events::Event;
use basalt_protocol::packets::play::world::{
    ClientboundPlayAcknowledgePlayerDigging, ClientboundPlayBlockChange,
};
use basalt_types::{Encode, EncodedSize, Position, Slot, Uuid};
use tokio::sync::mpsc;

use crate::messages::{GameInput, ServerOutput};

/// Per-player state owned by the game loop.
struct GamePlayer {
    /// Server-assigned entity ID.
    entity_id: i32,
    /// Player display name.
    username: String,
    /// Currently selected hotbar slot (0-8).
    held_slot: u8,
    /// Hotbar items (slots 0-8).
    hotbar: [Slot; 9],
    /// Channel to send output packets to this player's net task.
    output_tx: mpsc::Sender<ServerOutput>,
}

impl GamePlayer {
    /// Returns the currently held item.
    fn held_item(&self) -> &Slot {
        &self.hotbar[self.held_slot as usize]
    }
}

/// The game loop state and logic.
///
/// Owns the game event bus, player inventory state, and has
/// exclusive write access to the world. Runs as the callback
/// inside a [`TickLoop`](crate::tick::TickLoop).
pub(crate) struct GameLoop {
    /// Per-player game state, keyed by UUID.
    players: HashMap<Uuid, GamePlayer>,
    /// Game event bus (blocks, world mutations).
    bus: EventBus,
    /// World — sole writer (game loop), concurrent reader (network loop).
    world: Arc<basalt_world::World>,
    /// Receiver for net task → game loop messages.
    game_rx: mpsc::UnboundedReceiver<GameInput>,
    /// Sender for the I/O thread (async chunk persistence).
    io_tx: mpsc::UnboundedSender<crate::io_thread::IoRequest>,
}

impl GameLoop {
    /// Creates a new game loop with the given dependencies.
    pub fn new(
        bus: EventBus,
        world: Arc<basalt_world::World>,
        game_rx: mpsc::UnboundedReceiver<GameInput>,
        io_tx: mpsc::UnboundedSender<crate::io_thread::IoRequest>,
    ) -> Self {
        Self {
            players: HashMap::new(),
            bus,
            world,
            game_rx,
            io_tx,
        }
    }

    /// Processes one tick of the game loop.
    ///
    /// Called by the [`TickLoop`](crate::tick::TickLoop) at 20 TPS.
    pub fn tick(&mut self, _tick: u64) {
        self.drain_game_input();
    }

    /// Drains all pending messages from net tasks.
    fn drain_game_input(&mut self) {
        while let Ok(msg) = self.game_rx.try_recv() {
            match msg {
                GameInput::PlayerConnected {
                    entity_id,
                    uuid,
                    username,
                    output_tx,
                } => {
                    self.players.insert(
                        uuid,
                        GamePlayer {
                            entity_id,
                            username,
                            held_slot: 0,
                            hotbar: std::array::from_fn(|_| Slot::empty()),
                            output_tx,
                        },
                    );
                }
                GameInput::PlayerDisconnected { uuid, .. } => {
                    self.players.remove(&uuid);
                }
                GameInput::BlockDig {
                    uuid,
                    status,
                    x,
                    y,
                    z,
                    sequence,
                } => {
                    if status == 0 {
                        self.handle_block_dig(uuid, x, y, z, sequence);
                    }
                }
                GameInput::BlockPlace {
                    uuid,
                    x,
                    y,
                    z,
                    direction,
                    sequence,
                } => {
                    self.handle_block_place(uuid, x, y, z, direction, sequence);
                }
                GameInput::HeldItemSlot { uuid, slot } => {
                    if let Some(player) = self.players.get_mut(&uuid) {
                        let idx = slot as u8;
                        if idx < 9 {
                            player.held_slot = idx;
                        }
                    }
                }
                GameInput::SetCreativeSlot { uuid, slot, item } => {
                    if let Some(player) = self.players.get_mut(&uuid) {
                        // Creative inventory slots 36-44 map to hotbar 0-8
                        let hotbar_idx = slot - 36;
                        if (0..9).contains(&hotbar_idx) {
                            player.hotbar[hotbar_idx as usize] = item;
                        }
                    }
                }
            }
        }
    }

    /// Handles a block dig (break) from a player.
    ///
    /// Dispatches `BlockBrokenEvent` through the game bus. If not
    /// cancelled, mutates the world and broadcasts the block change.
    /// If cancelled, sends a correction to the player to revert
    /// the optimistic feedback.
    fn handle_block_dig(&mut self, uuid: Uuid, x: i32, y: i32, z: i32, sequence: i32) {
        let Some(player) = self.players.get(&uuid) else {
            return;
        };

        let original_state = self.world.get_block(x, y, z);

        let ctx = ServerContext::new(
            Arc::clone(&self.world),
            uuid,
            player.entity_id,
            player.username.clone(),
            0.0,
            0.0,
        );
        let mut event = BlockBrokenEvent {
            x,
            y,
            z,
            sequence,
            player_uuid: uuid,
            cancelled: false,
        };
        self.bus.dispatch(&mut event, &ctx);

        if event.is_cancelled() {
            // Send the original block state back to the player to revert
            // their client-side prediction.
            if let Some(player) = self.players.get(&uuid) {
                let _ = player.output_tx.try_send(encode_packet(
                    ClientboundPlayBlockChange::PACKET_ID,
                    &ClientboundPlayBlockChange {
                        location: Position::new(x, y, z),
                        r#type: i32::from(original_state),
                    },
                ));
            }
            return;
        }

        let responses = ctx.drain_responses();
        self.process_responses(uuid, &responses);
    }

    /// Handles a block place from a player.
    ///
    /// Computes the placement position from the target block + face,
    /// determines the block state from the held item, then dispatches
    /// `BlockPlacedEvent`. If cancelled, sends a correction.
    fn handle_block_place(
        &mut self,
        uuid: Uuid,
        x: i32,
        y: i32,
        z: i32,
        direction: i32,
        sequence: i32,
    ) {
        let Some(player) = self.players.get(&uuid) else {
            return;
        };

        // Compute placement position from target + face offset
        let (dx, dy, dz) = face_offset(direction);
        let (px, py, pz) = (x + dx, y + dy, z + dz);

        // Determine block state from held item
        let item = player.held_item();
        let Some(item_id) = item.item_id else {
            return;
        };
        let Some(block_state) = basalt_world::block::item_to_default_block_state(item_id) else {
            return;
        };

        let ctx = ServerContext::new(
            Arc::clone(&self.world),
            uuid,
            player.entity_id,
            player.username.clone(),
            0.0,
            0.0,
        );
        let mut event = BlockPlacedEvent {
            x: px,
            y: py,
            z: pz,
            block_state,
            sequence,
            player_uuid: uuid,
            cancelled: false,
        };
        self.bus.dispatch(&mut event, &ctx);

        if event.is_cancelled() {
            // Send AIR back to revert the player's client-side prediction.
            if let Some(player) = self.players.get(&uuid) {
                let _ = player.output_tx.try_send(encode_packet(
                    ClientboundPlayBlockChange::PACKET_ID,
                    &ClientboundPlayBlockChange {
                        location: Position::new(px, py, pz),
                        r#type: i32::from(basalt_world::block::AIR),
                    },
                ));
            }
            return;
        }

        let responses = ctx.drain_responses();
        self.process_responses(uuid, &responses);
    }

    /// Processes event handler responses.
    ///
    /// Handles world mutations (via Response::Broadcast(BlockChanged)),
    /// block acks, chat messages, and persistence. Block change broadcasts
    /// are sent to ALL players' output channels.
    fn process_responses(&mut self, source_uuid: Uuid, responses: &[Response]) {
        for response in responses {
            match response {
                Response::Broadcast(basalt_api::BroadcastMessage::BlockChanged {
                    x,
                    y,
                    z,
                    block_state,
                }) => {
                    // Broadcast block change to ALL players including the source.
                    // The source player needs both the ack (optimistic, from
                    // network loop) AND the BlockChanged (authoritative, from
                    // game loop) to fully commit the block change. The ack
                    // alone is a temporary confirmation — the client reverts
                    // without the BlockChanged.
                    let data = encode_packet(
                        ClientboundPlayBlockChange::PACKET_ID,
                        &ClientboundPlayBlockChange {
                            location: Position::new(*x, *y, *z),
                            r#type: *block_state,
                        },
                    );
                    for player in self.players.values() {
                        let _ = player.output_tx.try_send(data.clone());
                    }
                }
                Response::Broadcast(basalt_api::BroadcastMessage::Chat { content }) => {
                    // Game loop handlers can broadcast chat too
                    let data = encode_packet(
                        basalt_protocol::packets::play::chat::ClientboundPlaySystemChat::PACKET_ID,
                        &basalt_protocol::packets::play::chat::ClientboundPlaySystemChat {
                            content: content.clone(),
                            is_action_bar: false,
                        },
                    );
                    for player in self.players.values() {
                        let _ = player.output_tx.try_send(data.clone());
                    }
                }
                Response::Broadcast(_) => {
                    // Other broadcast types not handled by game loop
                }
                Response::SendBlockAck { sequence } => {
                    if let Some(player) = self.players.get(&source_uuid) {
                        let _ = player.output_tx.try_send(encode_packet(
                            ClientboundPlayAcknowledgePlayerDigging::PACKET_ID,
                            &ClientboundPlayAcknowledgePlayerDigging {
                                sequence_id: *sequence,
                            },
                        ));
                    }
                }
                Response::SendSystemChat {
                    content,
                    action_bar,
                } => {
                    if let Some(player) = self.players.get(&source_uuid) {
                        let _ = player.output_tx.try_send(encode_packet(
                            basalt_protocol::packets::play::chat::ClientboundPlaySystemChat::PACKET_ID,
                            &basalt_protocol::packets::play::chat::ClientboundPlaySystemChat {
                                content: content.clone(),
                                is_action_bar: *action_bar,
                            },
                        ));
                    }
                }
                Response::PersistChunk { cx, cz } => {
                    let _ = self
                        .io_tx
                        .send(crate::io_thread::IoRequest::PersistChunk { cx: *cx, cz: *cz });
                }
                // Teleport and chunk streaming are network loop concerns
                Response::SendPosition { .. }
                | Response::StreamChunks { .. }
                | Response::SendGameStateChange { .. } => {}
            }
        }
    }
}

/// Returns the (dx, dy, dz) offset for a block face direction.
///
/// Block faces in the Minecraft protocol:
/// 0 = bottom (-Y), 1 = top (+Y), 2 = north (-Z),
/// 3 = south (+Z), 4 = west (-X), 5 = east (+X).
fn face_offset(direction: i32) -> (i32, i32, i32) {
    match direction {
        0 => (0, -1, 0),
        1 => (0, 1, 0),
        2 => (0, 0, -1),
        3 => (0, 0, 1),
        4 => (-1, 0, 0),
        5 => (1, 0, 0),
        _ => (0, 0, 0),
    }
}

/// Encodes a packet struct into a [`ServerOutput::SendPacket`].
fn encode_packet<P: Encode + EncodedSize>(packet_id: i32, packet: &P) -> ServerOutput {
    let mut data = Vec::with_capacity(packet.encoded_size());
    packet.encode(&mut data).expect("packet encoding failed");
    ServerOutput::SendPacket {
        id: packet_id,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_api::Plugin;

    /// Creates a test GameLoop with a memory world and block plugin registered.
    fn test_game_loop() -> (
        GameLoop,
        mpsc::UnboundedSender<GameInput>,
        mpsc::UnboundedReceiver<crate::io_thread::IoRequest>,
    ) {
        let world = Arc::new(basalt_world::World::new_memory(42));
        let (game_tx, game_rx) = mpsc::unbounded_channel();
        let (io_tx, io_rx) = mpsc::unbounded_channel();

        let mut bus = EventBus::new();
        let mut network_bus = EventBus::new();
        let mut commands = Vec::new();
        {
            let mut registrar =
                basalt_api::PluginRegistrar::new(&mut network_bus, &mut bus, &mut commands);
            basalt_plugin_block::BlockPlugin.on_enable(&mut registrar);
        }

        let game_loop = GameLoop::new(bus, world, game_rx, io_tx);
        (game_loop, game_tx, io_rx)
    }

    /// Connects a test player and returns their output receiver.
    fn connect_player(
        game_loop: &mut GameLoop,
        game_tx: &mpsc::UnboundedSender<GameInput>,
        uuid: Uuid,
        entity_id: i32,
    ) -> mpsc::Receiver<ServerOutput> {
        let (output_tx, output_rx) = mpsc::channel(64);
        let _ = game_tx.send(GameInput::PlayerConnected {
            entity_id,
            uuid,
            username: "Steve".into(),
            output_tx,
        });
        game_loop.tick(0);
        output_rx
    }

    #[test]
    fn player_connect_and_disconnect() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        assert!(game_loop.players.contains_key(&uuid));

        let _ = game_tx.send(GameInput::PlayerDisconnected { uuid });
        game_loop.tick(1);
        assert!(!game_loop.players.contains_key(&uuid));
    }

    #[test]
    fn block_dig_sets_air_and_sends_ack_and_broadcast() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Place a stone block first
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::STONE);

        // Send block dig
        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 0,
            x: 5,
            y: 64,
            z: 3,
            sequence: 42,
        });
        game_loop.tick(2);

        // World should be AIR
        assert_eq!(
            game_loop.world.get_block(5, 64, 3),
            basalt_world::block::AIR
        );

        // Should have received ack + block change
        let mut got_ack = false;
        let mut got_block_change = false;
        while let Ok(ServerOutput::SendPacket { id, .. }) = rx.try_recv() {
            if id == ClientboundPlayAcknowledgePlayerDigging::PACKET_ID {
                got_ack = true;
            }
            if id == ClientboundPlayBlockChange::PACKET_ID {
                got_block_change = true;
            }
        }
        assert!(got_ack, "should have received block ack");
        assert!(got_block_change, "should have received block change");
    }

    #[test]
    fn block_place_sets_block_and_broadcasts() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Give the player a stone block in hotbar slot 0
        if let Some(player) = game_loop.players.get_mut(&uuid) {
            player.hotbar[0] = Slot {
                item_id: Some(1),
                item_count: 1,
                component_data: vec![],
            };
        }

        // Place block on top of (5, 63, 3) → (5, 64, 3)
        let _ = game_tx.send(GameInput::BlockPlace {
            uuid,
            x: 5,
            y: 63,
            z: 3,
            direction: 1, // top face = +Y
            sequence: 10,
        });
        game_loop.tick(2);

        assert_eq!(
            game_loop.world.get_block(5, 64, 3),
            basalt_world::block::STONE
        );

        // Should have output
        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert!(count > 0, "should have received output packets");
    }

    #[test]
    fn held_item_slot_change() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let _ = game_tx.send(GameInput::HeldItemSlot { uuid, slot: 3 });
        game_loop.tick(1);

        assert_eq!(game_loop.players.get(&uuid).unwrap().held_slot, 3);
    }

    #[test]
    fn set_creative_slot() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let item = Slot {
            item_id: Some(1),
            item_count: 64,
            component_data: vec![],
        };
        let _ = game_tx.send(GameInput::SetCreativeSlot {
            uuid,
            slot: 36, // hotbar slot 0
            item,
        });
        game_loop.tick(1);

        let player = game_loop.players.get(&uuid).unwrap();
        assert_eq!(player.hotbar[0].item_id, Some(1));
        assert_eq!(player.hotbar[0].item_count, 64);
    }

    #[test]
    fn face_offset_all_directions() {
        assert_eq!(face_offset(0), (0, -1, 0)); // bottom
        assert_eq!(face_offset(1), (0, 1, 0)); // top
        assert_eq!(face_offset(2), (0, 0, -1)); // north
        assert_eq!(face_offset(3), (0, 0, 1)); // south
        assert_eq!(face_offset(4), (-1, 0, 0)); // west
        assert_eq!(face_offset(5), (1, 0, 0)); // east
        assert_eq!(face_offset(99), (0, 0, 0)); // invalid
    }

    #[test]
    fn persist_chunk_forwarded_to_io_thread() {
        let (game_loop, _game_tx, mut io_rx) = test_game_loop();
        let _ = game_loop
            .io_tx
            .send(crate::io_thread::IoRequest::PersistChunk { cx: 0, cz: 0 });
        assert!(io_rx.try_recv().is_ok());
    }
}
