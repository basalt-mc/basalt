//! Click parsing, drag state, and window slot resolution.
//!
//! Converts raw protocol (slot, button, mode) triples from the
//! WindowClick packet into typed [`ClickAction`] variants, tracks
//! multi-packet drag operations via [`DragState`], and resolves
//! protocol slot numbers to logical [`WindowSlot`] locations.

/// A parsed click action from the client's WindowClick packet.
///
/// Converts raw (slot, button, mode) triples into typed variants
/// that the server can dispatch to dedicated handlers. Each variant
/// carries only the data relevant to that action type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClickAction {
    /// Left-click on a slot (pick up stack / place stack / swap with cursor).
    LeftClick {
        /// Protocol slot number.
        slot: i16,
    },
    /// Right-click on a slot (pick up half / place one / split cursor).
    RightClick {
        /// Protocol slot number.
        slot: i16,
    },
    /// Shift-click to quick-move an item between inventory regions.
    ShiftClick {
        /// Protocol slot number.
        slot: i16,
    },
    /// Double-click to collect matching items onto the cursor.
    DoubleClick {
        /// Protocol slot number.
        slot: i16,
    },
    /// Drop the item currently held on the cursor (click outside window).
    DropCursor {
        /// Whether to drop the entire stack (true) or a single item (false).
        drop_all: bool,
    },
    /// Drop from a specific slot (Q key while hovering).
    DropSlot {
        /// Protocol slot number.
        slot: i16,
        /// Whether to drop the entire stack (Ctrl+Q) or a single item (Q).
        drop_all: bool,
    },
    /// Swap a slot with a hotbar slot (number keys 1-9).
    HotbarSwap {
        /// Protocol slot number of the source slot.
        slot: i16,
        /// Hotbar index (0-8) to swap with.
        hotbar: u8,
    },
    /// Swap a slot with the offhand (F key).
    OffhandSwap {
        /// Protocol slot number.
        slot: i16,
    },
    /// Begin a drag operation (left/right/middle button down).
    StartDrag {
        /// Drag type: 0 = left (distribute), 1 = right (one each), 2 = middle (creative clone).
        drag_type: u8,
    },
    /// Add a slot to the current drag operation.
    AddDragSlot {
        /// Protocol slot number to include in the drag.
        slot: i16,
    },
    /// Finish a drag operation and distribute items.
    EndDrag {
        /// Drag type: 0 = left, 1 = right, 2 = middle.
        drag_type: u8,
    },
}

/// Parses a WindowClick packet's (slot, button, mode) into a typed [`ClickAction`].
///
/// Returns `None` for unrecognized combinations (e.g., creative
/// middle-click mode 3) that the server does not handle.
pub(super) fn parse_click_action(slot: i16, button: i8, mode: i32) -> Option<ClickAction> {
    match mode {
        // Mode 0: normal click (slot -999 = click outside window = drop cursor)
        0 => {
            if slot == -999 {
                return Some(ClickAction::DropCursor {
                    drop_all: button == 0,
                });
            }
            match button {
                0 => Some(ClickAction::LeftClick { slot }),
                1 => Some(ClickAction::RightClick { slot }),
                _ => None,
            }
        }
        // Mode 1: shift-click
        1 => Some(ClickAction::ShiftClick { slot }),
        // Mode 2: number key / offhand swap
        2 => {
            if button == 40 {
                Some(ClickAction::OffhandSwap { slot })
            } else if (0..=8).contains(&button) {
                Some(ClickAction::HotbarSwap {
                    slot,
                    hotbar: button as u8,
                })
            } else {
                None
            }
        }
        // Mode 3: creative middle-click (not handled)
        3 => None,
        // Mode 4: drop key
        4 => {
            if slot == -999 {
                Some(ClickAction::DropCursor {
                    drop_all: button == 1,
                })
            } else {
                Some(ClickAction::DropSlot {
                    slot,
                    drop_all: button == 1,
                })
            }
        }
        // Mode 5: drag
        5 => match button {
            0 => Some(ClickAction::StartDrag { drag_type: 0 }),
            4 => Some(ClickAction::StartDrag { drag_type: 1 }),
            8 => Some(ClickAction::StartDrag { drag_type: 2 }),
            1 | 5 | 9 => Some(ClickAction::AddDragSlot { slot }),
            2 => Some(ClickAction::EndDrag { drag_type: 0 }),
            6 => Some(ClickAction::EndDrag { drag_type: 1 }),
            10 => Some(ClickAction::EndDrag { drag_type: 2 }),
            _ => None,
        },
        // Mode 6: double-click (collect matching items)
        6 => Some(ClickAction::DoubleClick { slot }),
        _ => None,
    }
}

/// Tracks the state of a multi-packet drag operation per player.
///
/// Drag operations span three packets: [`ClickAction::StartDrag`],
/// one or more [`ClickAction::AddDragSlot`], and [`ClickAction::EndDrag`].
/// This enum captures the intermediate state between those packets.
#[derive(Debug, Clone)]
pub(crate) enum DragState {
    /// No drag in progress.
    None,
    /// A drag is currently active, collecting target slots.
    Active {
        /// Drag type: 0 = left (distribute evenly), 1 = right (one each), 2 = middle (creative).
        ///
        /// Stored for future validation that AddDragSlot/EndDrag packets
        /// match the original StartDrag type. Currently only read in tests.
        #[allow(dead_code)]
        drag_type: u8,
        /// Slots added so far via [`ClickAction::AddDragSlot`] packets.
        slots: Vec<i16>,
    },
}

/// Identifies a slot's logical location within a window.
///
/// Protocol slot numbers differ between window types (player inventory,
/// crafting table, chest). This enum normalizes them to a
/// storage-independent representation that the server can use
/// without caring which window type originated the click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowSlot {
    /// The crafting output slot (slot 0 in crafting windows).
    CraftOutput,
    /// A crafting grid input slot (0-based index).
    CraftGrid(usize),
    /// An armor slot: 0 = helmet, 1 = chestplate, 2 = leggings, 3 = boots.
    Armor(usize),
    /// Main inventory slot (internal index 9-35).
    MainInventory(usize),
    /// Hotbar slot (internal index 0-8).
    Hotbar(usize),
    /// The offhand slot.
    Offhand,
    /// A container-specific slot (chest, furnace, etc.).
    Container(usize),
}

/// Resolves a protocol slot in the player inventory window (window 0).
///
/// The player inventory layout is:
/// - 0: crafting output
/// - 1-4: 2x2 crafting grid
/// - 5-8: armor (helmet, chestplate, leggings, boots)
/// - 9-35: main inventory
/// - 36-44: hotbar
/// - 45: offhand
///
/// Returns `None` for out-of-range slots.
pub(super) fn resolve_player_inventory(slot: i16) -> Option<WindowSlot> {
    match slot {
        0 => Some(WindowSlot::CraftOutput),
        1..=4 => Some(WindowSlot::CraftGrid((slot - 1) as usize)),
        5..=8 => Some(WindowSlot::Armor((slot - 5) as usize)),
        9..=35 => Some(WindowSlot::MainInventory(slot as usize)),
        36..=44 => Some(WindowSlot::Hotbar((slot - 36) as usize)),
        45 => Some(WindowSlot::Offhand),
        _ => None,
    }
}

/// Resolves a protocol slot in a crafting table window.
///
/// The crafting table layout is:
/// - 0: crafting output
/// - 1-9: 3x3 crafting grid
/// - 10-36: main inventory (maps to internal 9-35)
/// - 37-45: hotbar (maps to internal 0-8)
///
/// Returns `None` for out-of-range slots.
pub(super) fn resolve_crafting_table(slot: i16) -> Option<WindowSlot> {
    match slot {
        0 => Some(WindowSlot::CraftOutput),
        1..=9 => Some(WindowSlot::CraftGrid((slot - 1) as usize)),
        10..=36 => Some(WindowSlot::MainInventory((slot - 1) as usize)),
        37..=45 => Some(WindowSlot::Hotbar((slot - 37) as usize)),
        _ => None,
    }
}

/// Resolves a protocol slot in a chest/container window.
///
/// The container layout is:
/// - 0..container_size: container slots
/// - container_size..container_size+27: main inventory (internal 9-35)
/// - container_size+27..container_size+36: hotbar (internal 0-8)
///
/// Returns `None` for out-of-range slots.
pub(super) fn resolve_chest(slot: i16, container_size: usize) -> Option<WindowSlot> {
    let s = slot as usize;
    if slot < 0 {
        return None;
    }
    if s < container_size {
        Some(WindowSlot::Container(s))
    } else if s < container_size + 27 {
        Some(WindowSlot::MainInventory(s - container_size + 9))
    } else if s < container_size + 36 {
        Some(WindowSlot::Hotbar(s - container_size - 27))
    } else {
        None
    }
}

/// Maps a [`WindowSlot`] back to a protocol slot number for the player inventory window.
///
/// Returns `None` for [`WindowSlot::Container`] since container slots
/// do not exist in the player inventory window.
pub(super) fn to_protocol_slot_player_inv(ws: &WindowSlot) -> Option<i16> {
    match ws {
        WindowSlot::CraftOutput => Some(0),
        WindowSlot::CraftGrid(i) => Some(*i as i16 + 1),
        WindowSlot::Armor(i) => Some(*i as i16 + 5),
        WindowSlot::MainInventory(i) => Some(*i as i16),
        WindowSlot::Hotbar(i) => Some(*i as i16 + 36),
        WindowSlot::Offhand => Some(45),
        WindowSlot::Container(_) => None,
    }
}

/// Maps a [`WindowSlot`] back to a protocol slot number for the crafting table window.
///
/// Returns `None` for slots that do not exist in the crafting table
/// window (armor, offhand, container).
pub(super) fn to_protocol_slot_crafting_table(ws: &WindowSlot) -> Option<i16> {
    match ws {
        WindowSlot::CraftOutput => Some(0),
        WindowSlot::CraftGrid(i) => Some(*i as i16 + 1),
        WindowSlot::MainInventory(i) => Some(*i as i16 + 1),
        WindowSlot::Hotbar(i) => Some(*i as i16 + 37),
        _ => None,
    }
}

/// Maps a [`WindowSlot`] back to a protocol slot number for a chest/container window.
///
/// Returns `None` for slots that do not exist in container windows
/// (craft output, craft grid, armor, offhand).
pub(super) fn to_protocol_slot_chest(ws: &WindowSlot, container_size: usize) -> Option<i16> {
    match ws {
        WindowSlot::Container(i) => Some(*i as i16),
        WindowSlot::MainInventory(i) => Some((*i - 9 + container_size) as i16),
        WindowSlot::Hotbar(i) => Some((*i + container_size + 27) as i16),
        _ => None,
    }
}

impl WindowSlot {
    /// Maps a logical slot location to the public [`WindowSlotKind`] enum.
    ///
    /// Strips the inner index and returns only the categorisation,
    /// so plugins can react to slot *kind* without depending on
    /// server-internal slot numbering.
    pub(super) fn to_kind(self) -> basalt_api::events::WindowSlotKind {
        use basalt_api::events::WindowSlotKind as K;
        match self {
            WindowSlot::CraftOutput => K::CraftOutput,
            WindowSlot::CraftGrid(_) => K::CraftGrid,
            WindowSlot::Armor(_) => K::Armor,
            WindowSlot::MainInventory(_) => K::MainInventory,
            WindowSlot::Hotbar(_) => K::Hotbar,
            WindowSlot::Offhand => K::Offhand,
            WindowSlot::Container(_) => K::Container,
        }
    }
}

/// Maps a server-internal [`ClickAction`] to the public [`ContainerClickType`].
///
/// Returns `None` for transient click phases (DropCursor, StartDrag,
/// AddDragSlot, EndDrag) which are internal details not exposed to plugins.
pub(super) fn click_type_from_action(
    action: &ClickAction,
) -> Option<basalt_api::events::ContainerClickType> {
    use basalt_api::events::ContainerClickType as T;
    match action {
        ClickAction::LeftClick { .. } => Some(T::LeftClick),
        ClickAction::RightClick { .. } => Some(T::RightClick),
        ClickAction::ShiftClick { .. } => Some(T::ShiftClick),
        ClickAction::DoubleClick { .. } => Some(T::DoubleClick),
        ClickAction::DropSlot { drop_all, .. } => Some(T::DropSlot {
            drop_all: *drop_all,
        }),
        ClickAction::HotbarSwap { hotbar, .. } => Some(T::HotbarSwap { hotbar: *hotbar }),
        ClickAction::OffhandSwap { .. } => Some(T::OffhandSwap),
        // Transient phases not exposed to plugins
        ClickAction::DropCursor { .. }
        | ClickAction::StartDrag { .. }
        | ClickAction::AddDragSlot { .. }
        | ClickAction::EndDrag { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // parse_click_action
    // ---------------------------------------------------------------

    #[test]
    fn parse_mode0_left_click() {
        assert_eq!(
            parse_click_action(10, 0, 0),
            Some(ClickAction::LeftClick { slot: 10 })
        );
    }

    #[test]
    fn parse_mode0_right_click() {
        assert_eq!(
            parse_click_action(5, 1, 0),
            Some(ClickAction::RightClick { slot: 5 })
        );
    }

    #[test]
    fn parse_mode0_invalid_button() {
        assert_eq!(parse_click_action(0, 2, 0), None);
    }

    #[test]
    fn parse_mode1_shift_click() {
        assert_eq!(
            parse_click_action(12, 0, 1),
            Some(ClickAction::ShiftClick { slot: 12 })
        );
        // Any button value works for shift-click
        assert_eq!(
            parse_click_action(12, 1, 1),
            Some(ClickAction::ShiftClick { slot: 12 })
        );
    }

    #[test]
    fn parse_mode2_hotbar_swap() {
        for hotbar in 0..=8 {
            assert_eq!(
                parse_click_action(20, hotbar, 2),
                Some(ClickAction::HotbarSwap {
                    slot: 20,
                    hotbar: hotbar as u8,
                })
            );
        }
    }

    #[test]
    fn parse_mode2_offhand_swap() {
        assert_eq!(
            parse_click_action(15, 40, 2),
            Some(ClickAction::OffhandSwap { slot: 15 })
        );
    }

    #[test]
    fn parse_mode2_invalid_button() {
        assert_eq!(parse_click_action(0, 9, 2), None);
        assert_eq!(parse_click_action(0, 39, 2), None);
        assert_eq!(parse_click_action(0, 41, 2), None);
    }

    #[test]
    fn parse_mode3_creative_middle_click_ignored() {
        assert_eq!(parse_click_action(10, 2, 3), None);
    }

    #[test]
    fn parse_mode4_drop_cursor_single() {
        assert_eq!(
            parse_click_action(-999, 0, 4),
            Some(ClickAction::DropCursor { drop_all: false })
        );
    }

    #[test]
    fn parse_mode4_drop_cursor_all() {
        assert_eq!(
            parse_click_action(-999, 1, 4),
            Some(ClickAction::DropCursor { drop_all: true })
        );
    }

    #[test]
    fn parse_mode4_drop_slot_single() {
        assert_eq!(
            parse_click_action(36, 0, 4),
            Some(ClickAction::DropSlot {
                slot: 36,
                drop_all: false,
            })
        );
    }

    #[test]
    fn parse_mode4_drop_slot_all() {
        assert_eq!(
            parse_click_action(36, 1, 4),
            Some(ClickAction::DropSlot {
                slot: 36,
                drop_all: true,
            })
        );
    }

    #[test]
    fn parse_mode5_start_drag_left() {
        assert_eq!(
            parse_click_action(-999, 0, 5),
            Some(ClickAction::StartDrag { drag_type: 0 })
        );
    }

    #[test]
    fn parse_mode5_start_drag_right() {
        assert_eq!(
            parse_click_action(-999, 4, 5),
            Some(ClickAction::StartDrag { drag_type: 1 })
        );
    }

    #[test]
    fn parse_mode5_start_drag_middle() {
        assert_eq!(
            parse_click_action(-999, 8, 5),
            Some(ClickAction::StartDrag { drag_type: 2 })
        );
    }

    #[test]
    fn parse_mode5_add_drag_slot_left() {
        assert_eq!(
            parse_click_action(10, 1, 5),
            Some(ClickAction::AddDragSlot { slot: 10 })
        );
    }

    #[test]
    fn parse_mode5_add_drag_slot_right() {
        assert_eq!(
            parse_click_action(20, 5, 5),
            Some(ClickAction::AddDragSlot { slot: 20 })
        );
    }

    #[test]
    fn parse_mode5_add_drag_slot_middle() {
        assert_eq!(
            parse_click_action(30, 9, 5),
            Some(ClickAction::AddDragSlot { slot: 30 })
        );
    }

    #[test]
    fn parse_mode5_end_drag_left() {
        assert_eq!(
            parse_click_action(-999, 2, 5),
            Some(ClickAction::EndDrag { drag_type: 0 })
        );
    }

    #[test]
    fn parse_mode5_end_drag_right() {
        assert_eq!(
            parse_click_action(-999, 6, 5),
            Some(ClickAction::EndDrag { drag_type: 1 })
        );
    }

    #[test]
    fn parse_mode5_end_drag_middle() {
        assert_eq!(
            parse_click_action(-999, 10, 5),
            Some(ClickAction::EndDrag { drag_type: 2 })
        );
    }

    #[test]
    fn parse_mode5_invalid_button() {
        assert_eq!(parse_click_action(0, 3, 5), None);
        assert_eq!(parse_click_action(0, 7, 5), None);
        assert_eq!(parse_click_action(0, 11, 5), None);
    }

    #[test]
    fn parse_mode6_double_click() {
        assert_eq!(
            parse_click_action(5, 0, 6),
            Some(ClickAction::DoubleClick { slot: 5 })
        );
    }

    #[test]
    fn parse_unknown_mode_returns_none() {
        assert_eq!(parse_click_action(0, 0, 7), None);
        assert_eq!(parse_click_action(0, 0, -1), None);
        assert_eq!(parse_click_action(0, 0, 100), None);
    }

    // ---------------------------------------------------------------
    // resolve_player_inventory
    // ---------------------------------------------------------------

    #[test]
    fn player_inv_craft_output() {
        assert_eq!(resolve_player_inventory(0), Some(WindowSlot::CraftOutput));
    }

    #[test]
    fn player_inv_craft_grid() {
        for i in 1..=4 {
            assert_eq!(
                resolve_player_inventory(i),
                Some(WindowSlot::CraftGrid((i - 1) as usize))
            );
        }
    }

    #[test]
    fn player_inv_armor() {
        for i in 5..=8 {
            assert_eq!(
                resolve_player_inventory(i),
                Some(WindowSlot::Armor((i - 5) as usize))
            );
        }
    }

    #[test]
    fn player_inv_main_inventory() {
        assert_eq!(
            resolve_player_inventory(9),
            Some(WindowSlot::MainInventory(9))
        );
        assert_eq!(
            resolve_player_inventory(35),
            Some(WindowSlot::MainInventory(35))
        );
    }

    #[test]
    fn player_inv_hotbar() {
        assert_eq!(resolve_player_inventory(36), Some(WindowSlot::Hotbar(0)));
        assert_eq!(resolve_player_inventory(44), Some(WindowSlot::Hotbar(8)));
    }

    #[test]
    fn player_inv_offhand() {
        assert_eq!(resolve_player_inventory(45), Some(WindowSlot::Offhand));
    }

    #[test]
    fn player_inv_out_of_range() {
        assert_eq!(resolve_player_inventory(-1), None);
        assert_eq!(resolve_player_inventory(46), None);
        assert_eq!(resolve_player_inventory(100), None);
    }

    // ---------------------------------------------------------------
    // resolve_crafting_table
    // ---------------------------------------------------------------

    #[test]
    fn crafting_table_output() {
        assert_eq!(resolve_crafting_table(0), Some(WindowSlot::CraftOutput));
    }

    #[test]
    fn crafting_table_grid() {
        for i in 1..=9 {
            assert_eq!(
                resolve_crafting_table(i),
                Some(WindowSlot::CraftGrid((i - 1) as usize))
            );
        }
    }

    #[test]
    fn crafting_table_main_inventory() {
        // Protocol 10 = internal 9
        assert_eq!(
            resolve_crafting_table(10),
            Some(WindowSlot::MainInventory(9))
        );
        // Protocol 36 = internal 35
        assert_eq!(
            resolve_crafting_table(36),
            Some(WindowSlot::MainInventory(35))
        );
    }

    #[test]
    fn crafting_table_hotbar() {
        assert_eq!(resolve_crafting_table(37), Some(WindowSlot::Hotbar(0)));
        assert_eq!(resolve_crafting_table(45), Some(WindowSlot::Hotbar(8)));
    }

    #[test]
    fn crafting_table_out_of_range() {
        assert_eq!(resolve_crafting_table(-1), None);
        assert_eq!(resolve_crafting_table(46), None);
    }

    // ---------------------------------------------------------------
    // resolve_chest
    // ---------------------------------------------------------------

    #[test]
    fn chest_single_container_slots() {
        // Single chest: 27 container slots
        assert_eq!(resolve_chest(0, 27), Some(WindowSlot::Container(0)));
        assert_eq!(resolve_chest(26, 27), Some(WindowSlot::Container(26)));
    }

    #[test]
    fn chest_single_main_inventory() {
        // Protocol 27 = internal 9
        assert_eq!(resolve_chest(27, 27), Some(WindowSlot::MainInventory(9)));
        // Protocol 53 = internal 35
        assert_eq!(resolve_chest(53, 27), Some(WindowSlot::MainInventory(35)));
    }

    #[test]
    fn chest_single_hotbar() {
        // Protocol 54 = hotbar 0
        assert_eq!(resolve_chest(54, 27), Some(WindowSlot::Hotbar(0)));
        // Protocol 62 = hotbar 8
        assert_eq!(resolve_chest(62, 27), Some(WindowSlot::Hotbar(8)));
    }

    #[test]
    fn chest_single_out_of_range() {
        assert_eq!(resolve_chest(-1, 27), None);
        assert_eq!(resolve_chest(63, 27), None);
    }

    #[test]
    fn chest_double_container_slots() {
        // Double chest: 54 container slots
        assert_eq!(resolve_chest(0, 54), Some(WindowSlot::Container(0)));
        assert_eq!(resolve_chest(53, 54), Some(WindowSlot::Container(53)));
    }

    #[test]
    fn chest_double_main_inventory() {
        // Protocol 54 = internal 9
        assert_eq!(resolve_chest(54, 54), Some(WindowSlot::MainInventory(9)));
        // Protocol 80 = internal 35
        assert_eq!(resolve_chest(80, 54), Some(WindowSlot::MainInventory(35)));
    }

    #[test]
    fn chest_double_hotbar() {
        // Protocol 81 = hotbar 0
        assert_eq!(resolve_chest(81, 54), Some(WindowSlot::Hotbar(0)));
        // Protocol 89 = hotbar 8
        assert_eq!(resolve_chest(89, 54), Some(WindowSlot::Hotbar(8)));
    }

    #[test]
    fn chest_double_out_of_range() {
        assert_eq!(resolve_chest(90, 54), None);
    }

    // ---------------------------------------------------------------
    // to_protocol_slot roundtrips — player inventory
    // ---------------------------------------------------------------

    #[test]
    fn roundtrip_player_inv_all_slots() {
        for slot in 0..=45i16 {
            let ws = resolve_player_inventory(slot).unwrap();
            let back = to_protocol_slot_player_inv(&ws).unwrap();
            assert_eq!(back, slot, "roundtrip failed for player inv slot {slot}");
        }
    }

    #[test]
    fn player_inv_container_returns_none() {
        assert_eq!(to_protocol_slot_player_inv(&WindowSlot::Container(0)), None);
    }

    // ---------------------------------------------------------------
    // to_protocol_slot roundtrips — crafting table
    // ---------------------------------------------------------------

    #[test]
    fn roundtrip_crafting_table_output_and_grid() {
        for slot in 0..=9i16 {
            let ws = resolve_crafting_table(slot).unwrap();
            let back = to_protocol_slot_crafting_table(&ws).unwrap();
            assert_eq!(
                back, slot,
                "roundtrip failed for crafting table slot {slot}"
            );
        }
    }

    #[test]
    fn roundtrip_crafting_table_hotbar() {
        for slot in 37..=45i16 {
            let ws = resolve_crafting_table(slot).unwrap();
            let back = to_protocol_slot_crafting_table(&ws).unwrap();
            assert_eq!(
                back, slot,
                "roundtrip failed for crafting table slot {slot}"
            );
        }
    }

    #[test]
    fn roundtrip_crafting_table_main_inventory() {
        for slot in 10..=36i16 {
            let ws = resolve_crafting_table(slot).unwrap();
            let back = to_protocol_slot_crafting_table(&ws).unwrap();
            assert_eq!(
                back, slot,
                "roundtrip failed for crafting table slot {slot}"
            );
        }
    }

    #[test]
    fn crafting_table_armor_returns_none() {
        assert_eq!(to_protocol_slot_crafting_table(&WindowSlot::Armor(0)), None);
    }

    #[test]
    fn crafting_table_offhand_returns_none() {
        assert_eq!(to_protocol_slot_crafting_table(&WindowSlot::Offhand), None);
    }

    #[test]
    fn crafting_table_container_returns_none() {
        assert_eq!(
            to_protocol_slot_crafting_table(&WindowSlot::Container(0)),
            None
        );
    }

    // ---------------------------------------------------------------
    // to_protocol_slot roundtrips — chest
    // ---------------------------------------------------------------

    #[test]
    fn roundtrip_chest_single_all_slots() {
        for slot in 0..63i16 {
            let ws = resolve_chest(slot, 27).unwrap();
            let back = to_protocol_slot_chest(&ws, 27).unwrap();
            assert_eq!(back, slot, "roundtrip failed for single chest slot {slot}");
        }
    }

    #[test]
    fn roundtrip_chest_double_all_slots() {
        for slot in 0..90i16 {
            let ws = resolve_chest(slot, 54).unwrap();
            let back = to_protocol_slot_chest(&ws, 54).unwrap();
            assert_eq!(back, slot, "roundtrip failed for double chest slot {slot}");
        }
    }

    #[test]
    fn chest_craft_output_returns_none() {
        assert_eq!(to_protocol_slot_chest(&WindowSlot::CraftOutput, 27), None);
    }

    #[test]
    fn chest_armor_returns_none() {
        assert_eq!(to_protocol_slot_chest(&WindowSlot::Armor(0), 27), None);
    }

    #[test]
    fn chest_offhand_returns_none() {
        assert_eq!(to_protocol_slot_chest(&WindowSlot::Offhand, 27), None);
    }

    // ---------------------------------------------------------------
    // DragState transitions
    // ---------------------------------------------------------------

    #[test]
    fn drag_state_none_is_default() {
        let state = DragState::None;
        assert!(matches!(state, DragState::None));
    }

    #[test]
    fn drag_state_active_collects_slots() {
        let mut state = DragState::Active {
            drag_type: 0,
            slots: Vec::new(),
        };
        if let DragState::Active { slots, .. } = &mut state {
            slots.push(10);
            slots.push(20);
            slots.push(30);
        }
        if let DragState::Active { drag_type, slots } = &state {
            assert_eq!(*drag_type, 0);
            assert_eq!(slots, &[10, 20, 30]);
        } else {
            panic!("expected Active state");
        }
    }

    #[test]
    fn drag_state_full_lifecycle() {
        // Start drag
        let action = parse_click_action(-999, 0, 5).unwrap();
        assert!(matches!(action, ClickAction::StartDrag { drag_type: 0 }));

        let mut state = DragState::Active {
            drag_type: 0,
            slots: Vec::new(),
        };

        // Add slots
        let add1 = parse_click_action(5, 1, 5).unwrap();
        assert!(matches!(add1, ClickAction::AddDragSlot { slot: 5 }));
        if let DragState::Active { slots, .. } = &mut state {
            slots.push(5);
        }

        let add2 = parse_click_action(10, 1, 5).unwrap();
        assert!(matches!(add2, ClickAction::AddDragSlot { slot: 10 }));
        if let DragState::Active { slots, .. } = &mut state {
            slots.push(10);
        }

        // End drag
        let end = parse_click_action(-999, 2, 5).unwrap();
        assert!(matches!(end, ClickAction::EndDrag { drag_type: 0 }));

        // Verify accumulated slots
        if let DragState::Active { slots, .. } = &state {
            assert_eq!(slots, &[5, 10]);
        }

        // Reset state
        let state = DragState::None;
        assert!(matches!(state, DragState::None));
    }

    #[test]
    fn drag_state_right_click_lifecycle() {
        // Right-click drag: button 4 start, button 5 add, button 6 end
        let start = parse_click_action(-999, 4, 5).unwrap();
        assert!(matches!(start, ClickAction::StartDrag { drag_type: 1 }));

        let add = parse_click_action(15, 5, 5).unwrap();
        assert!(matches!(add, ClickAction::AddDragSlot { slot: 15 }));

        let end = parse_click_action(-999, 6, 5).unwrap();
        assert!(matches!(end, ClickAction::EndDrag { drag_type: 1 }));
    }

    // ---------------------------------------------------------------
    // Edge cases
    // ---------------------------------------------------------------

    #[test]
    fn parse_negative_slot_mode0() {
        // slot -999 with mode 0 = click outside window = drop entire cursor
        assert_eq!(
            parse_click_action(-999, 0, 0),
            Some(ClickAction::DropCursor { drop_all: true })
        );
        // slot -999 with mode 0, button 1 = right-click outside = drop single
        assert_eq!(
            parse_click_action(-999, 1, 0),
            Some(ClickAction::DropCursor { drop_all: false })
        );
    }

    #[test]
    fn resolve_chest_zero_container_size() {
        // Degenerate case: 0-size container goes straight to player inv
        assert_eq!(resolve_chest(0, 0), Some(WindowSlot::MainInventory(9)));
        assert_eq!(resolve_chest(26, 0), Some(WindowSlot::MainInventory(35)));
        assert_eq!(resolve_chest(27, 0), Some(WindowSlot::Hotbar(0)));
    }

    #[test]
    fn player_inv_boundary_values() {
        // Exact boundaries
        assert!(resolve_player_inventory(0).is_some());
        assert!(resolve_player_inventory(45).is_some());
        assert!(resolve_player_inventory(-1).is_none());
        assert!(resolve_player_inventory(46).is_none());
    }

    #[test]
    fn crafting_table_boundary_values() {
        assert!(resolve_crafting_table(0).is_some());
        assert!(resolve_crafting_table(45).is_some());
        assert!(resolve_crafting_table(-1).is_none());
        assert!(resolve_crafting_table(46).is_none());
    }
}
