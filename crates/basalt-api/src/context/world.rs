//! WorldContext implementation for ServerContext.

use crate::components::{BlockPosition, ChunkPosition};
use crate::context::WorldContext;

use super::ServerContext;
use super::response::Response;

impl WorldContext for ServerContext {
    fn world(&self) -> &basalt_world::World {
        &self.world
    }
    fn get_block(&self, x: i32, y: i32, z: i32) -> u16 {
        self.world.get_block(x, y, z)
    }
    fn set_block(&self, x: i32, y: i32, z: i32, state: u16) {
        self.world.set_block(x, y, z, state);
    }
    fn get_block_entity(
        &self,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<basalt_world::block_entity::BlockEntity> {
        self.world.get_block_entity(x, y, z).map(|r| r.clone())
    }
    fn set_block_entity(
        &self,
        x: i32,
        y: i32,
        z: i32,
        entity: basalt_world::block_entity::BlockEntity,
    ) {
        self.world.set_block_entity(x, y, z, entity);
    }
    fn mark_chunk_dirty(&self, cx: i32, cz: i32) {
        self.world.mark_chunk_dirty(cx, cz);
    }
    fn send_block_ack(&self, sequence: i32) {
        self.responses.push(Response::SendBlockAck { sequence });
    }
    fn stream_chunks(&self, cx: i32, cz: i32) {
        self.responses
            .push(Response::StreamChunks(ChunkPosition { x: cx, z: cz }));
    }
    fn persist_chunk(&self, cx: i32, cz: i32) {
        self.responses
            .push(Response::PersistChunk(ChunkPosition { x: cx, z: cz }));
    }
    fn destroy_block_entity(&self, x: i32, y: i32, z: i32) {
        self.responses.push(Response::DestroyBlockEntity {
            position: BlockPosition { x, y, z },
        });
    }
}
