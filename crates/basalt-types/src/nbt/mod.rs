//! In-house NBT (Named Binary Tag) implementation for the Minecraft protocol.
//!
//! NBT is Minecraft's binary data format used for chunk data, entity metadata,
//! item stacks, and since 1.20.3, chat components (TextComponent). This
//! implementation covers the protocol-relevant subset without external
//! dependencies (no fastnbt, simdnbt, or serde).
//!
//! The encoding integrates natively with the crate's `Encode`/`Decode` traits.

mod decode;
mod encode;
mod tag;

pub use tag::{NbtCompound, NbtList, NbtTag};
