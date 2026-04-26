//! Response processing — translates event handler responses into server output.

use std::sync::Arc;

use basalt_api::context::Response;
use basalt_types::Uuid;

use super::{GameLoop, OutputHandle};
use crate::messages::{ServerOutput, SharedBroadcast};

impl GameLoop {
    /// Processes event handler responses.
    pub(super) fn process_responses(&mut self, source_uuid: Uuid, responses: &[Response]) {
        for response in responses {
            match response {
                Response::Broadcast(basalt_api::broadcast::BroadcastMessage::BlockChanged {
                    x,
                    y,
                    z,
                    block_state,
                }) => {
                    use basalt_mc_protocol::packets::play::world::ClientboundPlayBlockChange;
                    // Invalidate chunk cache for this block's chunk
                    self.chunk_cache.invalidate(*x >> 4, *z >> 4);
                    let bc = Arc::new(SharedBroadcast::single(
                        ClientboundPlayBlockChange::PACKET_ID,
                        ClientboundPlayBlockChange {
                            location: basalt_types::Position::new(*x, *y, *z),
                            r#type: *block_state,
                        },
                    ));
                    for (e, _) in self.ecs.iter::<OutputHandle>() {
                        self.send_to(e, |tx| {
                            let _ = tx.try_send(ServerOutput::Cached(Arc::clone(&bc)));
                        });
                    }
                }
                Response::Broadcast(basalt_api::broadcast::BroadcastMessage::Chat { content }) => {
                    use basalt_mc_protocol::packets::play::chat::ClientboundPlaySystemChat;
                    let bc = Arc::new(SharedBroadcast::single(
                        ClientboundPlaySystemChat::PACKET_ID,
                        ClientboundPlaySystemChat {
                            content: content.clone(),
                            is_action_bar: false,
                        },
                    ));
                    for (e, _) in self.ecs.iter::<OutputHandle>() {
                        self.send_to(e, |tx| {
                            let _ = tx.try_send(ServerOutput::Cached(Arc::clone(&bc)));
                        });
                    }
                }
                Response::Broadcast(_) => {}
                Response::SendBlockAck { sequence } => {
                    use basalt_mc_protocol::packets::play::world::ClientboundPlayAcknowledgePlayerDigging;
                    if let Some(eid) = self.find_by_uuid(source_uuid)
                        && let Some(handle) = self.ecs.get::<OutputHandle>(eid)
                    {
                        let _ = handle.tx.try_send(ServerOutput::plain(
                            ClientboundPlayAcknowledgePlayerDigging::PACKET_ID,
                            ClientboundPlayAcknowledgePlayerDigging {
                                sequence_id: *sequence,
                            },
                        ));
                    }
                }
                Response::SendSystemChat {
                    content,
                    action_bar,
                } => {
                    use basalt_mc_protocol::packets::play::chat::ClientboundPlaySystemChat;
                    if let Some(eid) = self.find_by_uuid(source_uuid)
                        && let Some(handle) = self.ecs.get::<OutputHandle>(eid)
                    {
                        let _ = handle.tx.try_send(ServerOutput::plain(
                            ClientboundPlaySystemChat::PACKET_ID,
                            ClientboundPlaySystemChat {
                                content: content.clone(),
                                is_action_bar: *action_bar,
                            },
                        ));
                    }
                }
                Response::SendPosition {
                    teleport_id,
                    position,
                    rotation,
                } => {
                    use basalt_mc_protocol::packets::play::player::ClientboundPlayPosition;
                    if let Some(eid) = self.find_by_uuid(source_uuid) {
                        if let Some(pos) = self.ecs.get_mut::<basalt_api::components::Position>(eid)
                        {
                            pos.x = position.x;
                            pos.y = position.y;
                            pos.z = position.z;
                        }
                        if let Some(handle) = self.ecs.get::<OutputHandle>(eid) {
                            let _ = handle.tx.try_send(ServerOutput::plain(
                                ClientboundPlayPosition::PACKET_ID,
                                ClientboundPlayPosition {
                                    teleport_id: *teleport_id,
                                    x: position.x,
                                    y: position.y,
                                    z: position.z,
                                    dx: 0.0,
                                    dy: 0.0,
                                    dz: 0.0,
                                    yaw: rotation.yaw,
                                    pitch: rotation.pitch,
                                    flags: 0,
                                },
                            ));
                        }
                    }
                }
                Response::StreamChunks(chunk) => {
                    if let Some(eid) = self.find_by_uuid(source_uuid) {
                        self.stream_chunks(eid, chunk.x, chunk.z);
                    }
                }
                Response::SendGameStateChange { reason, value } => {
                    use basalt_mc_protocol::packets::play::player::ClientboundPlayGameStateChange;
                    if let Some(eid) = self.find_by_uuid(source_uuid)
                        && let Some(handle) = self.ecs.get::<OutputHandle>(eid)
                    {
                        let _ = handle.tx.try_send(ServerOutput::plain(
                            ClientboundPlayGameStateChange::PACKET_ID,
                            ClientboundPlayGameStateChange {
                                reason: *reason,
                                game_mode: *value,
                            },
                        ));
                    }
                }
                Response::PersistChunk(chunk) => {
                    let _ = self
                        .io_tx
                        .send(crate::runtime::io_thread::IoRequest::PersistChunk {
                            cx: chunk.x,
                            cz: chunk.z,
                        });
                }
                Response::SpawnDroppedItem {
                    position,
                    item_id,
                    count,
                } => {
                    self.spawn_item_entity(position.x, position.y, position.z, *item_id, *count);
                }
                Response::OpenChest(pos) => {
                    if let Some(eid) = self.find_by_uuid(source_uuid) {
                        self.open_chest(source_uuid, eid, pos.x, pos.y, pos.z);
                    }
                }
                Response::OpenCraftingTable { position } => {
                    if let Some(eid) = self.find_by_uuid(source_uuid) {
                        self.open_crafting_table(eid, position.x, position.y, position.z);
                    }
                }
                Response::OpenContainer(container) => {
                    if let Some(eid) = self.find_by_uuid(source_uuid) {
                        self.open_custom_container(eid, source_uuid, container.clone());
                    }
                }
                Response::BroadcastBlockAction {
                    position,
                    action_id,
                    action_param,
                    block_id,
                } => {
                    let x = position.x;
                    let y = position.y;
                    let z = position.z;
                    let aid = *action_id;
                    let ap = *action_param;
                    let bid = *block_id;
                    use basalt_mc_protocol::packets::play::world::ClientboundPlayBlockAction;
                    for (e, _) in self.ecs.iter::<OutputHandle>() {
                        self.send_to(e, |tx| {
                            let _ = tx.try_send(ServerOutput::plain(
                                ClientboundPlayBlockAction::PACKET_ID,
                                ClientboundPlayBlockAction {
                                    location: basalt_types::Position::new(x, y, z),
                                    byte1: aid,
                                    byte2: ap,
                                    block_id: bid,
                                },
                            ));
                        });
                    }
                }
                Response::NotifyContainerViewers {
                    position,
                    slot_index,
                    item,
                } => {
                    if let Some(source_eid) = self.find_by_uuid(source_uuid) {
                        self.notify_container_viewers(
                            (position.x, position.y, position.z),
                            source_eid,
                            *slot_index,
                            item,
                        );
                    }
                }
                Response::DestroyBlockEntity { position } => {
                    if let Some(eid) = self.find_by_uuid(source_uuid) {
                        self.destroy_block_entity(
                            source_uuid,
                            eid,
                            position.x,
                            position.y,
                            position.z,
                        );
                    }
                }
                Response::UnlockRecipe { recipe_id, reason } => {
                    self.unlock_recipe(source_uuid, recipe_id.clone(), *reason);
                }
                Response::LockRecipe { recipe_id } => {
                    self.lock_recipe(source_uuid, recipe_id.clone());
                }
            }
        }
    }
}
