//! Container types shared across the Basalt crate ecosystem.
//!
//! These types live in `basalt-core` so that both `basalt-api` (plugin
//! API) and lower-level crates can reference them without circular
//! dependencies. Plugin crates access them via `basalt_api::container`.
//!
//! The main type is [`Container`], a reusable template value describing
//! how to open a container window.  Use [`ContainerBuilder`] (or
//! [`Container::builder()`]) for fluent construction.

use crate::components::BlockPosition;
use basalt_types::Slot;

/// Type of inventory window in Minecraft 1.21.4.
///
/// Each variant maps to a specific Minecraft protocol ID and has a
/// known slot count. Used when opening custom containers for players.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryType {
    /// 9x1 chest-like inventory (9 slots).
    Generic9x1,
    /// 9x2 chest-like inventory (18 slots).
    Generic9x2,
    /// 9x3 chest-like inventory (27 slots) -- single chest.
    Generic9x3,
    /// 9x4 chest-like inventory (36 slots).
    Generic9x4,
    /// 9x5 chest-like inventory (45 slots).
    Generic9x5,
    /// 9x6 chest-like inventory (54 slots) -- double chest.
    Generic9x6,
    /// 3x3 dispenser/dropper (9 slots).
    Generic3x3,
    /// 3x3 crafter (9 slots).
    Crafter3x3,
    /// Anvil (3 slots).
    Anvil,
    /// Beacon (1 slot).
    Beacon,
    /// Blast furnace (3 slots).
    BlastFurnace,
    /// Brewing stand (5 slots).
    BrewingStand,
    /// Crafting table (10 slots: 1 output + 3x3 grid).
    Crafting,
    /// Enchantment table (2 slots).
    Enchantment,
    /// Furnace (3 slots).
    Furnace,
    /// Grindstone (3 slots).
    Grindstone,
    /// Hopper (5 slots).
    Hopper,
    /// Lectern (1 slot).
    Lectern,
    /// Loom (4 slots).
    Loom,
    /// Merchant/villager trade (3 slots).
    Merchant,
    /// Shulker box (27 slots).
    ShulkerBox,
    /// Smithing table (4 slots).
    Smithing,
    /// Smoker (3 slots).
    Smoker,
    /// Cartography table (3 slots).
    Cartography,
    /// Stonecutter (2 slots).
    Stonecutter,
}

impl InventoryType {
    /// Returns the Minecraft protocol VarInt ID for this inventory type.
    ///
    /// Used when encoding the `OpenWindow` packet. IDs correspond to the
    /// 1.21.4 protocol (0 = generic\_9x1 through 24 = stonecutter).
    pub fn protocol_id(&self) -> i32 {
        match self {
            Self::Generic9x1 => 0,
            Self::Generic9x2 => 1,
            Self::Generic9x3 => 2,
            Self::Generic9x4 => 3,
            Self::Generic9x5 => 4,
            Self::Generic9x6 => 5,
            Self::Generic3x3 => 6,
            Self::Crafter3x3 => 7,
            Self::Anvil => 8,
            Self::Beacon => 9,
            Self::BlastFurnace => 10,
            Self::BrewingStand => 11,
            Self::Crafting => 12,
            Self::Enchantment => 13,
            Self::Furnace => 14,
            Self::Grindstone => 15,
            Self::Hopper => 16,
            Self::Lectern => 17,
            Self::Loom => 18,
            Self::Merchant => 19,
            Self::ShulkerBox => 20,
            Self::Smithing => 21,
            Self::Smoker => 22,
            Self::Cartography => 23,
            Self::Stonecutter => 24,
        }
    }

    /// Returns the number of container slots this type has.
    ///
    /// Does not include the player inventory slots that are appended
    /// when the window is opened.
    pub fn slot_count(&self) -> usize {
        match self {
            Self::Generic9x1 => 9,
            Self::Generic9x2 => 18,
            Self::Generic9x3 => 27,
            Self::Generic9x4 => 36,
            Self::Generic9x5 => 45,
            Self::Generic9x6 => 54,
            Self::Generic3x3 | Self::Crafter3x3 => 9,
            Self::Anvil => 3,
            Self::Beacon => 1,
            Self::BlastFurnace => 3,
            Self::BrewingStand => 5,
            Self::Crafting => 10,
            Self::Enchantment => 2,
            Self::Furnace => 3,
            Self::Grindstone => 3,
            Self::Hopper => 5,
            Self::Lectern => 1,
            Self::Loom => 4,
            Self::Merchant => 3,
            Self::ShulkerBox => 27,
            Self::Smithing => 4,
            Self::Smoker => 3,
            Self::Cartography => 3,
            Self::Stonecutter => 2,
        }
    }

    /// Returns true if this is a generic chest-like inventory (9xN) or a shulker box.
    ///
    /// Chest-like inventories have simple slot layouts where every slot
    /// behaves the same (no special output slot, no fuel/product slots).
    pub fn is_chest_like(&self) -> bool {
        matches!(
            self,
            Self::Generic9x1
                | Self::Generic9x2
                | Self::Generic9x3
                | Self::Generic9x4
                | Self::Generic9x5
                | Self::Generic9x6
                | Self::ShulkerBox
        )
    }

    /// Returns true if this inventory has a special crafting output slot at index 0.
    ///
    /// Currently only `Crafting` returns true. The output slot is
    /// server-computed from the crafting grid contents.
    pub fn has_craft_output(&self) -> bool {
        matches!(self, Self::Crafting)
    }
}

/// How a container window is backed in the world.
///
/// Virtual containers exist only on the server as transient GUI state;
/// their slots live on the player entity. Block-backed containers
/// correspond to a real block entity in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerBacking {
    /// No backing block -- a pure GUI menu.
    ///
    /// Slots live on the player entity via `VirtualContainerSlots`
    /// component and are cleaned up when the window closes.
    Virtual,
    /// Backed by a block entity at the given position.
    ///
    /// Reads and writes go through the block entity stored in the world.
    /// Other players viewing the same block see updates via viewer sync.
    Block {
        /// World position of the backing block.
        position: BlockPosition,
    },
}

/// Reusable template value describing how to open a container window.
///
/// A `Container` is a plain data value with no side effects.  It can
/// be stored, cloned, and passed to `ctx.containers().open(&container)`
/// to show the window to any player.  Build one via
/// [`Container::builder()`] or [`ContainerBuilder::new()`].
#[derive(Debug, Clone)]
pub struct Container {
    /// The Minecraft inventory type to open.
    pub inventory_type: InventoryType,
    /// Window title shown to the player.
    pub title: String,
    /// Whether the container is virtual (GUI) or backed by a block.
    pub backing: ContainerBacking,
    /// Optional initial slot contents.
    ///
    /// If `None` and `backing` is `Block`, the server reads slots from
    /// the block entity at the position. If `None` and `backing` is
    /// `Virtual`, slots start empty.
    ///
    /// If `Some`, these slots are used as-is (padded/truncated to match
    /// `inventory_type.slot_count()`).
    pub initial_slots: Option<Vec<Slot>>,
}

impl Container {
    /// Returns a fluent builder for configuring a container.
    pub fn builder() -> ContainerBuilder {
        ContainerBuilder::new()
    }
}

/// Fluent builder for [`Container`] configurations.
///
/// The builder has no side effects; call `.build()` to produce a
/// `Container` value that can be stored, cloned, and opened for any
/// player via `ctx.containers().open(&container)`.
pub struct ContainerBuilder {
    /// The inventory type to open.
    inventory_type: InventoryType,
    /// Window title displayed to the player.
    title: String,
    /// Whether the container is virtual or block-backed.
    backing: ContainerBacking,
    /// Optional pre-filled slot contents.
    initial_slots: Option<Vec<Slot>>,
}

impl ContainerBuilder {
    /// Creates a builder with sensible defaults (Generic9x3, empty title, Virtual, no slots).
    pub fn new() -> Self {
        Self {
            inventory_type: InventoryType::Generic9x3,
            title: String::new(),
            backing: ContainerBacking::Virtual,
            initial_slots: None,
        }
    }

    /// Sets the inventory type for the container window.
    pub fn inventory_type(mut self, t: InventoryType) -> Self {
        self.inventory_type = t;
        self
    }

    /// Sets the window title shown to the player.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Backs the container with a block entity at the given position.
    ///
    /// If no initial slots are provided via [`slots()`](Self::slots),
    /// the server reads the slots from the block entity at this position.
    pub fn backed_by(mut self, x: i32, y: i32, z: i32) -> Self {
        self.backing = ContainerBacking::Block {
            position: BlockPosition { x, y, z },
        };
        self
    }

    /// Pre-fills the container with the given slot contents.
    ///
    /// If the vector length does not match
    /// [`InventoryType::slot_count()`], it will be truncated or
    /// padded with empty slots on [`build()`](Self::build).
    pub fn slots(mut self, slots: Vec<Slot>) -> Self {
        self.initial_slots = Some(slots);
        self
    }

    /// Finalizes the builder into a [`Container`] value.
    ///
    /// Pads or truncates `initial_slots` to match `inventory_type.slot_count()`.
    pub fn build(self) -> Container {
        let expected = self.inventory_type.slot_count();
        let initial_slots = self.initial_slots.map(|mut s| {
            s.resize(expected, Slot::empty());
            s
        });

        Container {
            inventory_type: self.inventory_type,
            title: self.title,
            backing: self.backing,
            initial_slots,
        }
    }
}

impl Default for ContainerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All 25 inventory types with their expected protocol ID and slot count.
    const VARIANTS: [(InventoryType, i32, usize); 25] = [
        (InventoryType::Generic9x1, 0, 9),
        (InventoryType::Generic9x2, 1, 18),
        (InventoryType::Generic9x3, 2, 27),
        (InventoryType::Generic9x4, 3, 36),
        (InventoryType::Generic9x5, 4, 45),
        (InventoryType::Generic9x6, 5, 54),
        (InventoryType::Generic3x3, 6, 9),
        (InventoryType::Crafter3x3, 7, 9),
        (InventoryType::Anvil, 8, 3),
        (InventoryType::Beacon, 9, 1),
        (InventoryType::BlastFurnace, 10, 3),
        (InventoryType::BrewingStand, 11, 5),
        (InventoryType::Crafting, 12, 10),
        (InventoryType::Enchantment, 13, 2),
        (InventoryType::Furnace, 14, 3),
        (InventoryType::Grindstone, 15, 3),
        (InventoryType::Hopper, 16, 5),
        (InventoryType::Lectern, 17, 1),
        (InventoryType::Loom, 18, 4),
        (InventoryType::Merchant, 19, 3),
        (InventoryType::ShulkerBox, 20, 27),
        (InventoryType::Smithing, 21, 4),
        (InventoryType::Smoker, 22, 3),
        (InventoryType::Cartography, 23, 3),
        (InventoryType::Stonecutter, 24, 2),
    ];

    #[test]
    fn protocol_id_matches_all_variants() {
        for (variant, expected_id, _) in &VARIANTS {
            assert_eq!(
                variant.protocol_id(),
                *expected_id,
                "wrong protocol_id for {:?}",
                variant
            );
        }
    }

    #[test]
    fn slot_count_matches_all_variants() {
        for (variant, _, expected_count) in &VARIANTS {
            assert_eq!(
                variant.slot_count(),
                *expected_count,
                "wrong slot_count for {:?}",
                variant
            );
        }
    }

    #[test]
    fn is_chest_like_generic_and_shulker() {
        let chest_like = [
            InventoryType::Generic9x1,
            InventoryType::Generic9x2,
            InventoryType::Generic9x3,
            InventoryType::Generic9x4,
            InventoryType::Generic9x5,
            InventoryType::Generic9x6,
            InventoryType::ShulkerBox,
        ];
        for variant in &chest_like {
            assert!(
                variant.is_chest_like(),
                "{:?} should be chest-like",
                variant
            );
        }
    }

    #[test]
    fn is_chest_like_false_for_non_chest() {
        let non_chest = [
            InventoryType::Generic3x3,
            InventoryType::Crafter3x3,
            InventoryType::Anvil,
            InventoryType::Beacon,
            InventoryType::BlastFurnace,
            InventoryType::BrewingStand,
            InventoryType::Crafting,
            InventoryType::Enchantment,
            InventoryType::Furnace,
            InventoryType::Grindstone,
            InventoryType::Hopper,
            InventoryType::Lectern,
            InventoryType::Loom,
            InventoryType::Merchant,
            InventoryType::Smithing,
            InventoryType::Smoker,
            InventoryType::Cartography,
            InventoryType::Stonecutter,
        ];
        for variant in &non_chest {
            assert!(
                !variant.is_chest_like(),
                "{:?} should NOT be chest-like",
                variant
            );
        }
    }

    #[test]
    fn has_craft_output_only_crafting() {
        assert!(InventoryType::Crafting.has_craft_output());
        for (variant, _, _) in &VARIANTS {
            if *variant != InventoryType::Crafting {
                assert!(
                    !variant.has_craft_output(),
                    "{:?} should NOT have craft output",
                    variant
                );
            }
        }
    }

    #[test]
    fn container_backing_virtual() {
        let backing = ContainerBacking::Virtual;
        assert_eq!(backing, ContainerBacking::Virtual);
    }

    #[test]
    fn container_backing_block() {
        let pos = BlockPosition { x: 1, y: 2, z: 3 };
        let backing = ContainerBacking::Block { position: pos };
        assert_eq!(
            backing,
            ContainerBacking::Block {
                position: BlockPosition { x: 1, y: 2, z: 3 }
            }
        );
    }

    #[test]
    fn container_with_initial_slots() {
        let c = Container {
            inventory_type: InventoryType::Generic9x3,
            title: "My Chest".to_string(),
            backing: ContainerBacking::Virtual,
            initial_slots: Some(vec![Slot::default(); 27]),
        };
        assert_eq!(c.inventory_type.slot_count(), 27);
        assert_eq!(c.initial_slots.as_ref().unwrap().len(), 27);
    }

    #[test]
    fn container_no_initial_slots() {
        let c = Container {
            inventory_type: InventoryType::Crafting,
            title: "Crafting".to_string(),
            backing: ContainerBacking::Block {
                position: BlockPosition {
                    x: 10,
                    y: 64,
                    z: -5,
                },
            },
            initial_slots: None,
        };
        assert!(c.initial_slots.is_none());
        assert!(c.inventory_type.has_craft_output());
        assert_eq!(c.inventory_type.protocol_id(), 12);
    }

    #[test]
    fn builder_defaults() {
        let c = Container::builder().build();
        assert_eq!(c.inventory_type, InventoryType::Generic9x3);
        assert!(c.title.is_empty());
        assert!(matches!(c.backing, ContainerBacking::Virtual));
        assert!(c.initial_slots.is_none());
    }

    #[test]
    fn builder_full_chain() {
        let c = Container::builder()
            .inventory_type(InventoryType::Generic9x6)
            .title("Shop")
            .backed_by(5, 64, 3)
            .slots(vec![Slot::empty(); 54])
            .build();
        assert_eq!(c.inventory_type, InventoryType::Generic9x6);
        assert_eq!(c.title, "Shop");
        assert_eq!(
            c.backing,
            ContainerBacking::Block {
                position: BlockPosition { x: 5, y: 64, z: 3 }
            }
        );
        assert_eq!(c.initial_slots.unwrap().len(), 54);
    }

    #[test]
    fn builder_pads_slots() {
        let c = Container::builder()
            .inventory_type(InventoryType::Generic9x3)
            .slots(vec![Slot::new(1, 5)])
            .build();
        assert_eq!(c.initial_slots.as_ref().unwrap().len(), 27);
    }

    #[test]
    fn builder_truncates_slots() {
        let c = Container::builder()
            .inventory_type(InventoryType::Hopper)
            .slots(vec![Slot::new(1, 1); 10])
            .build();
        assert_eq!(c.initial_slots.as_ref().unwrap().len(), 5);
    }

    #[test]
    fn container_is_reusable() {
        let c = Container::builder()
            .inventory_type(InventoryType::Generic9x6)
            .build();
        let c2 = c.clone();
        assert_eq!(c.inventory_type, c2.inventory_type);
        assert_eq!(c.title, c2.title);
    }

    #[test]
    fn builder_default_trait() {
        let b = ContainerBuilder::default();
        let c = b.build();
        assert_eq!(c.inventory_type, InventoryType::Generic9x3);
    }

    #[test]
    fn builder_title_into_string() {
        let c = Container::builder().title(String::from("Dynamic")).build();
        assert_eq!(c.title, "Dynamic");
    }
}
