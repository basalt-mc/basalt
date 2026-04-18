//! EntityContext implementation for ServerContext.

use basalt_core::EntityContext;
use basalt_core::broadcast::BroadcastMessage;
use basalt_core::components::BlockPosition;

use super::ServerContext;
use super::response::Response;

impl EntityContext for ServerContext {
    fn spawn_dropped_item(&self, x: i32, y: i32, z: i32, item_id: i32, count: i32) {
        self.responses.push(Response::SpawnDroppedItem {
            position: BlockPosition { x, y, z },
            item_id,
            count,
        });
    }
    fn broadcast_block_change(&self, x: i32, y: i32, z: i32, block_state: i32) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::BlockChanged {
                x,
                y,
                z,
                block_state,
            }));
    }
    #[allow(clippy::too_many_arguments)]
    fn broadcast_entity_moved(
        &self,
        entity_id: i32,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    ) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::EntityMoved {
                entity_id,
                x,
                y,
                z,
                yaw,
                pitch,
                on_ground,
            }));
    }
    fn broadcast_player_joined(&self) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::PlayerJoined {
                info: basalt_core::PlayerSnapshot {
                    username: self.player.username.clone(),
                    uuid: self.player.uuid,
                    entity_id: self.player.entity_id,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    yaw: self.player.rotation.yaw,
                    pitch: self.player.rotation.pitch,
                    skin_properties: Vec::new(),
                },
            }));
    }
    fn broadcast_player_left(&self) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::PlayerLeft {
                uuid: self.player.uuid,
                entity_id: self.player.entity_id,
                username: self.player.username.clone(),
            }));
    }
    fn broadcast_raw(&self, msg: BroadcastMessage) {
        self.responses.push(Response::Broadcast(msg));
    }
}
