//! Player lifecycle — connect and disconnect handling.

use std::sync::Arc;

use basalt_api::events::{PlayerJoinedEvent, PlayerLeftEvent};
use basalt_protocol::packets::play::chat::ClientboundPlayDeclareCommands;
use basalt_protocol::packets::play::player::ClientboundPlayLogin;
use basalt_protocol::packets::play::player::ClientboundPlayLoginSpawninfo;
use basalt_types::{Position, Uuid};
use tokio::sync::mpsc;

use super::helpers::{send_player_info_add, send_spawn_entity};
use super::{ChunkStreamRate, ChunkView, GameLoop, OutputHandle, SkinData, VIEW_RADIUS};
use crate::messages::{EncodablePacket, ServerOutput, SharedBroadcast};

impl GameLoop {
    /// Handles a new player connection: spawn entity, send initial world, broadcast join.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_player_connected(
        &mut self,
        entity_id: i32,
        uuid: Uuid,
        username: String,
        skin_properties: Vec<basalt_core::broadcast::ProfileProperty>,
        position: (f64, f64, f64),
        yaw: f32,
        pitch: f32,
        output_tx: mpsc::Sender<ServerOutput>,
    ) {
        let eid = entity_id as basalt_ecs::EntityId;
        self.ecs.spawn_with_id(eid);
        self.ecs.set(
            eid,
            basalt_core::PlayerRef {
                uuid,
                username: username.clone(),
            },
        );
        self.ecs.set(
            eid,
            basalt_core::Position {
                x: position.0,
                y: position.1,
                z: position.2,
            },
        );
        self.ecs.set(eid, basalt_core::Rotation { yaw, pitch });
        self.ecs.set(
            eid,
            basalt_core::BoundingBox {
                width: 0.6,
                height: 1.8,
            },
        );
        self.ecs.set(eid, basalt_core::Inventory::empty());
        self.ecs.set(eid, basalt_core::CraftingGrid::empty());
        self.ecs
            .set(eid, basalt_core::components::KnownRecipes::default());
        self.ecs.set(
            eid,
            SkinData {
                properties: skin_properties.clone(),
            },
        );
        self.ecs.set(eid, ChunkView::empty());
        self.ecs
            .set(eid, ChunkStreamRate::new(self.chunk_batch_initial_rate));
        self.ecs.set(eid, OutputHandle { tx: output_tx });
        self.index_uuid(uuid, eid);

        // Send initial world data
        self.send_initial_world(eid, entity_id, position);

        // Initialise the recipe book — even an empty one is required;
        // the 1.21.4 client expects a `RecipeBookAdd { replace: true }`
        // packet on join to set up its book UI.
        self.send_initial_recipe_book(eid);

        // Send existing players to the new player + broadcast join
        let snapshot = basalt_api::broadcast::PlayerSnapshot {
            username: username.clone(),
            uuid,
            entity_id,
            x: position.0,
            y: position.1,
            z: position.2,
            yaw,
            pitch,
            skin_properties,
        };

        // Send self info to new player
        self.send_to(eid, |tx| send_player_info_add(tx, &snapshot));

        // Send all existing players to the new player, and broadcast join to them
        let other_eids: Vec<basalt_ecs::EntityId> = self
            .ecs
            .iter::<basalt_core::PlayerRef>()
            .filter(|&(id, _)| id != eid)
            .map(|(id, _)| id)
            .collect();

        for other_eid in &other_eids {
            // Build snapshot of existing player
            if let (Some(pr), Some(pos), Some(rot)) = (
                self.ecs.get::<basalt_core::PlayerRef>(*other_eid),
                self.ecs.get::<basalt_core::Position>(*other_eid),
                self.ecs.get::<basalt_core::Rotation>(*other_eid),
            ) {
                let skin = self
                    .ecs
                    .get::<SkinData>(*other_eid)
                    .map(|s| s.properties.clone())
                    .unwrap_or_default();
                let other_snapshot = basalt_api::broadcast::PlayerSnapshot {
                    username: pr.username.clone(),
                    uuid: pr.uuid,
                    entity_id: *other_eid as i32,
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                    yaw: rot.yaw,
                    pitch: rot.pitch,
                    skin_properties: skin,
                };
                // Send existing player info to new player
                self.send_to(eid, |tx| send_player_info_add(tx, &other_snapshot));
                self.send_to(eid, |tx| send_spawn_entity(tx, &other_snapshot));
            }

            // Send new player info to existing player
            self.send_to(*other_eid, |tx| send_player_info_add(tx, &snapshot));
            self.send_to(*other_eid, |tx| send_spawn_entity(tx, &snapshot));
            self.send_to(*other_eid, |tx| {
                let msg = basalt_types::TextComponent::text(format!("{username} joined the game"))
                    .color(basalt_types::TextColor::Named(
                        basalt_types::NamedColor::Yellow,
                    ));
                use basalt_protocol::packets::play::chat::ClientboundPlaySystemChat;
                let _ = tx.try_send(ServerOutput::plain(
                    ClientboundPlaySystemChat::PACKET_ID,
                    ClientboundPlaySystemChat {
                        content: msg.to_nbt(),
                        is_action_bar: false,
                    },
                ));
            });
        }

        // Welcome message
        self.send_to(eid, |tx| {
            let msg = basalt_types::TextComponent::text(format!("Welcome, {username}!")).color(
                basalt_types::TextColor::Named(basalt_types::NamedColor::Gold),
            );
            use basalt_protocol::packets::play::chat::ClientboundPlaySystemChat;
            let _ = tx.try_send(ServerOutput::plain(
                ClientboundPlaySystemChat::PACKET_ID,
                ClientboundPlaySystemChat {
                    content: msg.to_nbt(),
                    is_action_bar: false,
                },
            ));
        });

        // Dispatch PlayerJoinedEvent
        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
        let mut event = PlayerJoinedEvent;
        self.dispatch_event(&mut event, &ctx);
        self.process_responses(uuid, &ctx.drain_responses());
        self.rebuild_active_chunks();
    }

    /// Sends the initial world data to a newly connected player.
    pub(super) fn send_initial_world(
        &mut self,
        eid: basalt_ecs::EntityId,
        entity_id: i32,
        position: (f64, f64, f64),
    ) {
        // Login (Play) packet
        let login = ClientboundPlayLogin {
            entity_id,
            is_hardcore: false,
            world_names: vec!["minecraft:overworld".into()],
            max_players: 20,
            view_distance: 10,
            simulation_distance: 10,
            reduced_debug_info: false,
            enable_respawn_screen: true,
            do_limited_crafting: false,
            world_state: ClientboundPlayLoginSpawninfo {
                dimension: 0,
                name: "minecraft:overworld".into(),
                hashed_seed: 0,
                gamemode: 1,
                previous_gamemode: 255,
                is_debug: false,
                is_flat: true,
                death: None,
                portal_cooldown: 0,
                sea_level: 63,
            },
            enforces_secure_chat: false,
        };
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::Plain(EncodablePacket::new(
                ClientboundPlayLogin::PACKET_ID,
                login,
            )));
        });

        // DeclareCommands
        if !self.declare_commands.is_empty() {
            let dc = self.declare_commands.clone();
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::raw_owned(
                    ClientboundPlayDeclareCommands::PACKET_ID,
                    dc,
                ));
            });
        }

        // SpawnPosition
        let spawn_y = self.world.spawn_y() as i32;
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::Plain(EncodablePacket::new(
                basalt_protocol::packets::play::world::ClientboundPlaySpawnPosition::PACKET_ID,
                basalt_protocol::packets::play::world::ClientboundPlaySpawnPosition {
                    location: Position::new(0, spawn_y, 0),
                    angle: 0.0,
                },
            )));
        });

        // GameEvent (wait for chunks)
        self.send_to(eid, |tx| {
            use basalt_protocol::packets::play::player::ClientboundPlayGameStateChange;
            let _ = tx.try_send(ServerOutput::plain(
                ClientboundPlayGameStateChange::PACKET_ID,
                ClientboundPlayGameStateChange {
                    reason: 13,
                    game_mode: 0.0,
                },
            ));
        });

        // UpdateViewPosition + enqueue initial chunks for tick-paced sending.
        // The drainer wraps each batch in ChunkBatchStart/Finished and
        // marks chunks as loaded in `ChunkView` once they actually leave.
        let cx = (position.0 as i32) >> 4;
        let cz = (position.2 as i32) >> 4;
        self.send_to(eid, |tx| {
            use basalt_protocol::packets::play::world::ClientboundPlayUpdateViewPosition;
            let _ = tx.try_send(ServerOutput::plain(
                ClientboundPlayUpdateViewPosition::PACKET_ID,
                ClientboundPlayUpdateViewPosition {
                    chunk_x: cx,
                    chunk_z: cz,
                },
            ));
        });

        if let Some(rate) = self.ecs.get_mut::<ChunkStreamRate>(eid) {
            for dx in -VIEW_RADIUS..=VIEW_RADIUS {
                for dz in -VIEW_RADIUS..=VIEW_RADIUS {
                    rate.pending.push_back((cx + dx, cz + dz));
                }
            }
        }

        // Position
        self.send_to(eid, |tx| {
            use basalt_protocol::packets::play::player::ClientboundPlayPosition;
            let _ = tx.try_send(ServerOutput::plain(
                ClientboundPlayPosition::PACKET_ID,
                ClientboundPlayPosition {
                    teleport_id: 1,
                    x: position.0,
                    y: position.1,
                    z: position.2,
                    dx: 0.0,
                    dy: 0.0,
                    dz: 0.0,
                    yaw: 0.0,
                    pitch: 0.0,
                    flags: 0,
                },
            ));
        });

        // Sync full inventory
        if let Some(inv) = self.ecs.get::<basalt_core::Inventory>(eid) {
            let protocol_slots = inv.to_protocol_slots();
            self.send_to(eid, |tx| {
                use basalt_protocol::packets::play::inventory::ClientboundPlayWindowItems;
                let _ = tx.try_send(ServerOutput::plain(
                    ClientboundPlayWindowItems::PACKET_ID,
                    ClientboundPlayWindowItems {
                        window_id: 0,
                        state_id: 0,
                        items: protocol_slots,
                        carried_item: basalt_types::Slot::empty(),
                    },
                ));
            });
        }
    }

    /// Handles a player disconnection.
    pub(super) fn handle_player_disconnected(&mut self, uuid: Uuid) {
        let Some(eid) = self.find_by_uuid(uuid) else {
            return;
        };

        let (entity_id, username) = {
            let Some(pr) = self.ecs.get::<basalt_core::PlayerRef>(eid) else {
                return;
            };
            (eid as i32, pr.username.clone())
        };

        // Dispatch ContainerClosedEvent before despawn (if container is open)
        if self.ecs.has::<basalt_core::OpenContainer>(eid) {
            // Snapshot crafting grid for the disconnect path so plugins
            // can drop items even when the player crashes out.
            let crafting_grid_state = if matches!(
                self.ecs
                    .get::<basalt_core::OpenContainer>(eid)
                    .map(|oc| oc.inventory_type),
                Some(basalt_core::InventoryType::Crafting)
            ) {
                self.ecs
                    .get::<basalt_core::CraftingGrid>(eid)
                    .map(|g| g.slots.clone())
            } else {
                None
            };
            self.dispatch_container_closed(
                eid,
                uuid,
                basalt_api::events::CloseReason::Disconnect,
                crafting_grid_state,
            );
        }

        // Dispatch PlayerLeftEvent
        let ctx = self.make_context(uuid, entity_id, &username, 0.0, 0.0);
        let mut event = PlayerLeftEvent;
        self.dispatch_event(&mut event, &ctx);
        self.process_responses(uuid, &ctx.drain_responses());

        self.ecs.despawn(eid);
        self.remove_uuid(uuid);
        self.rebuild_active_chunks();

        // Broadcast leave to remaining players
        use basalt_protocol::packets::play::chat::ClientboundPlaySystemChat;
        use basalt_protocol::packets::play::entity::ClientboundPlayEntityDestroy;
        use basalt_protocol::packets::play::player::ClientboundPlayPlayerRemove;
        let remove_players = Arc::new(SharedBroadcast::single(
            ClientboundPlayPlayerRemove::PACKET_ID,
            ClientboundPlayPlayerRemove {
                players: vec![uuid],
            },
        ));
        let remove_entities = Arc::new(SharedBroadcast::single(
            ClientboundPlayEntityDestroy::PACKET_ID,
            ClientboundPlayEntityDestroy {
                entity_ids: vec![entity_id],
            },
        ));
        let msg = basalt_types::TextComponent::text(format!("{username} left the game")).color(
            basalt_types::TextColor::Named(basalt_types::NamedColor::Yellow),
        );
        let leave_chat = Arc::new(SharedBroadcast::single(
            ClientboundPlaySystemChat::PACKET_ID,
            ClientboundPlaySystemChat {
                content: msg.to_nbt(),
                is_action_bar: false,
            },
        ));

        for (other_eid, _) in self.ecs.iter::<OutputHandle>() {
            self.send_to(other_eid, |tx| {
                let _ = tx.try_send(ServerOutput::Cached(Arc::clone(&remove_players)));
                let _ = tx.try_send(ServerOutput::Cached(Arc::clone(&remove_entities)));
                let _ = tx.try_send(ServerOutput::Cached(Arc::clone(&leave_chat)));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use basalt_types::Uuid;

    use crate::messages::{GameInput, ServerOutput};

    #[test]
    fn player_connect_and_disconnect() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        assert!(game_loop.find_by_uuid(uuid).is_some());

        let _ = game_tx.send(GameInput::PlayerDisconnected { uuid });
        game_loop.tick(1);
        assert!(game_loop.find_by_uuid(uuid).is_none());
    }

    #[test]
    fn player_connect_sends_initial_world() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert!(count > 10, "expected many initial packets, got {count}");
    }

    #[test]
    fn player_connect_creates_all_components() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        assert!(game_loop.ecs.has::<basalt_core::Position>(eid));
        assert!(game_loop.ecs.has::<basalt_core::Rotation>(eid));
        assert!(game_loop.ecs.has::<basalt_core::BoundingBox>(eid));
        assert!(game_loop.ecs.has::<basalt_core::Inventory>(eid));
        assert!(game_loop.ecs.has::<basalt_core::CraftingGrid>(eid));
        assert!(game_loop.ecs.has::<basalt_core::PlayerRef>(eid));
        assert!(game_loop.ecs.has::<super::super::SkinData>(eid));
        assert!(game_loop.ecs.has::<super::super::ChunkView>(eid));
        assert!(game_loop.ecs.has::<super::super::OutputHandle>(eid));
    }

    #[test]
    fn player_connect_initializes_crafting_grid() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        let grid = game_loop
            .ecs
            .get::<basalt_core::CraftingGrid>(eid)
            .expect("CraftingGrid should be initialized on connect");
        assert_eq!(grid.grid_size, 2, "default grid should be 2x2");
        for slot in &grid.slots {
            assert!(slot.is_empty(), "grid slots should be empty on connect");
        }
        assert!(grid.output.is_empty(), "output should be empty on connect");
    }

    #[test]
    fn player_connect_syncs_inventory() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        use basalt_protocol::packets::play::inventory::ClientboundPlayWindowItems;
        let mut got_sync = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, ServerOutput::Plain(ep) if ep.id() == ClientboundPlayWindowItems::PACKET_ID)
            {
                got_sync = true;
            }
        }
        assert!(got_sync, "should receive SyncInventory on connect");
    }

    #[test]
    fn two_players_join_broadcast() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid1 = Uuid::from_bytes([1; 16]);
        let uuid2 = Uuid::from_bytes([2; 16]);
        let mut rx1 = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid1, 1);

        while rx1.try_recv().is_ok() {}

        let _rx2 = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid2, 2);

        let mut p1_count = 0;
        while rx1.try_recv().is_ok() {
            p1_count += 1;
        }
        assert!(
            p1_count >= 3,
            "player 1 should receive join broadcast, got {p1_count} packets"
        );
    }

    #[test]
    fn player_disconnect_broadcasts_leave() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid1 = Uuid::from_bytes([1; 16]);
        let uuid2 = Uuid::from_bytes([2; 16]);
        let mut rx1 = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid1, 1);
        let _rx2 = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid2, 2);

        while rx1.try_recv().is_ok() {}

        let _ = game_tx.send(GameInput::PlayerDisconnected { uuid: uuid2 });
        game_loop.tick(2);

        let mut p1_count = 0;
        while rx1.try_recv().is_ok() {
            p1_count += 1;
        }
        assert!(
            p1_count >= 3,
            "player 1 should receive leave broadcast, got {p1_count}"
        );
    }
}
