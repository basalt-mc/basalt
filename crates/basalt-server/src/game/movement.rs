//! Player movement — position/look updates and chunk streaming.

use std::collections::HashSet;
use std::sync::Arc;

use basalt_api::events::PlayerMovedEvent;
use basalt_mc_protocol::packets::play::entity::{
    ClientboundPlayEntityHeadRotation, ClientboundPlaySyncEntityPosition,
};
use basalt_types::Uuid;

use super::{ChunkStreamRate, ChunkView, GameLoop, OutputHandle, VIEW_RADIUS};
use crate::helpers::angle_to_byte;
use crate::messages::{EncodablePacket, ServerOutput, SharedBroadcast};

impl GameLoop {
    /// Handles movement input: updates ECS, broadcasts, checks chunk boundaries.
    pub(super) fn handle_movement(
        &mut self,
        uuid: Uuid,
        pos: Option<(f64, f64, f64)>,
        look: Option<(f32, f32)>,
        on_ground: bool,
    ) {
        let Some(eid) = self.find_by_uuid(uuid) else {
            return;
        };

        let (entity_id, old_cx, old_cz, x, y, z, yaw, pitch, username) = {
            let Some(p) = self.ecs.get::<basalt_api::components::Position>(eid) else {
                return;
            };
            let old_cx = (p.x as i32) >> 4;
            let old_cz = (p.z as i32) >> 4;
            let Some(r) = self.ecs.get::<basalt_api::components::Rotation>(eid) else {
                return;
            };
            let Some(pr) = self.ecs.get::<basalt_api::components::PlayerRef>(eid) else {
                return;
            };
            (
                eid as i32,
                old_cx,
                old_cz,
                pos.map_or(p.x, |p| p.0),
                pos.map_or(p.y, |p| p.1),
                pos.map_or(p.z, |p| p.2),
                look.map_or(r.yaw, |l| l.0),
                look.map_or(r.pitch, |l| l.1),
                pr.username.clone(),
            )
        };

        // Update ECS
        if let Some(p) = self.ecs.get_mut::<basalt_api::components::Position>(eid) {
            p.x = x;
            p.y = y;
            p.z = z;
        }
        if let Some(r) = self.ecs.get_mut::<basalt_api::components::Rotation>(eid) {
            r.yaw = yaw;
            r.pitch = pitch;
        }

        // Dispatch PlayerMovedEvent
        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
        let mut event = PlayerMovedEvent {
            position: basalt_api::components::Position { x, y, z },
            rotation: basalt_api::components::Rotation { yaw, pitch },
            on_ground,
            old_chunk: basalt_api::components::ChunkPosition {
                x: old_cx,
                z: old_cz,
            },
        };
        self.dispatch_event(&mut event, &ctx);
        let responses = ctx.drain_responses();
        self.process_responses(uuid, &responses);

        // Broadcast movement to other players
        let moved = Arc::new(SharedBroadcast::new(vec![
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
                    yaw,
                    pitch,
                    on_ground,
                },
            ),
            EncodablePacket::new(
                ClientboundPlayEntityHeadRotation::PACKET_ID,
                ClientboundPlayEntityHeadRotation {
                    entity_id,
                    head_yaw: angle_to_byte(yaw),
                },
            ),
        ]));
        for (other_eid, _) in self.ecs.iter::<OutputHandle>() {
            if other_eid != eid {
                self.send_to(other_eid, |tx| {
                    let _ = tx.try_send(ServerOutput::Cached(Arc::clone(&moved)));
                });
            }
        }

        // Check chunk boundary for streaming
        let new_cx = (x as i32) >> 4;
        let new_cz = (z as i32) >> 4;
        if new_cx != old_cx || new_cz != old_cz {
            self.stream_chunks(eid, new_cx, new_cz);
            self.rebuild_active_chunks();
        }
    }

    /// Streams chunks when a player crosses a chunk boundary.
    ///
    /// Sends `UnloadChunk` for chunks that left the view, drops still-pending
    /// chunks that have left the view (they were never sent), and enqueues
    /// newly-in-view chunks onto the player's [`ChunkStreamRate`] pending
    /// queue. The actual sending happens in `drain_chunk_batches` at the
    /// player's negotiated per-tick rate.
    pub(super) fn stream_chunks(&mut self, eid: basalt_ecs::EntityId, new_cx: i32, new_cz: i32) {
        use basalt_mc_protocol::packets::play::world::ClientboundPlayUpdateViewPosition;
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::plain(
                ClientboundPlayUpdateViewPosition::PACKET_ID,
                ClientboundPlayUpdateViewPosition {
                    chunk_x: new_cx,
                    chunk_z: new_cz,
                },
            ));
        });

        let r = VIEW_RADIUS;
        let mut in_view = HashSet::new();
        for dx in -r..=r {
            for dz in -r..=r {
                in_view.insert((new_cx + dx, new_cz + dz));
            }
        }

        // Unload chunks the client previously received but no longer needs.
        let Some(view) = self.ecs.get::<ChunkView>(eid) else {
            return;
        };
        let to_unload: Vec<(i32, i32)> = view
            .loaded_chunks
            .iter()
            .filter(|k| !in_view.contains(k))
            .copied()
            .collect();

        for &(cx, cz) in &to_unload {
            use basalt_mc_protocol::packets::play::world::ClientboundPlayUnloadChunk;
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::plain(
                    ClientboundPlayUnloadChunk::PACKET_ID,
                    ClientboundPlayUnloadChunk {
                        chunk_x: cx,
                        chunk_z: cz,
                    },
                ));
            });
        }

        if let Some(view) = self.ecs.get_mut::<ChunkView>(eid) {
            for k in &to_unload {
                view.loaded_chunks.remove(k);
            }
        }

        // Compute the set of chunks already accounted for so we don't
        // double-enqueue: `loaded_chunks` are sent, `pending` are queued.
        let already = {
            let loaded = self
                .ecs
                .get::<ChunkView>(eid)
                .map(|v| v.loaded_chunks.clone())
                .unwrap_or_default();
            let queued: HashSet<(i32, i32)> = self
                .ecs
                .get::<ChunkStreamRate>(eid)
                .map(|r| r.pending.iter().copied().collect())
                .unwrap_or_default();
            loaded.union(&queued).copied().collect::<HashSet<_>>()
        };

        if let Some(rate) = self.ecs.get_mut::<ChunkStreamRate>(eid) {
            // Drop pending chunks that have left the view — the client never
            // received them so no UnloadChunk is needed; we just cancel the
            // queued send.
            rate.pending.retain(|k| in_view.contains(k));
            // Enqueue newly-visible chunks. Order is "row-major in view radius"
            // which biases nearby chunks earlier — distance-priority is a
            // separate optimization (see issue #173 non-goals).
            for dx in -r..=r {
                for dz in -r..=r {
                    let key = (new_cx + dx, new_cz + dz);
                    if !already.contains(&key) {
                        rate.pending.push_back(key);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use basalt_types::Uuid;

    use crate::messages::{GameInput, ServerOutput};

    #[test]
    fn movement_updates_position_and_rotation() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let _ = game_tx.send(GameInput::PositionLook {
            uuid,
            x: 10.0,
            y: 65.0,
            z: -5.0,
            yaw: 90.0,
            pitch: 45.0,
            on_ground: true,
        });
        game_loop.tick(1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        let pos = game_loop
            .ecs
            .get::<basalt_api::components::Position>(eid)
            .unwrap();
        assert_eq!(pos.x, 10.0);
        assert_eq!(pos.y, 65.0);
        assert_eq!(pos.z, -5.0);
        let rot = game_loop
            .ecs
            .get::<basalt_api::components::Rotation>(eid)
            .unwrap();
        assert_eq!(rot.yaw, 90.0);
        assert_eq!(rot.pitch, 45.0);
    }

    #[test]
    fn look_only_updates_rotation() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let _ = game_tx.send(GameInput::Look {
            uuid,
            yaw: 180.0,
            pitch: -30.0,
            on_ground: true,
        });
        game_loop.tick(1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        let rot = game_loop
            .ecs
            .get::<basalt_api::components::Rotation>(eid)
            .unwrap();
        assert_eq!(rot.yaw, 180.0);
        assert_eq!(rot.pitch, -30.0);
        let pos = game_loop
            .ecs
            .get::<basalt_api::components::Position>(eid)
            .unwrap();
        assert_eq!(pos.x, 0.0);
    }

    #[test]
    fn movement_broadcasts_to_other_players() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid1 = Uuid::from_bytes([1; 16]);
        let uuid2 = Uuid::from_bytes([2; 16]);
        let _rx1 = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid1, 1);
        let mut rx2 = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid2, 2);

        while rx2.try_recv().is_ok() {}

        let _ = game_tx.send(GameInput::Position {
            uuid: uuid1,
            x: 5.0,
            y: -60.0,
            z: 3.0,
            on_ground: true,
        });
        game_loop.tick(2);

        let mut got_moved = false;
        while let Ok(msg) = rx2.try_recv() {
            if matches!(msg, ServerOutput::Cached(_)) {
                got_moved = true;
            }
        }
        assert!(got_moved, "player 2 should receive movement broadcast");
    }

    #[test]
    fn chunk_streaming_on_boundary_crossing() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        while rx.try_recv().is_ok() {}

        let _ = game_tx.send(GameInput::Position {
            uuid,
            x: 32.0,
            y: -60.0,
            z: 0.0,
            on_ground: true,
        });

        // The drainer ships up to floor(rate)=25 chunks per tick, and a
        // boundary crossing can leave the queue holding the full view
        // radius (~121 chunks). Drive enough ticks to drain everything
        // — 10 is well over the ~5 needed at rate 25.
        for tick in 1..=10 {
            game_loop.tick(tick);
        }

        let mut got_packets = false;
        while rx.try_recv().is_ok() {
            got_packets = true;
        }
        assert!(
            got_packets,
            "should receive chunk streaming packets on boundary crossing"
        );

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        let view = game_loop.ecs.get::<super::super::ChunkView>(eid).unwrap();
        let new_cx = (32.0_f64 as i32) >> 4;
        assert!(
            view.loaded_chunks.contains(&(new_cx, 0)),
            "chunk view should contain the new center chunk"
        );
    }

    #[test]
    fn position_update_for_unknown_player_ignored() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let unknown = Uuid::from_bytes([99; 16]);

        let _ = game_tx.send(GameInput::Position {
            uuid: unknown,
            x: 10.0,
            y: 65.0,
            z: -5.0,
            on_ground: true,
        });
        game_loop.tick(0);
    }
}
