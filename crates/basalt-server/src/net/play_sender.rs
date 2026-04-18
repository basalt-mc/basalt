//! Protocol encoding for game events — translates [`ServerOutput`] into TCP packets.
//!
//! Contains the encoding logic that maps game events to protocol packets.
//! [`write_server_output`] is the main entry point, called from the net task
//! select loop for both targeted and broadcast messages.

use basalt_net::connection::{Connection, Play};
use basalt_protocol::packets::play::chat::ClientboundPlaySystemChat;
use basalt_protocol::packets::play::entity::{
    ClientboundPlayEntityDestroy, ClientboundPlayEntityHeadRotation, ClientboundPlayEntityMetadata,
    ClientboundPlaySyncEntityPosition,
};
use basalt_protocol::packets::play::player::{
    ClientboundPlayGameStateChange, ClientboundPlayPlayerRemove, ClientboundPlayPosition,
};
use basalt_protocol::packets::play::world::{
    ClientboundPlayAcknowledgePlayerDigging, ClientboundPlayBlockChange,
    ClientboundPlayChunkBatchFinished, ClientboundPlayChunkBatchStart, ClientboundPlayMapChunk,
    ClientboundPlayUnloadChunk, ClientboundPlayUpdateViewPosition,
};
use basalt_types::{Encode, EncodedSize, Uuid};

use crate::helpers::{RawPayload, RawSlice, angle_to_byte};
use crate::messages::{BroadcastEvent, ServerOutput};
use crate::net::chunk_cache::ChunkPacketCache;

/// Encodes and writes a [`ServerOutput`] game event to the TCP connection.
///
/// This is where protocol knowledge lives: each game event variant is
/// translated into one or more protocol packets, encoded, and written.
pub(super) async fn write_server_output(
    conn: &mut Connection<Play>,
    output: &ServerOutput,
    chunk_cache: &ChunkPacketCache,
) -> crate::error::Result<()> {
    match output {
        // ── Hot path: targeted events ────────────────────────────────
        ServerOutput::BlockChanged { x, y, z, state } => {
            let packet = ClientboundPlayBlockChange {
                location: basalt_types::Position::new(*x, *y, *z),
                r#type: *state,
            };
            conn.write_packet_typed(ClientboundPlayBlockChange::PACKET_ID, &packet)
                .await?;
        }
        ServerOutput::BlockAck { sequence } => {
            let packet = ClientboundPlayAcknowledgePlayerDigging {
                sequence_id: *sequence,
            };
            conn.write_packet_typed(ClientboundPlayAcknowledgePlayerDigging::PACKET_ID, &packet)
                .await?;
        }
        ServerOutput::SystemChat {
            content,
            action_bar,
        } => {
            let packet = ClientboundPlaySystemChat {
                content: content.clone(),
                is_action_bar: *action_bar,
            };
            conn.write_packet_typed(ClientboundPlaySystemChat::PACKET_ID, &packet)
                .await?;
        }
        ServerOutput::GameStateChange { reason, value } => {
            let packet = ClientboundPlayGameStateChange {
                reason: *reason,
                game_mode: *value,
            };
            conn.write_packet_typed(ClientboundPlayGameStateChange::PACKET_ID, &packet)
                .await?;
        }
        ServerOutput::SetPosition {
            teleport_id,
            x,
            y,
            z,
            yaw,
            pitch,
        } => {
            let packet = ClientboundPlayPosition {
                teleport_id: *teleport_id,
                x: *x,
                y: *y,
                z: *z,
                dx: 0.0,
                dy: 0.0,
                dz: 0.0,
                yaw: *yaw,
                pitch: *pitch,
                flags: 0,
            };
            conn.write_packet_typed(ClientboundPlayPosition::PACKET_ID, &packet)
                .await?;
        }
        ServerOutput::SetSlot { slot, item } => {
            use basalt_protocol::packets::play::inventory::ClientboundPlaySetPlayerInventory;
            // Internal slot = raw MC slot (0-8 hotbar, 9-35 main), no conversion needed
            let packet = ClientboundPlaySetPlayerInventory {
                slot_id: i32::from(*slot),
                contents: item.clone(),
            };
            conn.write_packet_typed(ClientboundPlaySetPlayerInventory::PACKET_ID, &packet)
                .await?;
        }
        ServerOutput::SyncInventory { slots } => {
            use basalt_protocol::packets::play::inventory::ClientboundPlayWindowItems;
            let packet = ClientboundPlayWindowItems {
                window_id: 0,
                state_id: 0,
                items: slots.clone(),
                carried_item: basalt_types::Slot::empty(),
            };
            conn.write_packet_typed(ClientboundPlayWindowItems::PACKET_ID, &packet)
                .await?;
        }
        ServerOutput::OpenWindow {
            window_id,
            inventory_type,
            title,
            slots,
        } => {
            use basalt_protocol::packets::play::inventory::{
                ClientboundPlayOpenWindow, ClientboundPlayWindowItems,
            };
            let open = ClientboundPlayOpenWindow {
                window_id: *window_id as i32,
                inventory_type: *inventory_type,
                window_title: title.clone(),
            };
            conn.write_packet_typed(ClientboundPlayOpenWindow::PACKET_ID, &open)
                .await?;
            let items = ClientboundPlayWindowItems {
                window_id: *window_id as i32,
                state_id: 0,
                items: slots.clone(),
                carried_item: basalt_types::Slot::empty(),
            };
            conn.write_packet_typed(ClientboundPlayWindowItems::PACKET_ID, &items)
                .await?;
        }
        ServerOutput::SetContainerSlot {
            window_id,
            slot,
            item,
        } => {
            use basalt_protocol::packets::play::inventory::ClientboundPlaySetSlot;
            let packet = ClientboundPlaySetSlot {
                window_id: *window_id as i32,
                state_id: 0,
                slot: *slot,
                item: item.clone(),
            };
            conn.write_packet_typed(ClientboundPlaySetSlot::PACKET_ID, &packet)
                .await?;
        }
        ServerOutput::BlockAction {
            x,
            y,
            z,
            action_id,
            action_param,
            block_id,
        } => {
            use basalt_protocol::packets::play::world::ClientboundPlayBlockAction;
            let packet = ClientboundPlayBlockAction {
                location: basalt_types::Position::new(*x, *y, *z),
                byte1: *action_id,
                byte2: *action_param,
                block_id: *block_id,
            };
            conn.write_packet_typed(ClientboundPlayBlockAction::PACKET_ID, &packet)
                .await?;
            // TODO: chest open/close sound — SoundEffect encoding needs investigation
        }
        ServerOutput::BlockEntityData { x, y, z, action } => {
            use basalt_protocol::packets::play::world::ClientboundPlayTileEntityData;
            let packet = ClientboundPlayTileEntityData {
                location: basalt_types::Position::new(*x, *y, *z),
                action: *action,
                nbt_data: basalt_types::nbt::NbtCompound::new(),
            };
            conn.write_packet_typed(ClientboundPlayTileEntityData::PACKET_ID, &packet)
                .await?;
        }
        // ── Chunk path: cache-based ──────────────────────────────────
        ServerOutput::SendChunk { cx, cz } => {
            let bytes = chunk_cache.get_or_encode(*cx, *cz);
            conn.write_packet_typed(ClientboundPlayMapChunk::PACKET_ID, &RawSlice(&bytes))
                .await?;
        }
        ServerOutput::UnloadChunk { cx, cz } => {
            let packet = ClientboundPlayUnloadChunk {
                chunk_x: *cx,
                chunk_z: *cz,
            };
            conn.write_packet_typed(ClientboundPlayUnloadChunk::PACKET_ID, &packet)
                .await?;
        }
        ServerOutput::ChunkBatchStart => {
            conn.write_packet_typed(
                ClientboundPlayChunkBatchStart::PACKET_ID,
                &ClientboundPlayChunkBatchStart,
            )
            .await?;
        }
        ServerOutput::ChunkBatchFinished { batch_size } => {
            let packet = ClientboundPlayChunkBatchFinished {
                batch_size: *batch_size,
            };
            conn.write_packet_typed(ClientboundPlayChunkBatchFinished::PACKET_ID, &packet)
                .await?;
        }
        ServerOutput::UpdateViewPosition { cx, cz } => {
            let packet = ClientboundPlayUpdateViewPosition {
                chunk_x: *cx,
                chunk_z: *cz,
            };
            conn.write_packet_typed(ClientboundPlayUpdateViewPosition::PACKET_ID, &packet)
                .await?;
        }

        // ── Broadcast: shared, encoded once ──────────────────────────
        ServerOutput::Broadcast(shared) => {
            let packets = shared.get_or_encode(encode_broadcast);
            for (id, data) in packets {
                conn.write_packet_typed(*id, &RawSlice(data)).await?;
            }
        }

        // ── Cold path: rare events ───────────────────────────────────
        ServerOutput::Packet(ep) => {
            let mut data = Vec::with_capacity(ep.payload.encoded_size());
            ep.payload
                .encode(&mut data)
                .expect("packet encoding failed");
            conn.write_packet_typed(ep.id, &RawPayload(data)).await?;
        }
        ServerOutput::Raw { id, data } => {
            conn.write_packet_typed(*id, &RawSlice(data)).await?;
        }
    }
    Ok(())
}

/// Encodes a [`BroadcastEvent`] into protocol packets.
///
/// Called at most once per [`SharedBroadcast`] — the result is cached
/// in the `OnceLock` and reused by all subsequent consumers.
pub(crate) fn encode_broadcast(event: &BroadcastEvent) -> Vec<(i32, Vec<u8>)> {
    match event {
        BroadcastEvent::EntityMoved {
            entity_id,
            x,
            y,
            z,
            yaw,
            pitch,
            on_ground,
        } => {
            let sync = ClientboundPlaySyncEntityPosition {
                entity_id: *entity_id,
                x: *x,
                y: *y,
                z: *z,
                dx: 0.0,
                dy: 0.0,
                dz: 0.0,
                yaw: *yaw,
                pitch: *pitch,
                on_ground: *on_ground,
            };
            let head = ClientboundPlayEntityHeadRotation {
                entity_id: *entity_id,
                head_yaw: angle_to_byte(*yaw),
            };
            vec![
                encode_single(ClientboundPlaySyncEntityPosition::PACKET_ID, &sync),
                encode_single(ClientboundPlayEntityHeadRotation::PACKET_ID, &head),
            ]
        }
        BroadcastEvent::BlockChanged { x, y, z, state } => {
            let packet = ClientboundPlayBlockChange {
                location: basalt_types::Position::new(*x, *y, *z),
                r#type: *state,
            };
            vec![encode_single(
                ClientboundPlayBlockChange::PACKET_ID,
                &packet,
            )]
        }
        BroadcastEvent::SystemChat {
            content,
            action_bar,
        } => {
            let packet = ClientboundPlaySystemChat {
                content: content.clone(),
                is_action_bar: *action_bar,
            };
            vec![encode_single(ClientboundPlaySystemChat::PACKET_ID, &packet)]
        }
        BroadcastEvent::RemoveEntities { entity_ids } => {
            let packet = ClientboundPlayEntityDestroy {
                entity_ids: entity_ids.clone(),
            };
            vec![encode_single(
                ClientboundPlayEntityDestroy::PACKET_ID,
                &packet,
            )]
        }
        BroadcastEvent::RemovePlayers { uuids } => {
            let packet = ClientboundPlayPlayerRemove {
                players: uuids.clone(),
            };
            vec![encode_single(
                ClientboundPlayPlayerRemove::PACKET_ID,
                &packet,
            )]
        }
        BroadcastEvent::SpawnItemEntity {
            entity_id,
            x,
            y,
            z,
            vx,
            vy,
            vz,
            item_id,
            count,
        } => {
            use basalt_protocol::packets::play::entity::ClientboundPlaySpawnEntity;

            // SpawnEntity packet (type 55 = item entity)
            let spawn = ClientboundPlaySpawnEntity {
                entity_id: *entity_id,
                object_uuid: Uuid::from_bytes((*entity_id as u128).to_le_bytes()),
                r#type: 68, // item entity in 1.21.4
                x: *x,
                y: *y,
                z: *z,
                pitch: 0,
                yaw: 0,
                head_pitch: 0,
                object_data: 0,
                velocity: basalt_types::Vec3i16 {
                    x: (*vx * 8000.0) as i16,
                    y: (*vy * 8000.0) as i16,
                    z: (*vz * 8000.0) as i16,
                },
            };

            // EntityMetadata with the item slot
            let meta_packet = ClientboundPlayEntityMetadata {
                entity_id: *entity_id,
                metadata: encode_item_metadata(*item_id, *count),
            };

            vec![
                encode_single(ClientboundPlaySpawnEntity::PACKET_ID, &spawn),
                encode_single(ClientboundPlayEntityMetadata::PACKET_ID, &meta_packet),
            ]
        }
        BroadcastEvent::CollectItem {
            collected_entity_id,
            collector_entity_id,
            count,
        } => {
            use basalt_protocol::packets::play::entity::ClientboundPlayCollect;

            let packet = ClientboundPlayCollect {
                collected_entity_id: *collected_entity_id,
                collector_entity_id: *collector_entity_id,
                pickup_item_count: *count,
            };
            vec![encode_single(ClientboundPlayCollect::PACKET_ID, &packet)]
        }
    }
}

/// Encodes entity metadata entries for a dropped item entity.
///
/// Produces the raw metadata bytes (without entity ID -- that's in
/// the [`ClientboundPlayEntityMetadata`] struct):
/// - Index 8, type 7 (Slot), value = item slot
/// - 0xFF terminator
fn encode_item_metadata(item_id: i32, count: i32) -> Vec<u8> {
    use basalt_types::VarInt;

    let mut buf = Vec::new();
    // Index 8 = item slot for item entities
    8u8.encode(&mut buf).unwrap();
    // Type 7 = Slot
    VarInt(7).encode(&mut buf).unwrap();
    // Slot data
    let slot = basalt_types::Slot::new(item_id, count);
    slot.encode(&mut buf).unwrap();
    // Terminator
    0xFFu8.encode(&mut buf).unwrap();
    buf
}

/// Encodes a single protocol packet into `(packet_id, payload_bytes)`.
fn encode_single<P: Encode + EncodedSize>(id: i32, packet: &P) -> (i32, Vec<u8>) {
    let mut data = Vec::with_capacity(packet.encoded_size());
    packet.encode(&mut data).expect("packet encoding failed");
    (id, data)
}
