//! WorldContext implementation for ServerContext.

use basalt_core::WorldContext;
use basalt_core::components::{BlockPosition, ChunkPosition};

use super::ServerContext;
use super::response::Response;

impl WorldContext for ServerContext {
    fn world(&self) -> &basalt_world::World {
        &self.world
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
