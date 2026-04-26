//! Block entities — persistent per-block state.
//!
//! Block entities store data that standard block states cannot represent,
//! such as chest inventories, furnace cook progress, or sign text.
//! They are keyed by absolute world position and persisted with the chunk.

use basalt_types::Slot;

/// A block entity with typed data.
///
/// Each variant holds the state specific to that block type.
/// New variants are added as more interactive blocks are implemented.
#[derive(Debug, Clone)]
pub enum BlockEntity {
    /// A chest with 27 item slots (3 rows of 9).
    Chest {
        /// The 27 inventory slots.
        slots: Box<[Slot; 27]>,
    },
}

/// Type discriminator for [`BlockEntity`].
///
/// A small `Copy`/`Eq`/`Hash` enum used to identify the kind of a
/// block entity without owning its data. Use [`BlockEntity::kind`] to
/// extract the discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockEntityKind {
    /// A chest block entity.
    Chest,
}

impl BlockEntity {
    /// Creates a new empty chest block entity.
    pub fn empty_chest() -> Self {
        Self::Chest {
            slots: Box::new(std::array::from_fn(|_| Slot::empty())),
        }
    }

    /// Returns the [`BlockEntityKind`] discriminator.
    pub fn kind(&self) -> BlockEntityKind {
        match self {
            Self::Chest { .. } => BlockEntityKind::Chest,
        }
    }
}

impl From<&BlockEntity> for BlockEntityKind {
    fn from(be: &BlockEntity) -> Self {
        be.kind()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_chest_has_27_empty_slots() {
        let be = BlockEntity::empty_chest();
        match &be {
            BlockEntity::Chest { slots } => {
                assert_eq!(slots.len(), 27);
                assert!(slots.iter().all(|s| s.is_empty()));
            }
        }
    }

    #[test]
    fn empty_chest_has_chest_kind() {
        let be = BlockEntity::empty_chest();
        assert_eq!(be.kind(), BlockEntityKind::Chest);
        assert_eq!(BlockEntityKind::from(&be), BlockEntityKind::Chest);
    }
}
