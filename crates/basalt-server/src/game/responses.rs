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
                Response::Broadcast(basalt_api::BroadcastMessage::BlockChanged {
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
                Response::Broadcast(basalt_api::BroadcastMessage::Chat { content }) => {
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
                    if let Some(eid) = self.ecs.find_by_uuid(source_uuid)
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
                    if let Some(eid) = self.ecs.find_by_uuid(source_uuid)
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
                    x,
                    y,
                    z,
                    yaw,
                    pitch,
                } => {
                    if let Some(eid) = self.ecs.find_by_uuid(source_uuid) {
                        if let Some(pos) = self.ecs.get_mut::<basalt_ecs::Position>(eid) {
                            pos.x = *x;
                            pos.y = *y;
                            pos.z = *z;
                        }
                        if let Some(handle) = self.ecs.get::<OutputHandle>(eid) {
                            let _ = handle.tx.try_send(ServerOutput::SetPosition {
                                teleport_id: *teleport_id,
                                x: *x,
                                y: *y,
                                z: *z,
                                yaw: *yaw,
                                pitch: *pitch,
                            });
                        }
                    }
                }
                Response::StreamChunks { new_cx, new_cz } => {
                    if let Some(eid) = self.ecs.find_by_uuid(source_uuid) {
                        self.stream_chunks(eid, *new_cx, *new_cz);
                    }
                }
                Response::SendGameStateChange { reason, value } => {
                    if let Some(eid) = self.ecs.find_by_uuid(source_uuid)
                        && let Some(handle) = self.ecs.get::<OutputHandle>(eid)
                    {
                        let _ = handle.tx.try_send(ServerOutput::GameStateChange {
                            reason: *reason,
                            value: *value,
                        });
                    }
                }
                Response::PersistChunk { cx, cz } => {
                    let _ = self
                        .io_tx
                        .send(crate::runtime::io_thread::IoRequest::PersistChunk {
                            cx: *cx,
                            cz: *cz,
                        });
                }
                Response::SpawnDroppedItem {
                    x,
                    y,
                    z,
                    item_id,
                    count,
                } => {
                    self.spawn_item_entity(*x, *y, *z, *item_id, *count);
                }
                Response::OpenChest { x, y, z } => {
                    if let Some(eid) = self.ecs.find_by_uuid(source_uuid) {
                        self.open_chest(eid, *x, *y, *z);
                    }
                }
            }
        }
    }
}
