//! Network layer — per-player async tasks and shared state.

pub(crate) mod channels;
pub(crate) mod chunk_cache;
pub(crate) mod connection;
mod play_handler;
pub(crate) mod play_sender;
pub(crate) mod skin;
pub(crate) mod task;
