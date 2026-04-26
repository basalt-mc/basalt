//! No-op context implementation for unit tests.

use crate::broadcast::BroadcastMessage;
use crate::context::{
    ChatContext, ContainerContext, Context, EntityContext, PlayerContext, RecipeContext,
    UnlockReason, WorldContext,
};
use crate::gamemode::Gamemode;
use crate::logger::PluginLogger;
use crate::recipes::RecipeId;
use crate::world::handle::WorldHandle;
use basalt_types::{TextComponent, Uuid};

/// A no-op [`Context`] implementation for unit tests.
///
/// All methods are stubs that return sensible defaults (air for blocks,
/// no collision, no block entities).
pub struct NoopContext;

impl PlayerContext for NoopContext {
    fn uuid(&self) -> Uuid {
        Uuid::default()
    }
    fn entity_id(&self) -> i32 {
        1
    }
    fn username(&self) -> &str {
        "Steve"
    }
    fn yaw(&self) -> f32 {
        0.0
    }
    fn pitch(&self) -> f32 {
        0.0
    }
    fn position(&self) -> (f64, f64, f64) {
        (0.0, 0.0, 0.0)
    }
    fn teleport(&self, _x: f64, _y: f64, _z: f64, _yaw: f32, _pitch: f32) {}
    fn set_gamemode(&self, _mode: Gamemode) {}
    fn registered_commands(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}

impl ChatContext for NoopContext {
    fn send(&self, _text: &str) {}
    fn send_component(&self, _component: &TextComponent) {}
    fn action_bar(&self, _text: &str) {}
    fn broadcast(&self, _text: &str) {}
    fn broadcast_component(&self, _component: &TextComponent) {}
}

impl WorldHandle for NoopContext {
    fn get_block(&self, _x: i32, _y: i32, _z: i32) -> u16 {
        0
    }
    fn set_block(&self, _x: i32, _y: i32, _z: i32, _state: u16) {}
    fn get_block_entity(
        &self,
        _x: i32,
        _y: i32,
        _z: i32,
    ) -> Option<crate::world::block_entity::BlockEntity> {
        None
    }
    fn set_block_entity(
        &self,
        _x: i32,
        _y: i32,
        _z: i32,
        _entity: crate::world::block_entity::BlockEntity,
    ) {
    }
    fn mark_chunk_dirty(&self, _cx: i32, _cz: i32) {}
    fn persist_chunk(&self, _cx: i32, _cz: i32) {}
    fn dirty_chunks(&self) -> Vec<(i32, i32)> {
        Vec::new()
    }
    fn check_overlap(&self, _aabb: &crate::world::collision::Aabb) -> bool {
        false
    }
    fn ray_cast(
        &self,
        _origin: (f64, f64, f64),
        _direction: (f64, f64, f64),
        _max_distance: f64,
    ) -> Option<crate::world::collision::RayHit> {
        None
    }
    fn resolve_movement(
        &self,
        _aabb: &crate::world::collision::Aabb,
        dx: f64,
        dy: f64,
        dz: f64,
    ) -> (f64, f64, f64) {
        (dx, dy, dz)
    }
}

impl WorldContext for NoopContext {
    fn send_block_ack(&self, _sequence: i32) {}
    fn stream_chunks(&self, _cx: i32, _cz: i32) {}
    fn queue_persist_chunk(&self, _cx: i32, _cz: i32) {}
    fn destroy_block_entity(&self, _x: i32, _y: i32, _z: i32) {}
}

impl EntityContext for NoopContext {
    fn spawn_dropped_item(&self, _x: i32, _y: i32, _z: i32, _item_id: i32, _count: i32) {}
    fn broadcast_block_change(&self, _x: i32, _y: i32, _z: i32, _block_state: i32) {}
    #[allow(clippy::too_many_arguments)]
    fn broadcast_entity_moved(
        &self,
        _entity_id: i32,
        _x: f64,
        _y: f64,
        _z: f64,
        _yaw: f32,
        _pitch: f32,
        _on_ground: bool,
    ) {
    }
    fn broadcast_player_joined(&self) {}
    fn broadcast_player_left(&self) {}
    fn broadcast_raw(&self, _msg: BroadcastMessage) {}
    fn broadcast_block_action(
        &self,
        _x: i32,
        _y: i32,
        _z: i32,
        _action_id: u8,
        _action_param: u8,
        _block_id: i32,
    ) {
    }
}

impl ContainerContext for NoopContext {
    fn open_chest(&self, _x: i32, _y: i32, _z: i32) {}
    fn open_crafting_table(&self, _x: i32, _y: i32, _z: i32) {}
    fn open(&self, _container: &crate::container::Container) {}
    fn notify_viewers(
        &self,
        _x: i32,
        _y: i32,
        _z: i32,
        _slot_index: i16,
        _item: basalt_types::Slot,
    ) {
    }
}

impl RecipeContext for NoopContext {
    fn unlock(&self, _id: &RecipeId, _reason: UnlockReason) {}
    fn lock(&self, _id: &RecipeId) {}
    fn has(&self, _id: &RecipeId) -> bool {
        false
    }
    fn unlocked(&self) -> Vec<RecipeId> {
        Vec::new()
    }
}

impl Context for NoopContext {
    fn logger(&self) -> PluginLogger {
        PluginLogger::new("test")
    }
    fn player(&self) -> &dyn PlayerContext {
        self
    }
    fn chat(&self) -> &dyn ChatContext {
        self
    }
    fn world_ctx(&self) -> &dyn WorldContext {
        self
    }
    fn entities(&self) -> &dyn EntityContext {
        self
    }
    fn containers(&self) -> &dyn ContainerContext {
        self
    }
    fn recipes(&self) -> &dyn RecipeContext {
        self
    }
}
