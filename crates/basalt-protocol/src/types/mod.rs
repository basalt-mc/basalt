//! Hand-rolled protocol types that the codegen IR cannot represent.
//!
//! These are switch-on-tag union types whose variant data is laid out
//! directly after the tag with no length prefix. The codegen falls back
//! to opaque `Vec<u8>` for them, which produces invalid wire bytes (the
//! default `Encode for Vec<u8>` adds a varint length prefix that the
//! protocol does not expect). Anything in this module is the
//! authoritative encoding — the matching codegen'd structs in
//! `crate::packets` are dead code today.

pub mod recipe_display;

pub use recipe_display::{IDSet, RecipeBookEntry, RecipeDisplay, SlotDisplay};
