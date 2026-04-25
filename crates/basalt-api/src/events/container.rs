//! Container events: open/close lifecycle, clicks, drags, block entities.

use crate::components::BlockPosition;
use crate::container::{ContainerBacking, InventoryType};
use basalt_types::Slot;
use basalt_world::block_entity::BlockEntity;

// ---------------------------------------------------------------------------
// Helper enums
// ---------------------------------------------------------------------------

/// Why a container window is closing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    /// Player pressed E / ESC.
    Manual,
    /// Player disconnected from the server.
    Disconnect,
    /// Server-initiated close (e.g., admin command).
    ForceClose,
}

/// Categorises which slot was clicked, independent of window type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowSlotKind {
    /// Crafting output slot (in crafting table or player inventory 2x2).
    CraftOutput,
    /// Crafting grid input slot.
    CraftGrid,
    /// Armor slot.
    Armor,
    /// Main player inventory slot (rows under hotbar).
    MainInventory,
    /// Hotbar slot.
    Hotbar,
    /// Offhand slot.
    Offhand,
    /// Container slot (chest slot, hopper slot, etc.).
    Container,
}

/// Type of drag operation (paint mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragType {
    /// Left-click drag: distribute cursor items evenly across slots.
    LeftDrag,
    /// Right-click drag: place 1 item per slot.
    RightDrag,
    /// Middle-click drag: creative fill.
    MiddleDrag,
}

/// Mirror of server's click action that plugins can safely consume.
///
/// Excludes transient click phases (DropCursor, StartDrag, AddDragSlot,
/// EndDrag) which are internal details of the server's click processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerClickType {
    /// Normal left click on a slot.
    LeftClick,
    /// Normal right click on a slot.
    RightClick,
    /// Shift-click (quick-move).
    ShiftClick,
    /// Double-click (collect matching items).
    DoubleClick,
    /// Q-key drop from a slot.
    DropSlot {
        /// When true the entire stack is dropped (Ctrl+Q).
        drop_all: bool,
    },
    /// Number key swap with hotbar slot (0-8).
    HotbarSwap {
        /// Hotbar slot index (0-8).
        hotbar: u8,
    },
    /// Swap with offhand slot (F key).
    OffhandSwap,
}

/// Identifies the kind of block entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockEntityKind {
    /// A chest (single or part of a double chest).
    Chest,
    // Future: Furnace, Hopper, ShulkerBox, BrewingStand, etc.
}

// ---------------------------------------------------------------------------
// Container lifecycle events
// ---------------------------------------------------------------------------

/// Fires BEFORE a container window opens.
///
/// Cancellable -- plugins can deny the open (e.g., permission checks).
/// Fires during the Validate stage of the game bus.
#[derive(Debug, Clone)]
pub struct ContainerOpenRequestEvent {
    /// The inventory type being requested.
    pub inventory_type: InventoryType,
    /// How the container is backed (block or virtual).
    pub backing: ContainerBacking,
    /// Title that will be shown to the player.
    pub title: String,
    /// Whether the event has been cancelled.
    pub cancelled: bool,
}
crate::game_cancellable_event!(ContainerOpenRequestEvent);

/// Fires AFTER a container window has been opened for a player.
///
/// Non-cancellable. Fires during the Post stage.
#[derive(Debug, Clone)]
pub struct ContainerOpenedEvent {
    /// Protocol window ID assigned at open time (1-127).
    pub window_id: u8,
    /// The inventory type that was opened.
    pub inventory_type: InventoryType,
    /// How the container is backed.
    pub backing: ContainerBacking,
    /// Number of players viewing the same block-backed container,
    /// **including** the player who just opened it. Always at least 1
    /// for `Block` backings, 0 for `Virtual` backings (each virtual
    /// container is per-player).
    ///
    /// Used by `ContainerPlugin` to broadcast the chest-lid open
    /// animation with the right viewer count.
    pub viewer_count: u32,
}
crate::game_event!(ContainerOpenedEvent);

/// Fires BEFORE the `OpenContainer` component is removed from the player.
///
/// Non-cancellable. Fires during the Post stage.
#[derive(Debug, Clone)]
pub struct ContainerClosedEvent {
    /// Window ID that is being closed.
    pub window_id: u8,
    /// The inventory type.
    pub inventory_type: InventoryType,
    /// How the container was backed.
    pub backing: ContainerBacking,
    /// Why the container is closing.
    pub reason: CloseReason,
    /// Number of remaining viewers on the same block-backed container
    /// **excluding** the closing player. 0 for `Virtual` backings.
    ///
    /// Used by `ContainerPlugin` to broadcast the chest-lid close
    /// animation with the right remaining-viewer count (action
    /// param 0 closes the lid completely).
    pub viewer_count: u32,
    /// Snapshot of the player's `CraftingGrid` slots at the moment of
    /// close, populated **only** when `inventory_type ==
    /// InventoryType::Crafting` (3x3 crafting table). The server has
    /// already reset the grid to 2x2 by the time this event fires —
    /// plugins use the snapshot to spawn dropped items.
    ///
    /// `None` for any non-crafting close.
    pub crafting_grid_state: Option<[Slot; 9]>,
}
crate::game_event!(ContainerClosedEvent);

// ---------------------------------------------------------------------------
// Click / drag events
// ---------------------------------------------------------------------------

/// Fires BEFORE a click inside an open container is applied.
///
/// Cancellable -- plugins use this to implement GUI menus where
/// slots act as buttons. Fires during the Validate stage.
///
/// Only fires when a container is open (NOT for player inventory
/// clicks with no open container).
#[derive(Debug, Clone)]
pub struct ContainerClickEvent {
    /// Window ID of the open container.
    pub window_id: u8,
    /// How the container is backed.
    pub backing: ContainerBacking,
    /// Protocol slot index that was clicked.
    pub slot_index: i16,
    /// Logical categorisation of the clicked slot.
    pub window_slot_kind: WindowSlotKind,
    /// Type of click action (left, right, shift, etc.).
    pub click_type: ContainerClickType,
    /// Cursor item state immediately before the click.
    pub cursor_before: Slot,
    /// Whether the event has been cancelled.
    pub cancelled: bool,
}
crate::game_cancellable_event!(ContainerClickEvent);

/// Fires BEFORE a drag (paint mode) distribution is applied.
///
/// Cancellable -- plugins can prevent drag within GUIs.
/// Fires during the Validate stage.
#[derive(Debug, Clone)]
pub struct ContainerDragEvent {
    /// Window ID of the open container.
    pub window_id: u8,
    /// How the container is backed.
    pub backing: ContainerBacking,
    /// Slots affected by the drag (protocol slot index + planned result).
    pub affected_slots: Vec<(i16, Slot)>,
    /// Drag type (left/right/middle).
    pub drag_type: DragType,
    /// Cursor item before distribution.
    pub cursor: Slot,
    /// Whether the event has been cancelled.
    pub cancelled: bool,
}
crate::game_cancellable_event!(ContainerDragEvent);

/// Fires AFTER a container slot has changed.
///
/// Non-cancellable. Fires during the Post stage, once per changed
/// slot. Only fires for [`WindowSlotKind::Container`] -- not for craft
/// grid / inventory slots inside a container window.
#[derive(Debug, Clone)]
pub struct ContainerSlotChangedEvent {
    /// Window ID of the container.
    pub window_id: u8,
    /// How the container is backed.
    pub backing: ContainerBacking,
    /// Protocol slot index that changed.
    pub slot_index: i16,
    /// Slot state before the change.
    pub old: Slot,
    /// Slot state after the change.
    pub new: Slot,
}
crate::game_event!(ContainerSlotChangedEvent);

// ---------------------------------------------------------------------------
// Block entity events
// ---------------------------------------------------------------------------

/// Fires AFTER a block entity is created at a position that had none.
///
/// Non-cancellable. Fires during the Post stage.
#[derive(Debug, Clone)]
pub struct BlockEntityCreatedEvent {
    /// World position of the new block entity.
    pub position: BlockPosition,
    /// Kind of block entity that was created.
    pub kind: BlockEntityKind,
}
crate::game_event!(BlockEntityCreatedEvent);

/// Fires AFTER a block entity's data is modified.
///
/// Non-cancellable. Fires during the Post stage. Triggered by
/// slot writes, tick processing, or explicit replacements.
#[derive(Debug, Clone)]
pub struct BlockEntityModifiedEvent {
    /// World position of the block entity.
    pub position: BlockPosition,
    /// Kind of block entity that was modified.
    pub kind: BlockEntityKind,
}
crate::game_event!(BlockEntityModifiedEvent);

/// Fires AFTER a block entity is removed from the world.
///
/// Non-cancellable. Fires during the Post stage. Carries the last
/// state so plugins can drop contents, backup data, etc.
#[derive(Debug, Clone)]
pub struct BlockEntityDestroyedEvent {
    /// World position of the destroyed block entity.
    pub position: BlockPosition,
    /// Kind of block entity that was destroyed.
    pub kind: BlockEntityKind,
    /// Block entity state immediately before destruction.
    pub last_state: BlockEntity,
}
crate::game_event!(BlockEntityDestroyedEvent);

#[cfg(test)]
mod tests {
    use crate::events::{BusKind, Event, EventRouting};

    use super::*;

    // -- Helper enum construction and equality --------------------------------

    #[test]
    fn close_reason_variants() {
        assert_eq!(CloseReason::Manual, CloseReason::Manual);
        assert_eq!(CloseReason::Disconnect, CloseReason::Disconnect);
        assert_eq!(CloseReason::ForceClose, CloseReason::ForceClose);
        assert_ne!(CloseReason::Manual, CloseReason::Disconnect);
    }

    #[test]
    fn window_slot_kind_variants() {
        let variants = [
            WindowSlotKind::CraftOutput,
            WindowSlotKind::CraftGrid,
            WindowSlotKind::Armor,
            WindowSlotKind::MainInventory,
            WindowSlotKind::Hotbar,
            WindowSlotKind::Offhand,
            WindowSlotKind::Container,
        ];
        for (i, a) in variants.iter().enumerate() {
            assert_eq!(a, a, "variant should equal itself");
            for b in variants.iter().skip(i + 1) {
                assert_ne!(a, b, "distinct variants should differ");
            }
        }
    }

    #[test]
    fn drag_type_variants() {
        assert_eq!(DragType::LeftDrag, DragType::LeftDrag);
        assert_eq!(DragType::RightDrag, DragType::RightDrag);
        assert_eq!(DragType::MiddleDrag, DragType::MiddleDrag);
        assert_ne!(DragType::LeftDrag, DragType::RightDrag);
    }

    #[test]
    fn container_click_type_variants() {
        assert_eq!(ContainerClickType::LeftClick, ContainerClickType::LeftClick);
        assert_eq!(
            ContainerClickType::RightClick,
            ContainerClickType::RightClick
        );
        assert_eq!(
            ContainerClickType::ShiftClick,
            ContainerClickType::ShiftClick
        );
        assert_eq!(
            ContainerClickType::DoubleClick,
            ContainerClickType::DoubleClick
        );
        assert_eq!(
            ContainerClickType::DropSlot { drop_all: true },
            ContainerClickType::DropSlot { drop_all: true }
        );
        assert_ne!(
            ContainerClickType::DropSlot { drop_all: false },
            ContainerClickType::DropSlot { drop_all: true }
        );
        assert_eq!(
            ContainerClickType::HotbarSwap { hotbar: 3 },
            ContainerClickType::HotbarSwap { hotbar: 3 }
        );
        assert_ne!(
            ContainerClickType::HotbarSwap { hotbar: 0 },
            ContainerClickType::HotbarSwap { hotbar: 1 }
        );
        assert_eq!(
            ContainerClickType::OffhandSwap,
            ContainerClickType::OffhandSwap
        );
    }

    #[test]
    fn block_entity_kind_variants() {
        assert_eq!(BlockEntityKind::Chest, BlockEntityKind::Chest);
    }

    // -- Container lifecycle events -------------------------------------------

    #[test]
    fn container_open_request_cancellation() {
        let mut event = ContainerOpenRequestEvent {
            inventory_type: InventoryType::Generic9x3,
            backing: ContainerBacking::Virtual,
            title: "Test".to_string(),
            cancelled: false,
        };
        assert!(!event.is_cancelled());
        event.cancel();
        assert!(event.is_cancelled());
    }

    #[test]
    fn container_opened_construction() {
        let event = ContainerOpenedEvent {
            window_id: 1,
            inventory_type: InventoryType::Generic9x3,
            backing: ContainerBacking::Block {
                position: BlockPosition { x: 5, y: 64, z: 3 },
            },
            viewer_count: 1,
        };
        assert_eq!(event.window_id, 1);
        assert_eq!(event.inventory_type, InventoryType::Generic9x3);
        assert_eq!(event.viewer_count, 1);
    }

    #[test]
    fn container_opened_not_cancellable() {
        let mut event = ContainerOpenedEvent {
            window_id: 1,
            inventory_type: InventoryType::Generic9x3,
            backing: ContainerBacking::Virtual,
            viewer_count: 0,
        };
        event.cancel(); // no-op
        assert!(!event.is_cancelled());
    }

    #[test]
    fn container_closed_construction() {
        let event = ContainerClosedEvent {
            window_id: 2,
            inventory_type: InventoryType::Hopper,
            backing: ContainerBacking::Block {
                position: BlockPosition {
                    x: 10,
                    y: 32,
                    z: -5,
                },
            },
            reason: CloseReason::Manual,
            viewer_count: 0,
            crafting_grid_state: None,
        };
        assert_eq!(event.window_id, 2);
        assert_eq!(event.reason, CloseReason::Manual);
    }

    #[test]
    fn container_closed_not_cancellable() {
        let mut event = ContainerClosedEvent {
            window_id: 1,
            inventory_type: InventoryType::Generic9x3,
            backing: ContainerBacking::Virtual,
            reason: CloseReason::Disconnect,
            viewer_count: 0,
            crafting_grid_state: None,
        };
        event.cancel();
        assert!(!event.is_cancelled());
    }

    // -- Click / drag events --------------------------------------------------

    #[test]
    fn container_click_cancellation() {
        let mut event = ContainerClickEvent {
            window_id: 1,
            backing: ContainerBacking::Virtual,
            slot_index: 5,
            window_slot_kind: WindowSlotKind::Container,
            click_type: ContainerClickType::LeftClick,
            cursor_before: Slot::empty(),
            cancelled: false,
        };
        assert!(!event.is_cancelled());
        event.cancel();
        assert!(event.is_cancelled());
    }

    #[test]
    fn container_click_field_access() {
        let event = ContainerClickEvent {
            window_id: 3,
            backing: ContainerBacking::Block {
                position: BlockPosition { x: 0, y: 64, z: 0 },
            },
            slot_index: 12,
            window_slot_kind: WindowSlotKind::Hotbar,
            click_type: ContainerClickType::HotbarSwap { hotbar: 2 },
            cursor_before: Slot::new(1, 32),
            cancelled: false,
        };
        assert_eq!(event.slot_index, 12);
        assert_eq!(event.window_slot_kind, WindowSlotKind::Hotbar);
        assert_eq!(
            event.click_type,
            ContainerClickType::HotbarSwap { hotbar: 2 }
        );
    }

    #[test]
    fn container_drag_cancellation() {
        let mut event = ContainerDragEvent {
            window_id: 1,
            backing: ContainerBacking::Virtual,
            affected_slots: vec![(0, Slot::new(1, 16)), (1, Slot::new(1, 16))],
            drag_type: DragType::LeftDrag,
            cursor: Slot::new(1, 32),
            cancelled: false,
        };
        assert!(!event.is_cancelled());
        event.cancel();
        assert!(event.is_cancelled());
    }

    #[test]
    fn container_drag_field_access() {
        let event = ContainerDragEvent {
            window_id: 2,
            backing: ContainerBacking::Virtual,
            affected_slots: vec![(5, Slot::new(3, 1))],
            drag_type: DragType::RightDrag,
            cursor: Slot::new(3, 5),
            cancelled: false,
        };
        assert_eq!(event.affected_slots.len(), 1);
        assert_eq!(event.drag_type, DragType::RightDrag);
    }

    #[test]
    fn container_slot_changed_construction() {
        let event = ContainerSlotChangedEvent {
            window_id: 1,
            backing: ContainerBacking::Block {
                position: BlockPosition { x: 5, y: 64, z: 3 },
            },
            slot_index: 7,
            old: Slot::empty(),
            new: Slot::new(1, 1),
        };
        assert_eq!(event.slot_index, 7);
    }

    #[test]
    fn container_slot_changed_not_cancellable() {
        let mut event = ContainerSlotChangedEvent {
            window_id: 1,
            backing: ContainerBacking::Virtual,
            slot_index: 0,
            old: Slot::empty(),
            new: Slot::empty(),
        };
        event.cancel();
        assert!(!event.is_cancelled());
    }

    // -- Block entity events --------------------------------------------------

    #[test]
    fn block_entity_created_construction() {
        let event = BlockEntityCreatedEvent {
            position: BlockPosition { x: 5, y: 64, z: 3 },
            kind: BlockEntityKind::Chest,
        };
        assert_eq!(event.position, BlockPosition { x: 5, y: 64, z: 3 });
        assert_eq!(event.kind, BlockEntityKind::Chest);
    }

    #[test]
    fn block_entity_created_not_cancellable() {
        let mut event = BlockEntityCreatedEvent {
            position: BlockPosition { x: 0, y: 0, z: 0 },
            kind: BlockEntityKind::Chest,
        };
        event.cancel();
        assert!(!event.is_cancelled());
    }

    #[test]
    fn block_entity_modified_construction() {
        let event = BlockEntityModifiedEvent {
            position: BlockPosition {
                x: -10,
                y: 32,
                z: 100,
            },
            kind: BlockEntityKind::Chest,
        };
        assert_eq!(event.kind, BlockEntityKind::Chest);
    }

    #[test]
    fn block_entity_modified_not_cancellable() {
        let mut event = BlockEntityModifiedEvent {
            position: BlockPosition { x: 0, y: 0, z: 0 },
            kind: BlockEntityKind::Chest,
        };
        event.cancel();
        assert!(!event.is_cancelled());
    }

    #[test]
    fn block_entity_destroyed_carries_last_state() {
        let be = BlockEntity::empty_chest();
        let event = BlockEntityDestroyedEvent {
            position: BlockPosition {
                x: 100,
                y: 64,
                z: -50,
            },
            kind: BlockEntityKind::Chest,
            last_state: be,
        };
        match &event.last_state {
            BlockEntity::Chest { slots } => {
                assert_eq!(slots.len(), 27);
            }
        }
    }

    #[test]
    fn block_entity_destroyed_not_cancellable() {
        let mut event = BlockEntityDestroyedEvent {
            position: BlockPosition { x: 0, y: 0, z: 0 },
            kind: BlockEntityKind::Chest,
            last_state: BlockEntity::empty_chest(),
        };
        event.cancel();
        assert!(!event.is_cancelled());
    }

    // -- Bus kind routing -----------------------------------------------------

    #[test]
    fn all_events_route_to_game_bus() {
        assert_eq!(ContainerOpenRequestEvent::BUS, BusKind::Game);
        assert_eq!(ContainerOpenedEvent::BUS, BusKind::Game);
        assert_eq!(ContainerClosedEvent::BUS, BusKind::Game);
        assert_eq!(ContainerClickEvent::BUS, BusKind::Game);
        assert_eq!(ContainerDragEvent::BUS, BusKind::Game);
        assert_eq!(ContainerSlotChangedEvent::BUS, BusKind::Game);
        assert_eq!(BlockEntityCreatedEvent::BUS, BusKind::Game);
        assert_eq!(BlockEntityModifiedEvent::BUS, BusKind::Game);
        assert_eq!(BlockEntityDestroyedEvent::BUS, BusKind::Game);
    }

    #[test]
    fn bus_kind_method_matches_const() {
        let events_game: Vec<Box<dyn Event>> = vec![
            Box::new(ContainerOpenRequestEvent {
                inventory_type: InventoryType::Generic9x3,
                backing: ContainerBacking::Virtual,
                title: String::new(),
                cancelled: false,
            }),
            Box::new(ContainerOpenedEvent {
                window_id: 1,
                inventory_type: InventoryType::Generic9x3,
                backing: ContainerBacking::Virtual,
                viewer_count: 0,
            }),
            Box::new(ContainerClosedEvent {
                window_id: 1,
                inventory_type: InventoryType::Generic9x3,
                backing: ContainerBacking::Virtual,
                reason: CloseReason::Manual,
                viewer_count: 0,
                crafting_grid_state: None,
            }),
            Box::new(ContainerClickEvent {
                window_id: 1,
                backing: ContainerBacking::Virtual,
                slot_index: 0,
                window_slot_kind: WindowSlotKind::Container,
                click_type: ContainerClickType::LeftClick,
                cursor_before: Slot::empty(),
                cancelled: false,
            }),
            Box::new(ContainerDragEvent {
                window_id: 1,
                backing: ContainerBacking::Virtual,
                affected_slots: vec![],
                drag_type: DragType::LeftDrag,
                cursor: Slot::empty(),
                cancelled: false,
            }),
            Box::new(ContainerSlotChangedEvent {
                window_id: 1,
                backing: ContainerBacking::Virtual,
                slot_index: 0,
                old: Slot::empty(),
                new: Slot::empty(),
            }),
            Box::new(BlockEntityCreatedEvent {
                position: BlockPosition { x: 0, y: 0, z: 0 },
                kind: BlockEntityKind::Chest,
            }),
            Box::new(BlockEntityModifiedEvent {
                position: BlockPosition { x: 0, y: 0, z: 0 },
                kind: BlockEntityKind::Chest,
            }),
            Box::new(BlockEntityDestroyedEvent {
                position: BlockPosition { x: 0, y: 0, z: 0 },
                kind: BlockEntityKind::Chest,
                last_state: BlockEntity::empty_chest(),
            }),
        ];
        for event in &events_game {
            assert_eq!(event.bus_kind(), BusKind::Game);
        }
    }
}
