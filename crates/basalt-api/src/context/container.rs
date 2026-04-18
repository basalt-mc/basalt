//! ContainerContext implementation for ServerContext.

use basalt_core::ContainerContext;
use basalt_core::components::BlockPosition;

use super::ServerContext;
use super::response::Response;

impl ContainerContext for ServerContext {
    fn open_chest(&self, x: i32, y: i32, z: i32) {
        self.responses
            .push(Response::OpenChest(BlockPosition { x, y, z }));
    }
}
