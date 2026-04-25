//! Hand-rolled protocol types — used as a deliberate exception when
//! the codegen IR cannot model a Mojang protocol pattern cleanly.
//!
//! Today only [`IDSet`] qualifies: its wire format encodes the
//! variant count in the discriminator tag itself (`tag = N + 1` for
//! `N` inline varints), which no other Mojang type uses. Everything
//! else — including the recursive switch-on-tag unions
//! `RecipeDisplay` / `SlotDisplay` — is now produced by the codegen
//! and lives under [`crate::packets::play::types`].

pub mod id_set;

pub use id_set::IDSet;
