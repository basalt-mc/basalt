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

    fn open_crafting_table(&self, x: i32, y: i32, z: i32) {
        self.responses.push(Response::OpenCraftingTable {
            position: BlockPosition { x, y, z },
        });
    }

    fn open(&self, container: &basalt_core::container::Container) {
        self.responses
            .push(Response::OpenContainer(container.clone()));
    }
}
