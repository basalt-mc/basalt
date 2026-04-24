//! Response processing — translates event handler responses into server output.

use std::sync::Arc;

use basalt_api::context::Response;
use basalt_types::Uuid;

use super::{GameLoop, OutputHandle};
use crate::messages::{BroadcastEvent, ServerOutput, SharedBroadcast};

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
                    // Invalidate chunk cache for this block's chunk
                    self.chunk_cache.invalidate(*x >> 4, *z >> 4);
                    let bc = Arc::new(SharedBroadcast::new(BroadcastEvent::BlockChanged {
                        x: *x,
                        y: *y,
                        z: *z,
                        state: *block_state,
                    }));
                    for (e, _) in self.ecs.iter::<OutputHandle>() {
                        self.send_to(e, |tx| {
                            let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&bc)));
                        });
                    }
                }
                Response::Broadcast(basalt_api::broadcast::BroadcastMessage::Chat { content }) => {
                    let bc = Arc::new(SharedBroadcast::new(BroadcastEvent::SystemChat {
                        content: content.clone(),
                        action_bar: false,
                    }));
                    for (e, _) in self.ecs.iter::<OutputHandle>() {
                        self.send_to(e, |tx| {
                            let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&bc)));
                        });
                    }
                }
                Response::Broadcast(_) => {}
                Response::SendBlockAck { sequence } => {
                    if let Some(eid) = self.find_by_uuid(source_uuid)
                        && let Some(handle) = self.ecs.get::<OutputHandle>(eid)
                    {
                        let _ = handle.tx.try_send(ServerOutput::BlockAck {
                            sequence: *sequence,
                        });
                    }
                }
                Response::SendSystemChat {
                    content,
                    action_bar,
                } => {
                    if let Some(eid) = self.find_by_uuid(source_uuid)
                        && let Some(handle) = self.ecs.get::<OutputHandle>(eid)
                    {
                        let _ = handle.tx.try_send(ServerOutput::SystemChat {
                            content: content.clone(),
                            action_bar: *action_bar,
                        });
                    }
                }
                Response::SendPosition {
                    teleport_id,
                    position,
                    rotation,
                } => {
                    if let Some(eid) = self.find_by_uuid(source_uuid) {
                        if let Some(pos) = self.ecs.get_mut::<basalt_core::Position>(eid) {
                            pos.x = position.x;
                            pos.y = position.y;
                            pos.z = position.z;
                        }
                        if let Some(handle) = self.ecs.get::<OutputHandle>(eid) {
                            let _ = handle.tx.try_send(ServerOutput::SetPosition {
                                teleport_id: *teleport_id,
                                x: position.x,
                                y: position.y,
                                z: position.z,
                                yaw: rotation.yaw,
                                pitch: rotation.pitch,
                            });
                        }
                    }
                }
                Response::StreamChunks(chunk) => {
                    if let Some(eid) = self.find_by_uuid(source_uuid) {
                        self.stream_chunks(eid, chunk.x, chunk.z);
                    }
                }
                Response::SendGameStateChange { reason, value } => {
                    if let Some(eid) = self.find_by_uuid(source_uuid)
                        && let Some(handle) = self.ecs.get::<OutputHandle>(eid)
                    {
                        let _ = handle.tx.try_send(ServerOutput::GameStateChange {
                            reason: *reason,
                            value: *value,
                        });
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
            }
        }
    }
}
