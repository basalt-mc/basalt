//! Game loop helpers — output sending, context creation, chunk streaming utilities.

use std::sync::Arc;

use basalt_api::context::ServerContext;
use basalt_core::PlayerInfo;
use basalt_core::components::{Position, Rotation};
use basalt_protocol::packets::play::entity::ClientboundPlaySpawnEntity;
use basalt_types::{Encode, Uuid, VarInt, Vec3i16};
use tokio::sync::mpsc;

use super::{GameLoop, OutputHandle};
use crate::helpers::angle_to_byte;
use crate::messages::{EncodablePacket, ServerOutput};

impl GameLoop {
    /// Sends output to a player entity via their OutputHandle.
    pub(super) fn send_to(
        &self,
        eid: basalt_ecs::EntityId,
        f: impl FnOnce(&mpsc::Sender<ServerOutput>),
    ) {
        if let Some(handle) = self.ecs.get::<OutputHandle>(eid) {
            f(&handle.tx);
        }
    }

    /// Creates a ServerContext for event dispatch.
    pub(super) fn make_context(
        &self,
        uuid: Uuid,
        entity_id: i32,
        username: &str,
        yaw: f32,
        pitch: f32,
    ) -> ServerContext {
        // Resolve the player's current position from the ECS so plugin
        // handlers see fresh state. Falls back to (0, 0, 0) if the
        // entity has no Position component (shouldn't happen for
        // online players but stays defensive).
        let position = self
            .find_by_uuid(uuid)
            .and_then(|eid| self.ecs.get::<Position>(eid).map(|p| (p.x, p.y, p.z)))
            .map(|(x, y, z)| Position { x, y, z })
            .unwrap_or(Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            });
        ServerContext::new(
            Arc::clone(&self.world),
            PlayerInfo {
                uuid,
                entity_id,
                username: username.to_string(),
                rotation: Rotation { yaw, pitch },
                position,
            },
        )
    }

    /// Sends a chunk to a player and follows up with BlockEntityData
    /// for any block entities in that chunk (chests, etc.).
    pub(super) fn send_chunk_with_entities(&self, eid: basalt_ecs::EntityId, cx: i32, cz: i32) {
        // Force chunk + block entities to be loaded from disk before querying
        self.world.with_chunk(cx, cz, |_| {});

        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::SendChunk { cx, cz });
        });
        // Send block entity data for chests in this chunk
        for (x, y, z, be) in self.world.block_entities_in_chunk(cx, cz) {
            let action = match &be {
                basalt_world::block_entity::BlockEntity::Chest { .. } => 2,
            };
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::BlockEntityData { x, y, z, action });
            });
        }
    }
}

/// Sends a PlayerInfo "add player" packet (manually encoded due to switch fields).
pub(crate) fn send_player_info_add(
    output_tx: &mpsc::Sender<ServerOutput>,
    info: &basalt_api::broadcast::PlayerSnapshot,
) {
    use basalt_protocol::packets::play::player::ClientboundPlayPlayerInfo;

    let mut buf = Vec::new();
    let actions: u8 = 0x01 | 0x04 | 0x08;
    actions.encode(&mut buf).unwrap();
    VarInt(1).encode(&mut buf).unwrap();
    info.uuid.encode(&mut buf).unwrap();
    info.username.encode(&mut buf).unwrap();
    VarInt(info.skin_properties.len() as i32)
        .encode(&mut buf)
        .unwrap();
    for prop in &info.skin_properties {
        prop.name.encode(&mut buf).unwrap();
        prop.value.encode(&mut buf).unwrap();
        if let Some(sig) = &prop.signature {
            true.encode(&mut buf).unwrap();
            sig.encode(&mut buf).unwrap();
        } else {
            false.encode(&mut buf).unwrap();
        }
    }
    VarInt(1).encode(&mut buf).unwrap(); // gamemode: creative
    true.encode(&mut buf).unwrap(); // listed
    let _ = output_tx.try_send(ServerOutput::Raw {
        id: ClientboundPlayPlayerInfo::PACKET_ID,
        data: buf,
    });
}

/// Sends a SpawnEntity packet for a player entity.
pub(crate) fn send_spawn_entity(
    output_tx: &mpsc::Sender<ServerOutput>,
    info: &basalt_api::broadcast::PlayerSnapshot,
) {
    let packet = ClientboundPlaySpawnEntity {
        entity_id: info.entity_id,
        object_uuid: info.uuid,
        r#type: 147,
        x: info.x,
        y: info.y,
        z: info.z,
        pitch: angle_to_byte(info.pitch),
        yaw: (info.yaw / 360.0 * 256.0) as i8,
        head_pitch: 0,
        object_data: 0,
        velocity: Vec3i16 { x: 0, y: 0, z: 0 },
    };
    let _ = output_tx.try_send(ServerOutput::Packet(EncodablePacket::new(
        ClientboundPlaySpawnEntity::PACKET_ID,
        packet,
    )));
}

#[cfg(test)]
mod tests {
    use super::super::blocks::face_offset;

    #[test]
    fn face_offset_all_directions() {
        assert_eq!(face_offset(0), (0, -1, 0));
        assert_eq!(face_offset(1), (0, 1, 0));
        assert_eq!(face_offset(2), (0, 0, -1));
        assert_eq!(face_offset(3), (0, 0, 1));
        assert_eq!(face_offset(4), (-1, 0, 0));
        assert_eq!(face_offset(5), (1, 0, 0));
        assert_eq!(face_offset(99), (0, 0, 0));
    }
}
