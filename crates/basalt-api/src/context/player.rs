//! PlayerContext implementation for ServerContext.

use std::sync::atomic::{AtomicI32, Ordering};

use basalt_core::PlayerContext;
use basalt_core::components::{Position, Rotation};
use basalt_core::gamemode::Gamemode;
use basalt_types::Uuid;

use super::ServerContext;
use super::response::Response;

/// Game state change reason code for gamemode changes.
const GAME_STATE_CHANGE_GAMEMODE: u8 = 3;

/// Global teleport ID counter shared across all server contexts.
static GLOBAL_TELEPORT_COUNTER: AtomicI32 = AtomicI32::new(1);

impl PlayerContext for ServerContext {
    fn uuid(&self) -> Uuid {
        self.player.uuid
    }
    fn entity_id(&self) -> i32 {
        self.player.entity_id
    }
    fn username(&self) -> &str {
        &self.player.username
    }
    fn yaw(&self) -> f32 {
        self.player.rotation.yaw
    }
    fn pitch(&self) -> f32 {
        self.player.rotation.pitch
    }
    fn teleport(&self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
        let teleport_id = GLOBAL_TELEPORT_COUNTER.fetch_add(1, Ordering::Relaxed);
        self.responses.push(Response::SendPosition {
            teleport_id,
            position: Position { x, y, z },
            rotation: Rotation { yaw, pitch },
        });
    }
    fn set_gamemode(&self, mode: Gamemode) {
        self.responses.push(Response::SendGameStateChange {
            reason: GAME_STATE_CHANGE_GAMEMODE,
            value: mode.id() as f32,
        });
    }
    fn registered_commands(&self) -> Vec<(String, String)> {
        self.command_list.borrow().clone()
    }
}
