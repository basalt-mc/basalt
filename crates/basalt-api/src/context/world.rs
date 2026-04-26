//! WorldHandle and WorldContext implementations for ServerContext.

use crate::components::{BlockPosition, ChunkPosition};
use crate::context::WorldContext;
use crate::world::collision::{Aabb, RayHit};
use crate::world::handle::WorldHandle;

use super::ServerContext;
use super::response::Response;

impl WorldHandle for ServerContext {
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

    fn persist_chunk(&self, cx: i32, cz: i32) {
        self.world.persist_chunk(cx, cz);
    }

    fn dirty_chunks(&self) -> Vec<(i32, i32)> {
        self.world.dirty_chunks()
    }

    fn check_overlap(&self, aabb: &Aabb) -> bool {
        crate::world::collision::check_overlap(&self.world, aabb)
    }

    fn ray_cast(
        &self,
        origin: (f64, f64, f64),
        direction: (f64, f64, f64),
        max_distance: f64,
    ) -> Option<RayHit> {
        crate::world::collision::ray_cast(&self.world, origin, direction, max_distance)
    }

    fn resolve_movement(&self, aabb: &Aabb, dx: f64, dy: f64, dz: f64) -> (f64, f64, f64) {
        crate::world::collision::resolve_movement(&self.world, aabb, dx, dy, dz)
    }
}

impl WorldContext for ServerContext {
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
