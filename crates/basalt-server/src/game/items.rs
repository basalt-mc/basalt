//! Item entities — spawning, movement broadcasting, pickup, and lifetime expiry.

use std::sync::Arc;

use super::{GameLoop, OutputHandle};
use crate::messages::{BroadcastEvent, ServerOutput, SharedBroadcast};

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
            basalt_ecs::Position {
                x: px,
                y: py,
                z: pz,
            },
        );
        self.ecs.set(
            eid,
            basalt_ecs::Velocity {
                dx: 0.0,
                dy: 0.2,
                dz: 0.0,
            },
        );
        self.ecs.set(
            eid,
            basalt_ecs::BoundingBox {
                width: 0.25,
                height: 0.25,
            },
        );
        self.ecs.set(eid, basalt_ecs::EntityKind { type_id: 68 });
        self.ecs.set(
            eid,
            basalt_ecs::PickupDelay {
                remaining_ticks: 10,
            },
        );
        self.ecs.set(
            eid,
            basalt_ecs::Lifetime {
                remaining_ticks: 6000,
            },
        );
        self.ecs.set(
            eid,
            basalt_ecs::DroppedItem {
                slot: basalt_types::Slot::new(item_id, count),
            },
        );

        // Broadcast spawn to all players
        let bc = Arc::new(SharedBroadcast::new(BroadcastEvent::SpawnItemEntity {
            entity_id,
            x: px,
            y: py,
            z: pz,
            vx: 0.0,
            vy: 0.2,
            vz: 0.0,
            item_id,
            count,
        }));
        for (other_eid, _) in self.ecs.iter::<OutputHandle>() {
            self.send_to(other_eid, |tx| {
                let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&bc)));
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
            .iter::<basalt_ecs::DroppedItem>()
            .filter_map(|(eid, _)| {
                let vel = self.ecs.get::<basalt_ecs::Velocity>(eid)?;
                // Only broadcast if actually moving
                if vel.dx.abs() < 0.001 && vel.dy.abs() < 0.001 && vel.dz.abs() < 0.001 {
                    return None;
                }
                let pos = self.ecs.get::<basalt_ecs::Position>(eid)?;
                Some((eid, pos.x, pos.y, pos.z))
            })
            .collect();

        if moving.is_empty() {
            return;
        }

        for (eid, x, y, z) in moving {
            let bc = Arc::new(SharedBroadcast::new(BroadcastEvent::EntityMoved {
                entity_id: eid as i32,
                x,
                y,
                z,
                yaw: 0.0,
                pitch: 0.0,
                on_ground: false,
            }));
            for (player_eid, _) in self.ecs.iter::<OutputHandle>() {
                self.send_to(player_eid, |tx| {
                    let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&bc)));
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
            .iter::<basalt_ecs::DroppedItem>()
            .filter_map(|(eid, item)| {
                // Skip items still on pickup delay
                if let Some(delay) = self.ecs.get::<basalt_ecs::PickupDelay>(eid)
                    && delay.remaining_ticks > 0
                {
                    return None;
                }
                let pos = self.ecs.get::<basalt_ecs::Position>(eid)?;
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
            .iter::<basalt_ecs::PlayerRef>()
            .filter_map(|(eid, _)| {
                let pos = self.ecs.get::<basalt_ecs::Position>(eid)?;
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
                    let Some(inv) = self.ecs.get_mut::<basalt_ecs::Inventory>(*player_eid) else {
                        continue;
                    };
                    let Some(idx) = inv.try_insert(*item_id, *count) else {
                        continue;
                    };
                    (idx, inv.slots[idx].clone())
                };

                // Send SetSlot to sync (raw internal index = SetPlayerInventory slot)
                self.send_to(*player_eid, |tx| {
                    let _ = tx.try_send(ServerOutput::SetSlot {
                        slot: inv_idx as i16,
                        item: slot_after,
                    });
                });

                // Broadcast collect animation + entity destroy
                let collect = Arc::new(SharedBroadcast::new(BroadcastEvent::CollectItem {
                    collected_entity_id: *item_eid as i32,
                    collector_entity_id: *player_eid as i32,
                    count: *count,
                }));
                let destroy = Arc::new(SharedBroadcast::new(BroadcastEvent::RemoveEntities {
                    entity_ids: vec![*item_eid as i32],
                }));
                for (e, _) in self.ecs.iter::<OutputHandle>() {
                    self.send_to(e, |tx| {
                        let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&collect)));
                        let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&destroy)));
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
        for (eid, lt) in self.ecs.iter::<basalt_ecs::Lifetime>() {
            if lt.remaining_ticks == 0 {
                expired.push(eid);
            }
        }

        if expired.is_empty() {
            return;
        }

        let entity_ids: Vec<i32> = expired.iter().map(|&eid| eid as i32).collect();
        let bc = Arc::new(SharedBroadcast::new(BroadcastEvent::RemoveEntities {
            entity_ids,
        }));
        for (player_eid, _) in self.ecs.iter::<OutputHandle>() {
            self.send_to(player_eid, |tx| {
                let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&bc)));
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
            basalt_ecs::Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        );
        game_loop
            .ecs
            .set(eid, basalt_ecs::Lifetime { remaining_ticks: 2 });

        game_loop.tick(1); // system: 2 → 1, collect: not 0 → alive
        assert!(game_loop.ecs.has::<basalt_ecs::Lifetime>(eid));

        game_loop.tick(2); // system: 1 → 0, collect: 0 → despawned
        assert!(
            !game_loop.ecs.has::<basalt_ecs::Lifetime>(eid),
            "entity should be despawned after lifetime reaches 0"
        );
    }
}
