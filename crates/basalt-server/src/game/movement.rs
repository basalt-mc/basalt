//! Player movement — position/look updates and chunk streaming.

use std::collections::HashSet;
use std::sync::Arc;

use basalt_api::events::PlayerMovedEvent;
use basalt_types::Uuid;

use super::{ChunkView, GameLoop, OutputHandle, VIEW_RADIUS};
use crate::messages::{BroadcastEvent, ServerOutput, SharedBroadcast};

impl GameLoop {
    /// Handles movement input: updates ECS, broadcasts, checks chunk boundaries.
    pub(super) fn handle_movement(
        &mut self,
        uuid: Uuid,
        pos: Option<(f64, f64, f64)>,
        look: Option<(f32, f32)>,
        on_ground: bool,
    ) {
        let Some(eid) = self.ecs.find_by_uuid(uuid) else {
            return;
        };

        let (entity_id, old_cx, old_cz, x, y, z, yaw, pitch, username) = {
            let Some(p) = self.ecs.get::<basalt_ecs::Position>(eid) else {
                return;
            };
            let old_cx = (p.x as i32) >> 4;
            let old_cz = (p.z as i32) >> 4;
            let Some(r) = self.ecs.get::<basalt_ecs::Rotation>(eid) else {
                return;
            };
            let Some(pr) = self.ecs.get::<basalt_ecs::PlayerRef>(eid) else {
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
        if let Some(p) = self.ecs.get_mut::<basalt_ecs::Position>(eid) {
            p.x = x;
            p.y = y;
            p.z = z;
        }
        if let Some(r) = self.ecs.get_mut::<basalt_ecs::Rotation>(eid) {
            r.yaw = yaw;
            r.pitch = pitch;
        }

        // Dispatch PlayerMovedEvent
        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
        let mut event = PlayerMovedEvent {
            entity_id,
            x,
            y,
            z,
            yaw,
            pitch,
            on_ground,
            old_cx,
            old_cz,
        };
        self.bus.dispatch(&mut event, &ctx);
        let responses = ctx.drain_responses();
        self.process_responses(uuid, &responses);

        // Broadcast movement to other players
        let moved = Arc::new(SharedBroadcast::new(BroadcastEvent::EntityMoved {
            entity_id,
            x,
            y,
            z,
            yaw,
            pitch,
            on_ground,
        }));
        for (other_eid, _) in self.ecs.iter::<OutputHandle>() {
            if other_eid != eid {
                self.send_to(other_eid, |tx| {
                    let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&moved)));
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
    pub(super) fn stream_chunks(&mut self, eid: basalt_ecs::EntityId, new_cx: i32, new_cz: i32) {
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::UpdateViewPosition {
                cx: new_cx,
                cz: new_cz,
            });
        });

        let r = VIEW_RADIUS;
        let mut in_view = HashSet::new();
        for dx in -r..=r {
            for dz in -r..=r {
                in_view.insert((new_cx + dx, new_cz + dz));
            }
        }

        // Unload
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
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::UnloadChunk { cx, cz });
            });
        }

        let Some(view) = self.ecs.get_mut::<ChunkView>(eid) else {
            return;
        };
        for k in &to_unload {
            view.loaded_chunks.remove(k);
        }

        // Load
        let mut to_load = Vec::new();
        for &key in &in_view {
            if view.loaded_chunks.insert(key) {
                to_load.push(key);
            }
        }

        if !to_load.is_empty() {
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::ChunkBatchStart);
            });
            for &(cx, cz) in &to_load {
                self.send_chunk_with_entities(eid, cx, cz);
            }
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::ChunkBatchFinished {
                    batch_size: to_load.len() as i32,
                });
            });
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

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        let pos = game_loop.ecs.get::<basalt_ecs::Position>(eid).unwrap();
        assert_eq!(pos.x, 10.0);
        assert_eq!(pos.y, 65.0);
        assert_eq!(pos.z, -5.0);
        let rot = game_loop.ecs.get::<basalt_ecs::Rotation>(eid).unwrap();
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

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        let rot = game_loop.ecs.get::<basalt_ecs::Rotation>(eid).unwrap();
        assert_eq!(rot.yaw, 180.0);
        assert_eq!(rot.pitch, -30.0);
        let pos = game_loop.ecs.get::<basalt_ecs::Position>(eid).unwrap();
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
            if matches!(msg, ServerOutput::Broadcast(_)) {
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
        game_loop.tick(1);

        let mut got_packets = false;
        while rx.try_recv().is_ok() {
            got_packets = true;
        }
        assert!(
            got_packets,
            "should receive chunk streaming packets on boundary crossing"
        );

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
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
