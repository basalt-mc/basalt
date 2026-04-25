//! Per-tick chunk-batch drainer.
//!
//! Each tick, [`GameLoop::drain_chunk_batches`] visits every player
//! holding a non-empty pending queue and ships up to
//! `floor(desired_chunks_per_tick)` chunks, wrapped in
//! `ChunkBatchStart` / `ChunkBatchFinished`. The negotiated rate
//! comes from `ServerboundPlayChunkBatchReceived` (see
//! `dispatch.rs`); on join it's seeded from
//! `chunk_batch_initial_rate`.

use super::{ChunkStreamRate, ChunkView, GameLoop};
use crate::messages::ServerOutput;

impl GameLoop {
    /// Drains pending chunks for every connected player, respecting
    /// each player's per-tick budget.
    ///
    /// Behavior per player:
    /// - Budget = `floor(desired_chunks_per_tick)`. A budget of zero
    ///   skips the player this tick — clients reporting <1 chunk/tick
    ///   are effectively paused. The clamp floor (`0.01`) prevents
    ///   permanent stalls while keeping the integer-budget model.
    /// - Pops up to `min(budget, pending.len())` entries off the front
    ///   of the queue. Order is preserved — entries enqueued earlier
    ///   ship earlier.
    /// - Marks each shipped chunk as loaded in [`ChunkView`] before
    ///   sending so a concurrent boundary crossing can compute the
    ///   correct unload set.
    /// - Wraps the burst in `ChunkBatchStart` / `ChunkBatchFinished`,
    ///   which the 1.21.4 client uses to time its next decode-rate
    ///   report.
    pub(super) fn drain_chunk_batches(&mut self) {
        // Snapshot of player entities with non-empty queues. We can't
        // hold the iterator's borrow across the per-player work because
        // the chunk-cache lookup needs `&mut self` (via send_chunk_with_entities).
        let candidates: Vec<basalt_ecs::EntityId> = self
            .ecs
            .iter::<ChunkStreamRate>()
            .filter(|(_, r)| !r.pending.is_empty())
            .map(|(eid, _)| eid)
            .collect();

        for eid in candidates {
            let to_send: Vec<(i32, i32)> = {
                let Some(rate) = self.ecs.get_mut::<ChunkStreamRate>(eid) else {
                    continue;
                };
                let budget = rate.desired_chunks_per_tick.floor() as usize;
                if budget == 0 {
                    continue;
                }
                let n = budget.min(rate.pending.len());
                (0..n).filter_map(|_| rate.pending.pop_front()).collect()
            };

            if to_send.is_empty() {
                continue;
            }

            if let Some(view) = self.ecs.get_mut::<ChunkView>(eid) {
                for &k in &to_send {
                    view.loaded_chunks.insert(k);
                }
            }

            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::ChunkBatchStart);
            });
            for (cx, cz) in &to_send {
                self.send_chunk_with_entities(eid, *cx, *cz);
            }
            let batch_size = to_send.len() as i32;
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::ChunkBatchFinished { batch_size });
            });
        }
    }
}
