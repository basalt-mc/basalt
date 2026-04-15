//! Gamemode type for the Minecraft protocol.
//!
//! Provides a type-safe enum instead of raw `u8` values for gamemodes,
//! preventing invalid values from reaching the protocol layer.

use std::fmt;

/// A Minecraft game mode.
///
/// Used by [`Context::set_gamemode`](crate::Context::set_gamemode)
/// and the `/gamemode` command. Maps directly to the protocol values
/// sent in the GameStateChange packet (reason = 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gamemode {
    /// Survival mode (id 0): resource gathering, health, hunger.
    Survival = 0,
    /// Creative mode (id 1): unlimited resources, flight, no damage.
    Creative = 1,
    /// Adventure mode (id 2): survival with block interaction restrictions.
    Adventure = 2,
    /// Spectator mode (id 3): invisible, fly through blocks, no interaction.
    Spectator = 3,
}

impl Gamemode {
    /// Returns the protocol ID for this gamemode.
    pub fn id(self) -> u8 {
        self as u8
    }

    /// Creates a gamemode from its protocol ID.
    ///
    /// Returns `None` for invalid IDs (anything other than 0-3).
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::Survival),
            1 => Some(Self::Creative),
            2 => Some(Self::Adventure),
            3 => Some(Self::Spectator),
            _ => None,
        }
    }
}

impl fmt::Display for Gamemode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Gamemode::Survival => write!(f, "Survival"),
            Gamemode::Creative => write!(f, "Creative"),
            Gamemode::Adventure => write!(f, "Adventure"),
            Gamemode::Spectator => write!(f, "Spectator"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamemode_ids() {
        assert_eq!(Gamemode::Survival.id(), 0);
        assert_eq!(Gamemode::Creative.id(), 1);
        assert_eq!(Gamemode::Adventure.id(), 2);
        assert_eq!(Gamemode::Spectator.id(), 3);
    }

    #[test]
    fn from_valid_id() {
        assert_eq!(Gamemode::from_id(0), Some(Gamemode::Survival));
        assert_eq!(Gamemode::from_id(1), Some(Gamemode::Creative));
        assert_eq!(Gamemode::from_id(2), Some(Gamemode::Adventure));
        assert_eq!(Gamemode::from_id(3), Some(Gamemode::Spectator));
    }

    #[test]
    fn from_invalid_id() {
        assert_eq!(Gamemode::from_id(4), None);
        assert_eq!(Gamemode::from_id(255), None);
    }

    #[test]
    fn display() {
        assert_eq!(Gamemode::Survival.to_string(), "Survival");
        assert_eq!(Gamemode::Creative.to_string(), "Creative");
        assert_eq!(Gamemode::Adventure.to_string(), "Adventure");
        assert_eq!(Gamemode::Spectator.to_string(), "Spectator");
    }
}
