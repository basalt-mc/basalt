//! Component for storing slots of virtual (non-block-backed) containers.

use crate::components::Component;

/// Holds the slot contents of a virtual container window.
///
/// Set on a player entity when they open a virtual container (GUI menu
/// with no backing block). Removed on CloseWindow. For block-backed
/// containers, slots live in the block entity instead.
#[derive(Debug, Clone)]
pub struct VirtualContainerSlots {
    /// The container slots (count matches `inventory_type.slot_count()`).
    pub slots: Vec<basalt_types::Slot>,
}

impl Component for VirtualContainerSlots {}
