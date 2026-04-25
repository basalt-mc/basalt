//! ContainerContext implementation for ServerContext.

use crate::components::BlockPosition;
use crate::context::ContainerContext;

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

    fn open(&self, container: &crate::container::Container) {
        self.responses
            .push(Response::OpenContainer(container.clone()));
    }

    fn notify_viewers(&self, x: i32, y: i32, z: i32, slot_index: i16, item: basalt_types::Slot) {
        self.responses.push(Response::NotifyContainerViewers {
            position: BlockPosition { x, y, z },
            slot_index,
            item,
        });
    }
}
