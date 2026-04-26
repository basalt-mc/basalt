//! Item entities — spawning, movement broadcasting, pickup, and lifetime expiry.

use std::sync::Arc;

use basalt_mc_protocol::packets::play::entity::{
    ClientboundPlayCollect, ClientboundPlayEntityDestroy, ClientboundPlayEntityHeadRotation,
    ClientboundPlayEntityMetadata, ClientboundPlaySpawnEntity, ClientboundPlaySyncEntityPosition,
};
use basalt_types::{Encode, Uuid, VarInt, Vec3i16};

use super::{GameLoop, OutputHandle};
use crate::helpers::angle_to_byte;
use crate::messages::{EncodablePacket, ServerOutput, SharedBroadcast};

/// Encodes entity metadata entries for a dropped item entity.
///
/// Produces the raw metadata bytes (without entity ID — that's in
/// the [`ClientboundPlayEntityMetadata`] struct):
/// - Index 8, type 7 (Slot), value = item slot
/// - 0xFF terminator
fn encode_item_metadata(item_id: i32, count: i32) -> Vec<u8> {
    let mut buf = Vec::new();
    8u8.encode(&mut buf).unwrap();
    VarInt(7).encode(&mut buf).unwrap();
    let slot = basalt_types::Slot::new(item_id, count);
    slot.encode(&mut buf).unwrap();
    0xFFu8.encode(&mut buf).unwrap();
    buf
}

impl GameLoop {
    /// Spawns a dropped item entity and broadcasts it to all players.
    pub(super) fn spawn_item_entity(&mut self, x: i32, y: i32, z: i32, item_id: i32, count: i32) {
        use std::sync::atomic::Ordering;

        let entity_id = self.next_entity_id.fetch_add(1, Ordering::Relaxed);
        let eid = entity_id as basalt_ecs::EntityId;

        // Small random offset so items don't stack perfectly
        let px = x as f64 + 0.5;
        let py = y as f64 + 0.25;
        let pz = z as f64 + 0.5;

        self.ecs.spawn_with_id(eid);
        self.ecs.set(
            eid,
            basalt_api::components::Position {
                x: px,
                y: py,
                z: pz,
            },
        );
        self.ecs.set(
            eid,
            basalt_api::components::Velocity {
                dx: 0.0,
                dy: 0.2,
                dz: 0.0,
            },
        );
        self.ecs.set(
            eid,
            basalt_api::components::BoundingBox {
                width: 0.25,
                height: 0.25,
            },
        );
        self.ecs
            .set(eid, basalt_api::components::EntityKind { type_id: 68 });
        self.ecs.set(
            eid,
            basalt_api::components::PickupDelay {
                remaining_ticks: 10,
            },
        );
        self.ecs.set(
            eid,
            basalt_api::components::Lifetime {
                remaining_ticks: 6000,
            },
        );
        self.ecs.set(
            eid,
            basalt_api::components::DroppedItem {
                slot: basalt_types::Slot::new(item_id, count),
            },
        );

        // Broadcast spawn to all players: SpawnEntity (item type 68) +
        // EntityMetadata carrying the item slot.
        let bc = Arc::new(SharedBroadcast::new(vec![
            EncodablePacket::new(
                ClientboundPlaySpawnEntity::PACKET_ID,
                ClientboundPlaySpawnEntity {
                    entity_id,
                    object_uuid: Uuid::from_bytes((entity_id as u128).to_le_bytes()),
                    r#type: 68, // item entity in 1.21.4
                    x: px,
                    y: py,
                    z: pz,
                    pitch: 0,
                    yaw: 0,
                    head_pitch: 0,
                    object_data: 0,
                    velocity: Vec3i16 {
                        x: 0,
                        y: (0.2 * 8000.0) as i16,
                        z: 0,
                    },
                },
            ),
            EncodablePacket::new(
                ClientboundPlayEntityMetadata::PACKET_ID,
                ClientboundPlayEntityMetadata {
                    entity_id,
                    metadata: encode_item_metadata(item_id, count),
                },
            ),
        ]));
        for (other_eid, _) in self.ecs.iter::<OutputHandle>() {
            self.send_to(other_eid, |tx| {
                let _ = tx.try_send(ServerOutput::Cached(Arc::clone(&bc)));
            });
        }
    }

    /// Broadcasts position updates for non-player entities that have velocity.
    ///
    /// After physics runs (via ecs.run_all), entities may have moved.
    /// Players get movement broadcast via handle_movement, but non-player
    /// entities (dropped items) need separate broadcasts.
    pub(super) fn broadcast_item_movement(&mut self) {
        let moving: Vec<(basalt_ecs::EntityId, f64, f64, f64)> = self
            .ecs
            .iter::<basalt_api::components::DroppedItem>()
            .filter_map(|(eid, _)| {
                let vel = self.ecs.get::<basalt_api::components::Velocity>(eid)?;
                // Only broadcast if actually moving
                if vel.dx.abs() < 0.001 && vel.dy.abs() < 0.001 && vel.dz.abs() < 0.001 {
                    return None;
                }
                let pos = self.ecs.get::<basalt_api::components::Position>(eid)?;
                Some((eid, pos.x, pos.y, pos.z))
            })
            .collect();

        if moving.is_empty() {
            return;
        }

        for (eid, x, y, z) in moving {
            let entity_id = eid as i32;
            let bc = Arc::new(SharedBroadcast::new(vec![
                EncodablePacket::new(
                    ClientboundPlaySyncEntityPosition::PACKET_ID,
                    ClientboundPlaySyncEntityPosition {
                        entity_id,
                        x,
                        y,
                        z,
                        dx: 0.0,
                        dy: 0.0,
                        dz: 0.0,
                        yaw: 0.0,
                        pitch: 0.0,
                        on_ground: false,
                    },
                ),
                EncodablePacket::new(
                    ClientboundPlayEntityHeadRotation::PACKET_ID,
                    ClientboundPlayEntityHeadRotation {
                        entity_id,
                        head_yaw: angle_to_byte(0.0),
                    },
                ),
            ]));
            for (player_eid, _) in self.ecs.iter::<OutputHandle>() {
                self.send_to(player_eid, |tx| {
                    let _ = tx.try_send(ServerOutput::Cached(Arc::clone(&bc)));
                });
            }
        }
    }

    /// Checks proximity between item entities and players, picks up items.
    ///
    /// For each dropped item with an expired pickup delay, checks distance
    /// to all players. If within 1.5 blocks and the player's inventory has
    /// space, the item is transferred, a collect animation is broadcast,
    /// and the item entity is despawned.
    pub(super) fn tick_item_pickup(&mut self) {
        // Collect all item entities eligible for pickup
        let items: Vec<(basalt_ecs::EntityId, f64, f64, f64, i32, i32)> = self
            .ecs
            .iter::<basalt_api::components::DroppedItem>()
            .filter_map(|(eid, item)| {
                // Skip items still on pickup delay
                if let Some(delay) = self.ecs.get::<basalt_api::components::PickupDelay>(eid)
                    && delay.remaining_ticks > 0
                {
                    return None;
                }
                let pos = self.ecs.get::<basalt_api::components::Position>(eid)?;
                let item_id = item.slot.item_id?;
                Some((eid, pos.x, pos.y, pos.z, item_id, item.slot.item_count))
            })
            .collect();

        if items.is_empty() {
            return;
        }

        // Collect all players
        let players: Vec<(basalt_ecs::EntityId, f64, f64, f64)> = self
            .ecs
            .iter::<basalt_api::components::PlayerRef>()
            .filter_map(|(eid, _)| {
                let pos = self.ecs.get::<basalt_api::components::Position>(eid)?;
                Some((eid, pos.x, pos.y, pos.z))
            })
            .collect();

        const PICKUP_RADIUS_SQ: f64 = 1.5 * 1.5;

        for (item_eid, ix, iy, iz, item_id, count) in &items {
            for (player_eid, px, py, pz) in &players {
                let dx = ix - px;
                let dy = iy - py;
                let dz = iz - pz;
                let dist_sq = dx * dx + dy * dy + dz * dz;

                if dist_sq > PICKUP_RADIUS_SQ {
                    continue;
                }

                // Try to insert into player inventory
                let (inv_idx, slot_after) = {
                    let Some(inv) = self
                        .ecs
                        .get_mut::<basalt_api::components::Inventory>(*player_eid)
                    else {
                        continue;
                    };
                    let Some(idx) = inv.try_insert(*item_id, *count) else {
                        continue;
                    };
                    (idx, inv.slots[idx].clone())
                };

                // Send SetSlot to sync (raw internal index = SetPlayerInventory slot)
                self.send_to(*player_eid, |tx| {
                    use basalt_mc_protocol::packets::play::inventory::ClientboundPlaySetPlayerInventory;
                    let _ = tx.try_send(ServerOutput::plain(
                        ClientboundPlaySetPlayerInventory::PACKET_ID,
                        ClientboundPlaySetPlayerInventory {
                            slot_id: i32::from(inv_idx as i16),
                            contents: slot_after,
                        },
                    ));
                });

                // Broadcast collect animation + entity destroy
                let collect = Arc::new(SharedBroadcast::single(
                    ClientboundPlayCollect::PACKET_ID,
                    ClientboundPlayCollect {
                        collected_entity_id: *item_eid as i32,
                        collector_entity_id: *player_eid as i32,
                        pickup_item_count: *count,
                    },
                ));
                let destroy = Arc::new(SharedBroadcast::single(
                    ClientboundPlayEntityDestroy::PACKET_ID,
                    ClientboundPlayEntityDestroy {
                        entity_ids: vec![*item_eid as i32],
                    },
                ));
                for (e, _) in self.ecs.iter::<OutputHandle>() {
                    self.send_to(e, |tx| {
                        let _ = tx.try_send(ServerOutput::Cached(Arc::clone(&collect)));
                        let _ = tx.try_send(ServerOutput::Cached(Arc::clone(&destroy)));
                    });
                }

                // Despawn the item entity
                self.ecs.despawn(*item_eid);
                break; // item is consumed, move to next item
            }
        }
    }

    /// Despawns entities whose lifetime has expired.
    ///
    /// The lifetime decrement is handled by the `lifetime_system` ECS
    /// system (registered in lib.rs). This method collects entities
    /// that reached zero and handles the side effects (broadcast + despawn).
    pub(super) fn collect_expired_entities(&mut self) {
        let mut expired = Vec::new();
        for (eid, lt) in self.ecs.iter::<basalt_api::components::Lifetime>() {
            if lt.remaining_ticks == 0 {
                expired.push(eid);
            }
        }

        if expired.is_empty() {
            return;
        }

        let entity_ids: Vec<i32> = expired.iter().map(|&eid| eid as i32).collect();
        let bc = Arc::new(SharedBroadcast::single(
            ClientboundPlayEntityDestroy::PACKET_ID,
            ClientboundPlayEntityDestroy { entity_ids },
        ));
        for (player_eid, _) in self.ecs.iter::<OutputHandle>() {
            self.send_to(player_eid, |tx| {
                let _ = tx.try_send(ServerOutput::Cached(Arc::clone(&bc)));
            });
        }
        for eid in expired {
            self.ecs.despawn(eid);
        }
    }
}

#[cfg(test)]
mod tests {
    use basalt_types::Uuid;

    #[test]
    fn lifetime_system_despawns_expired() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Manually spawn an entity with lifetime = 2
        let eid = 999u32;
        game_loop.ecs.spawn_with_id(eid);
        game_loop.ecs.set(
            eid,
            basalt_api::components::Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        );
        game_loop
            .ecs
            .set(eid, basalt_api::components::Lifetime { remaining_ticks: 2 });

        game_loop.tick(1); // system: 2 → 1, collect: not 0 → alive
        assert!(game_loop.ecs.has::<basalt_api::components::Lifetime>(eid));

        game_loop.tick(2); // system: 1 → 0, collect: 0 → despawned
        assert!(
            !game_loop.ecs.has::<basalt_api::components::Lifetime>(eid),
            "entity should be despawned after lifetime reaches 0"
        );
    }
}
