//! Dedicated I/O thread for disk persistence.
//!
//! Receives chunk persist requests via an MPSC channel and writes them
//! to BSR region files with LZ4 compression. Never blocks the game
//! loop — all disk I/O happens on this dedicated OS thread.
//!
//! On graceful shutdown, all pending requests are flushed before exit.

use std::sync::Arc;
use std::thread::{self, JoinHandle};

use tokio::sync::mpsc;

/// A request sent to the I/O thread.
pub(crate) enum IoRequest {
    /// Persist a chunk to disk.
    PersistChunk {
        /// Chunk X coordinate.
        cx: i32,
        /// Chunk Z coordinate.
        cz: i32,
    },
    /// Shut down the I/O thread after flushing pending requests.
    Shutdown,
}

/// A dedicated OS thread for disk persistence.
///
/// Receives [`IoRequest`] messages via a channel and processes them
/// sequentially. The game loop sends persist requests here instead
/// of calling `World::persist_chunk()` directly, so disk I/O never
/// blocks a game tick.
pub(crate) struct IoThread {
    /// Sender for submitting persist requests.
    tx: mpsc::UnboundedSender<IoRequest>,
    /// Handle to the OS thread, consumed on shutdown.
    handle: Option<JoinHandle<()>>,
}

impl IoThread {
    /// Spawns the I/O thread.
    ///
    /// The thread runs until it receives [`IoRequest::Shutdown`] or
    /// the channel is closed. It processes requests sequentially in
    /// FIFO order.
    pub fn start(world: Arc<basalt_world::World>) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel();

        let handle = thread::Builder::new()
            .name("io-thread".into())
            .spawn(move || {
                log::info!(target: "basalt::io", "I/O thread started");
                while let Some(req) = rx.blocking_recv() {
                    match req {
                        IoRequest::PersistChunk { cx, cz } => {
                            world.persist_chunk(cx, cz);
                        }
                        IoRequest::Shutdown => {
                            // Flush remaining requests before exiting
                            while let Ok(req) = rx.try_recv() {
                                if let IoRequest::PersistChunk { cx, cz } = req {
                                    world.persist_chunk(cx, cz);
                                }
                            }
                            log::info!(target: "basalt::io", "I/O thread shutting down");
                            break;
                        }
                    }
                }
            })
            .expect("failed to spawn I/O thread");

        Self {
            tx,
            handle: Some(handle),
        }
    }

    /// Returns a clone of the sender for submitting persist requests.
    pub fn sender(&self) -> mpsc::UnboundedSender<IoRequest> {
        self.tx.clone()
    }

    /// Signals the I/O thread to flush and shut down, then joins it.
    pub fn stop(&mut self) {
        let _ = self.tx.send(IoRequest::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for IoThread {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_thread_starts_and_stops() {
        let world = Arc::new(basalt_world::World::new_memory(42));
        let mut io = IoThread::start(world);
        let tx = io.sender();

        // Should be able to send a request
        tx.send(IoRequest::PersistChunk { cx: 0, cz: 0 }).unwrap();

        io.stop();
    }

    #[test]
    fn io_thread_drop_stops_cleanly() {
        let world = Arc::new(basalt_world::World::new_memory(42));
        {
            let _io = IoThread::start(world);
            // Dropped here — should stop cleanly
        }
    }
}
