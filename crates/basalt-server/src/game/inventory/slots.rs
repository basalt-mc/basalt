//! Slot access helpers for server-authoritative inventory clicks.
//!
//! Provides [`WindowType`] to identify the active window, plus helpers
//! to read, write, and sync slots by [`WindowSlot`] regardless of
//! which window type originated the click.

use basalt_types::Slot;

use super::super::GameLoop;
use super::super::click::{
    WindowSlot, resolve_chest, resolve_crafting_table, resolve_player_inventory,
    to_protocol_slot_chest, to_protocol_slot_crafting_table, to_protocol_slot_player_inv,
};
use crate::messages::ServerOutput;

/// Identifies the window type a player currently has open.
///
/// Determined from ECS state: if `OpenContainer` exists, reads the
/// stored `inventory_type` and `backing` fields directly. No
/// `OpenContainer` means the player inventory window.
#[derive(Debug, Clone)]
pub(super) enum WindowType {
    /// The default player inventory window (window ID 0).
    PlayerInventory,
    /// A crafting table window (3x3 grid).
    CraftingTable {
        /// Protocol window ID assigned when the table was opened.
        window_id: u8,
    },
    /// A chest or double chest container window.
    Chest {
        /// Protocol window ID assigned when the chest was opened.
        window_id: u8,
        /// Total number of container slots (27 or 54).
        container_size: usize,
        /// Block position of the container (primary part for doubles).
        position: (i32, i32, i32),
    },
}

impl GameLoop {
    /// Determines the window type currently open for a player.
    ///
    /// Reads directly from the `OpenContainer` component's `inventory_type`
    /// and `backing` fields. Returns `PlayerInventory` if no container
    /// is open.
    pub(super) fn determine_window_type(&self, eid: basalt_ecs::EntityId) -> WindowType {
        let Some(oc) = self.ecs.get::<basalt_api::components::OpenContainer>(eid) else {
            return WindowType::PlayerInventory;
        };

        let window_id = oc.window_id;

        match oc.inventory_type {
            basalt_api::container::InventoryType::Crafting => {
                WindowType::CraftingTable { window_id }
            }
            _ => {
                let container_size = oc.inventory_type.slot_count();
                let position = match oc.backing {
                    basalt_api::container::ContainerBacking::Block { position } => {
                        (position.x, position.y, position.z)
                    }
                    basalt_api::container::ContainerBacking::Virtual => (0, 0, 0),
                };
                WindowType::Chest {
                    window_id,
                    container_size,
                    position,
                }
            }
        }
    }

    /// Resolves a protocol slot number to a [`WindowSlot`] for the given window type.
    pub(super) fn resolve_slot(&self, wt: &WindowType, slot: i16) -> Option<WindowSlot> {
        match wt {
            WindowType::PlayerInventory => resolve_player_inventory(slot),
            WindowType::CraftingTable { .. } => resolve_crafting_table(slot),
            WindowType::Chest { container_size, .. } => resolve_chest(slot, *container_size),
        }
    }

    /// Reads the item in a logical slot for a player.
    ///
    /// Routes to the correct storage (inventory, crafting grid, or
    /// block entity) based on the [`WindowSlot`] variant.
    pub(super) fn read_slot(
        &self,
        eid: basalt_ecs::EntityId,
        ws: &WindowSlot,
        container_pos: Option<(i32, i32, i32)>,
    ) -> Slot {
        match ws {
            WindowSlot::CraftOutput => self
                .ecs
                .get::<basalt_api::components::CraftingGrid>(eid)
                .map(|g| g.output.clone())
                .unwrap_or_default(),
            WindowSlot::CraftGrid(i) => self
                .ecs
                .get::<basalt_api::components::CraftingGrid>(eid)
                .map(|g| g.slots[*i].clone())
                .unwrap_or_default(),
            WindowSlot::Armor(_) => Slot::empty(),
            WindowSlot::MainInventory(i) | WindowSlot::Hotbar(i) => self
                .ecs
                .get::<basalt_api::components::Inventory>(eid)
                .map(|inv| inv.slots[*i].clone())
                .unwrap_or_default(),
            WindowSlot::Offhand => Slot::empty(),
            WindowSlot::Container(i) => {
                let Some(pos) = container_pos else {
                    return Slot::empty();
                };
                let view = self.build_chest_view(pos.0, pos.1, pos.2);
                if let Some((part_pos, local_idx)) = view.slot_to_part(*i as i16) {
                    self.read_container_slot(part_pos, local_idx)
                } else {
                    Slot::empty()
                }
            }
        }
    }

    /// Writes an item to a logical slot for a player.
    ///
    /// Routes to the correct storage and handles side effects
    /// (marking chunks dirty, invalidating cache) for container slots.
    pub(super) fn write_slot(
        &mut self,
        eid: basalt_ecs::EntityId,
        ws: &WindowSlot,
        item: Slot,
        container_pos: Option<(i32, i32, i32)>,
    ) {
        match ws {
            WindowSlot::CraftOutput => {
                if let Some(grid) = self
                    .ecs
                    .get_mut::<basalt_api::components::CraftingGrid>(eid)
                {
                    grid.output = item;
                }
            }
            WindowSlot::CraftGrid(i) => {
                if let Some(grid) = self
                    .ecs
                    .get_mut::<basalt_api::components::CraftingGrid>(eid)
                {
                    grid.slots[*i] = item;
                }
            }
            WindowSlot::MainInventory(i) | WindowSlot::Hotbar(i) => {
                if let Some(inv) = self.ecs.get_mut::<basalt_api::components::Inventory>(eid) {
                    inv.slots[*i] = item;
                }
            }
            WindowSlot::Container(i) => {
                let Some(pos) = container_pos else { return };
                let view = self.build_chest_view(pos.0, pos.1, pos.2);
                if let Some((part_pos, local_idx)) = view.slot_to_part(*i as i16) {
                    self.write_container_slot(part_pos, local_idx, item);
                }
            }
            WindowSlot::Armor(_) | WindowSlot::Offhand => {
                // Not tracked yet — silently ignore
            }
        }
    }

    /// Sends a single slot update to the player's client.
    ///
    /// Converts the [`WindowSlot`] to a protocol slot number using
    /// the appropriate mapper for the window type, then sends a
    /// `SetContainerSlot` packet.
    pub(super) fn sync_slot(
        &self,
        eid: basalt_ecs::EntityId,
        wt: &WindowType,
        ws: &WindowSlot,
        item: Slot,
    ) {
        let (window_id, proto_slot) = match wt {
            WindowType::PlayerInventory => (0i32, to_protocol_slot_player_inv(ws)),
            WindowType::CraftingTable { window_id } => {
                (i32::from(*window_id), to_protocol_slot_crafting_table(ws))
            }
            WindowType::Chest {
                window_id,
                container_size,
                ..
            } => (
                i32::from(*window_id),
                to_protocol_slot_chest(ws, *container_size),
            ),
        };
        let Some(slot) = proto_slot else { return };
        self.send_to(eid, |tx| {
            use basalt_mc_protocol::packets::play::inventory::ClientboundPlaySetSlot;
            let _ = tx.try_send(ServerOutput::plain(
                ClientboundPlaySetSlot::PACKET_ID,
                ClientboundPlaySetSlot {
                    window_id,
                    state_id: 0,
                    slot,
                    item,
                },
            ));
        });
    }

    /// Sends the current cursor item to the client.
    ///
    /// Uses the protocol convention window_id=-1, slot=-1 for the
    /// carried item. Called after every click action to keep the
    /// client in sync.
    pub(super) fn sync_cursor(&self, eid: basalt_ecs::EntityId) {
        let cursor = self
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .map(|inv| inv.cursor.clone())
            .unwrap_or_default();
        self.send_to(eid, |tx| {
            use basalt_mc_protocol::packets::play::inventory::ClientboundPlaySetSlot;
            let _ = tx.try_send(ServerOutput::plain(
                ClientboundPlaySetSlot::PACKET_ID,
                ClientboundPlaySetSlot {
                    window_id: -1,
                    state_id: 0,
                    slot: -1,
                    item: cursor,
                },
            ));
        });
    }

    /// Reads all accessible slots in the current window as a flat vector.
    ///
    /// Used by double-click collect to scan every slot for matching items.
    /// Returns `(slots, window_slots)` where each slot's logical position
    /// is tracked for write-back.
    pub(super) fn read_all_slots(
        &self,
        eid: basalt_ecs::EntityId,
        wt: &WindowType,
        container_pos: Option<(i32, i32, i32)>,
    ) -> (Vec<Slot>, Vec<WindowSlot>) {
        let mut items = Vec::new();
        let mut positions = Vec::new();

        let slot_range: Vec<i16> = match wt {
            WindowType::PlayerInventory => (0..=45).collect(),
            WindowType::CraftingTable { .. } => (0..=45).collect(),
            WindowType::Chest { container_size, .. } => {
                (0..(*container_size as i16 + 36)).collect()
            }
        };

        for proto_slot in slot_range {
            if let Some(ws) = self.resolve_slot(wt, proto_slot) {
                // Skip output slot for double-click collect (can't take from it)
                if matches!(ws, WindowSlot::CraftOutput) {
                    continue;
                }
                items.push(self.read_slot(eid, &ws, container_pos));
                positions.push(ws);
            }
        }

        (items, positions)
    }

    /// Checks if a [`WindowSlot`] is in a crafting grid.
    pub(super) fn is_craft_slot(ws: &WindowSlot) -> bool {
        matches!(ws, WindowSlot::CraftGrid(_))
    }
}
