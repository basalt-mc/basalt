//! Hand-rolled `SlotDisplay`, `RecipeDisplay`, and `RecipeBookEntry`
//! used by the 1.21.4 recipe-book S2C packets.
//!
//! The codegen IR cannot represent recursive, switch-on-tag union types
//! (the `data: type ?` clauses in `proto.yml`), so the generated structs
//! fall back to opaque `Vec<u8>` whose default `Encode` impl prepends a
//! varint length — the wrong wire format. These hand-written types are
//! the authoritative encoding.
//!
//! Wire format (1.21.4):
//!
//! ```text
//! SlotDisplay  := varint tag (0..=7) + variant fields
//! RecipeDisplay := varint tag (0..=4) + variant fields
//! ```
//!
//! Only `Encode` and `EncodedSize` are implemented — these types are S2C
//! only, the server never decodes them.

use basalt_types::{Encode, EncodedSize, Result, Slot, VarInt};

/// One display slot in a recipe — empty, a literal item, an item tag, or
/// a recursive composition.
///
/// See the [type-level docs](self) for the wire format.
#[derive(Debug, Clone, PartialEq)]
pub enum SlotDisplay {
    /// No item — shown as a blank slot (tag 0, no payload).
    Empty,
    /// Any fuel item (used in furnace recipe displays — tag 1, no payload).
    AnyFuel,
    /// A specific item by id (tag 2 + varint).
    Item {
        /// Item registry id.
        item_id: i32,
    },
    /// A specific item stack with optional NBT (tag 3 + Slot).
    ItemStack {
        /// The full item stack.
        slot: Slot,
    },
    /// Items matching a registry tag (tag 4 + string identifier).
    Tag {
        /// Tag identifier, e.g. `"minecraft:logs"`.
        name: String,
    },
    /// Smithing trim composite display (tag 5 + 3 nested SlotDisplays).
    SmithingTrim {
        /// Base item slot (the equipment being trimmed).
        base: Box<SlotDisplay>,
        /// Trim material slot.
        material: Box<SlotDisplay>,
        /// Trim pattern slot.
        pattern: Box<SlotDisplay>,
    },
    /// "Take input, leave remainder" pair for recipes that don't fully
    /// consume the input slot (tag 6 + 2 nested SlotDisplays).
    WithRemainder {
        /// What the player must place.
        input: Box<SlotDisplay>,
        /// What is left in the slot after crafting.
        remainder: Box<SlotDisplay>,
    },
    /// Cycle through several display options (tag 7 + varint count + slots).
    ///
    /// The vanilla client cycles between entries every ~30 ticks, used
    /// for tag-based ingredients to show all matching items in turn.
    Composite {
        /// Sub-displays cycled by the client.
        entries: Vec<SlotDisplay>,
    },
}

impl SlotDisplay {
    /// Returns the wire tag (0..=7) for this variant.
    fn tag(&self) -> i32 {
        match self {
            Self::Empty => 0,
            Self::AnyFuel => 1,
            Self::Item { .. } => 2,
            Self::ItemStack { .. } => 3,
            Self::Tag { .. } => 4,
            Self::SmithingTrim { .. } => 5,
            Self::WithRemainder { .. } => 6,
            Self::Composite { .. } => 7,
        }
    }
}

impl Encode for SlotDisplay {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        VarInt(self.tag()).encode(buf)?;
        match self {
            Self::Empty | Self::AnyFuel => Ok(()),
            Self::Item { item_id } => VarInt(*item_id).encode(buf),
            Self::ItemStack { slot } => slot.encode(buf),
            Self::Tag { name } => name.encode(buf),
            Self::SmithingTrim {
                base,
                material,
                pattern,
            } => {
                base.encode(buf)?;
                material.encode(buf)?;
                pattern.encode(buf)
            }
            Self::WithRemainder { input, remainder } => {
                input.encode(buf)?;
                remainder.encode(buf)
            }
            Self::Composite { entries } => {
                VarInt(entries.len() as i32).encode(buf)?;
                for entry in entries {
                    entry.encode(buf)?;
                }
                Ok(())
            }
        }
    }
}

impl EncodedSize for SlotDisplay {
    fn encoded_size(&self) -> usize {
        let tag_size = VarInt(self.tag()).encoded_size();
        let body_size = match self {
            Self::Empty | Self::AnyFuel => 0,
            Self::Item { item_id } => VarInt(*item_id).encoded_size(),
            Self::ItemStack { slot } => slot.encoded_size(),
            Self::Tag { name } => name.encoded_size(),
            Self::SmithingTrim {
                base,
                material,
                pattern,
            } => base.encoded_size() + material.encoded_size() + pattern.encoded_size(),
            Self::WithRemainder { input, remainder } => {
                input.encoded_size() + remainder.encoded_size()
            }
            Self::Composite { entries } => {
                let count_size = VarInt(entries.len() as i32).encoded_size();
                count_size + entries.iter().map(|e| e.encoded_size()).sum::<usize>()
            }
        };
        tag_size + body_size
    }
}

/// A recipe display in the player's recipe book — describes the slots
/// the client should render for this recipe.
///
/// This is a presentation type: the actual matching logic lives in
/// `basalt-recipes::RecipeRegistry`, which produces the `Recipe` enum.
/// Convert from `Recipe` to `RecipeDisplay` via the
/// `basalt-server::game::recipe_book::to_display` helper.
#[derive(Debug, Clone, PartialEq)]
pub enum RecipeDisplay {
    /// Shapeless 2x2 / 3x3 crafting recipe (tag 0).
    CraftingShapeless {
        /// Ingredient slots in any order.
        ingredients: Vec<SlotDisplay>,
        /// Result slot.
        result: SlotDisplay,
        /// Station shown next to the recipe (typically `Item { CRAFTING_TABLE }`).
        crafting_station: SlotDisplay,
    },
    /// Shaped grid crafting recipe (tag 1).
    CraftingShaped {
        /// Pattern width (1..=3).
        width: i32,
        /// Pattern height (1..=3).
        height: i32,
        /// Ingredient grid in row-major order, length `width * height`.
        ingredients: Vec<SlotDisplay>,
        /// Result slot.
        result: SlotDisplay,
        /// Station shown next to the recipe.
        crafting_station: SlotDisplay,
    },
    /// Furnace / blast furnace / smoker / campfire recipe (tag 2).
    Furnace {
        /// Input ingredient.
        ingredient: SlotDisplay,
        /// Fuel slot (typically `AnyFuel`).
        fuel: SlotDisplay,
        /// Smelted result.
        result: SlotDisplay,
        /// Station shown next to the recipe.
        crafting_station: SlotDisplay,
        /// Smelt duration in ticks.
        duration: i32,
        /// Experience awarded on completion.
        experience: f32,
    },
    /// Stonecutter recipe (tag 3).
    Stonecutter {
        /// Input ingredient.
        ingredient: SlotDisplay,
        /// Result.
        result: SlotDisplay,
        /// Station shown next to the recipe.
        crafting_station: SlotDisplay,
    },
    /// Smithing-table recipe (tag 4).
    Smithing {
        /// Smithing template slot.
        template: SlotDisplay,
        /// Base equipment slot.
        base: SlotDisplay,
        /// Addition material slot.
        addition: SlotDisplay,
        /// Resulting equipment.
        result: SlotDisplay,
        /// Station shown next to the recipe.
        crafting_station: SlotDisplay,
    },
}

impl RecipeDisplay {
    /// Returns the wire tag (0..=4) for this variant.
    fn tag(&self) -> i32 {
        match self {
            Self::CraftingShapeless { .. } => 0,
            Self::CraftingShaped { .. } => 1,
            Self::Furnace { .. } => 2,
            Self::Stonecutter { .. } => 3,
            Self::Smithing { .. } => 4,
        }
    }
}

impl Encode for RecipeDisplay {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        VarInt(self.tag()).encode(buf)?;
        match self {
            Self::CraftingShapeless {
                ingredients,
                result,
                crafting_station,
            } => {
                encode_slot_vec(ingredients, buf)?;
                result.encode(buf)?;
                crafting_station.encode(buf)
            }
            Self::CraftingShaped {
                width,
                height,
                ingredients,
                result,
                crafting_station,
            } => {
                VarInt(*width).encode(buf)?;
                VarInt(*height).encode(buf)?;
                encode_slot_vec(ingredients, buf)?;
                result.encode(buf)?;
                crafting_station.encode(buf)
            }
            Self::Furnace {
                ingredient,
                fuel,
                result,
                crafting_station,
                duration,
                experience,
            } => {
                ingredient.encode(buf)?;
                fuel.encode(buf)?;
                result.encode(buf)?;
                crafting_station.encode(buf)?;
                VarInt(*duration).encode(buf)?;
                experience.encode(buf)
            }
            Self::Stonecutter {
                ingredient,
                result,
                crafting_station,
            } => {
                ingredient.encode(buf)?;
                result.encode(buf)?;
                crafting_station.encode(buf)
            }
            Self::Smithing {
                template,
                base,
                addition,
                result,
                crafting_station,
            } => {
                template.encode(buf)?;
                base.encode(buf)?;
                addition.encode(buf)?;
                result.encode(buf)?;
                crafting_station.encode(buf)
            }
        }
    }
}

/// A set of registry entries — referenced either by name (an inline
/// tag identifier) or by an inline list of registry ids.
///
/// Wire format (`registryEntryHolderSet` per minecraft-data 1.21.4):
///
/// ```text
/// IDSet := varint tag
///   if tag == 0: name: string
///   else:        ids: varint[tag - 1]
/// ```
///
/// Used by `crafting_requirements` on a recipe-book entry to gate
/// when the recipe should appear in the book (e.g. "only show if the
/// player carries an item from this tag").
#[derive(Debug, Clone, PartialEq)]
pub enum IDSet {
    /// Reference a registered tag by its identifier
    /// (e.g. `"minecraft:logs"`).
    Tag(String),
    /// Inline list of registry ids.
    Ids(Vec<i32>),
}

impl Encode for IDSet {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Tag(name) => {
                VarInt(0).encode(buf)?;
                name.encode(buf)
            }
            Self::Ids(ids) => {
                // Tag = ids.len() + 1; ids are written without an
                // additional length prefix (the length is recovered
                // from the tag by the reader).
                VarInt((ids.len() as i32) + 1).encode(buf)?;
                for id in ids {
                    VarInt(*id).encode(buf)?;
                }
                Ok(())
            }
        }
    }
}

impl EncodedSize for IDSet {
    fn encoded_size(&self) -> usize {
        match self {
            Self::Tag(name) => VarInt(0).encoded_size() + name.encoded_size(),
            Self::Ids(ids) => {
                VarInt((ids.len() as i32) + 1).encoded_size()
                    + ids
                        .iter()
                        .map(|id| VarInt(*id).encoded_size())
                        .sum::<usize>()
            }
        }
    }
}

impl EncodedSize for RecipeDisplay {
    fn encoded_size(&self) -> usize {
        let tag_size = VarInt(self.tag()).encoded_size();
        let body_size = match self {
            Self::CraftingShapeless {
                ingredients,
                result,
                crafting_station,
            } => {
                slot_vec_size(ingredients) + result.encoded_size() + crafting_station.encoded_size()
            }
            Self::CraftingShaped {
                width,
                height,
                ingredients,
                result,
                crafting_station,
            } => {
                VarInt(*width).encoded_size()
                    + VarInt(*height).encoded_size()
                    + slot_vec_size(ingredients)
                    + result.encoded_size()
                    + crafting_station.encoded_size()
            }
            Self::Furnace {
                ingredient,
                fuel,
                result,
                crafting_station,
                duration,
                experience,
            } => {
                ingredient.encoded_size()
                    + fuel.encoded_size()
                    + result.encoded_size()
                    + crafting_station.encoded_size()
                    + VarInt(*duration).encoded_size()
                    + experience.encoded_size()
            }
            Self::Stonecutter {
                ingredient,
                result,
                crafting_station,
            } => {
                ingredient.encoded_size() + result.encoded_size() + crafting_station.encoded_size()
            }
            Self::Smithing {
                template,
                base,
                addition,
                result,
                crafting_station,
            } => {
                template.encoded_size()
                    + base.encoded_size()
                    + addition.encoded_size()
                    + result.encoded_size()
                    + crafting_station.encoded_size()
            }
        };
        tag_size + body_size
    }
}

/// One entry in a [`ClientboundPlayRecipeBookAdd`](crate::packets::play::ClientboundPlayRecipeBookAdd) packet.
///
/// Wire layout: `display_id (varint) | display (RecipeDisplay) | group (varint) | category (varint) | crafting_requirements (Option<Vec<IDSet>>) | flags (u8)`.
///
/// Plugin-registered recipes typically carry no `crafting_requirements`
/// (they show up unconditionally in the book); the field is here so
/// future furnace / data-driven recipes can populate the predicates
/// the vanilla client expects.
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeBookEntry {
    /// Per-player numeric id assigned by the server. Stable for the
    /// lifetime of the connection so subsequent `Place Recipe` C2S
    /// packets can reference it.
    pub display_id: i32,
    /// The display payload.
    pub display: RecipeDisplay,
    /// Recipe-book group tag — recipes sharing the same group are
    /// shown together in the book. `0` for ungrouped.
    pub group: i32,
    /// Category index from `recipe_book_category` (0..=12 in 1.21.4):
    /// `crafting_building_blocks`, `crafting_redstone`,
    /// `crafting_equipment`, `crafting_misc`, `furnace_food`,
    /// `furnace_blocks`, `furnace_misc`, `blast_furnace_blocks`,
    /// `blast_furnace_misc`, `smoker_food`, `stonecutter`, `smithing`,
    /// `campfire`.
    pub category: i32,
    /// Optional list of [`IDSet`] predicates that gate when this recipe
    /// is shown. `None` (the common case for plugin recipes) means the
    /// recipe is always visible.
    pub crafting_requirements: Option<Vec<IDSet>>,
    /// Bitflags. `0x01` notify the player on add; `0x02` highlight in
    /// the book UI.
    pub flags: u8,
}

impl Encode for RecipeBookEntry {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        VarInt(self.display_id).encode(buf)?;
        self.display.encode(buf)?;
        VarInt(self.group).encode(buf)?;
        VarInt(self.category).encode(buf)?;
        match &self.crafting_requirements {
            Some(requirements) => {
                true.encode(buf)?;
                VarInt(requirements.len() as i32).encode(buf)?;
                for req in requirements {
                    req.encode(buf)?;
                }
            }
            None => {
                false.encode(buf)?;
            }
        }
        self.flags.encode(buf)
    }
}

impl EncodedSize for RecipeBookEntry {
    fn encoded_size(&self) -> usize {
        let req_size = match &self.crafting_requirements {
            Some(requirements) => {
                true.encoded_size()
                    + VarInt(requirements.len() as i32).encoded_size()
                    + requirements.iter().map(|r| r.encoded_size()).sum::<usize>()
            }
            None => false.encoded_size(),
        };
        VarInt(self.display_id).encoded_size()
            + self.display.encoded_size()
            + VarInt(self.group).encoded_size()
            + VarInt(self.category).encoded_size()
            + req_size
            + self.flags.encoded_size()
    }
}

/// Encodes a varint length prefix followed by each [`SlotDisplay`].
fn encode_slot_vec(items: &[SlotDisplay], buf: &mut Vec<u8>) -> Result<()> {
    VarInt(items.len() as i32).encode(buf)?;
    for item in items {
        item.encode(buf)?;
    }
    Ok(())
}

/// Returns the encoded byte length of a varint-prefixed slot vector.
fn slot_vec_size(items: &[SlotDisplay]) -> usize {
    VarInt(items.len() as i32).encoded_size()
        + items.iter().map(|s| s.encoded_size()).sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip: encode then verify length matches `encoded_size`.
    fn roundtrip<T: Encode + EncodedSize>(value: &T) -> Vec<u8> {
        let mut buf = Vec::new();
        value.encode(&mut buf).expect("encode");
        assert_eq!(
            buf.len(),
            value.encoded_size(),
            "encoded_size disagrees with encode output length"
        );
        buf
    }

    #[test]
    fn slot_display_empty_writes_only_tag() {
        let bytes = roundtrip(&SlotDisplay::Empty);
        assert_eq!(bytes, vec![0x00]);
    }

    #[test]
    fn slot_display_any_fuel() {
        let bytes = roundtrip(&SlotDisplay::AnyFuel);
        assert_eq!(bytes, vec![0x01]);
    }

    #[test]
    fn slot_display_item() {
        let bytes = roundtrip(&SlotDisplay::Item { item_id: 879 });
        // tag 2 + varint(879) — 879 = 0xef + 0x06 in varint
        assert_eq!(bytes, vec![0x02, 0xef, 0x06]);
    }

    #[test]
    fn slot_display_tag() {
        let bytes = roundtrip(&SlotDisplay::Tag {
            name: "minecraft:logs".into(),
        });
        // tag 4 + varint length 14 + "minecraft:logs"
        let mut expected = vec![0x04, 14];
        expected.extend_from_slice(b"minecraft:logs");
        assert_eq!(bytes, expected);
    }

    #[test]
    fn slot_display_with_remainder_recursive() {
        // Encode a WithRemainder whose remainder is also a WithRemainder.
        let inner = SlotDisplay::WithRemainder {
            input: Box::new(SlotDisplay::Item { item_id: 1 }),
            remainder: Box::new(SlotDisplay::Empty),
        };
        let outer = SlotDisplay::WithRemainder {
            input: Box::new(SlotDisplay::Item { item_id: 2 }),
            remainder: Box::new(inner),
        };
        let bytes = roundtrip(&outer);
        // tag 6 (outer) | tag 2 + varint 2 (input) | tag 6 (inner) | tag 2 + varint 1 | tag 0
        assert_eq!(bytes, vec![0x06, 0x02, 0x02, 0x06, 0x02, 0x01, 0x00]);
    }

    #[test]
    fn slot_display_composite_with_count_prefix() {
        let composite = SlotDisplay::Composite {
            entries: vec![
                SlotDisplay::Item { item_id: 1 },
                SlotDisplay::Item { item_id: 2 },
                SlotDisplay::Empty,
            ],
        };
        let bytes = roundtrip(&composite);
        // tag 7 | varint(3) | tag 2 + varint 1 | tag 2 + varint 2 | tag 0
        assert_eq!(bytes, vec![0x07, 3, 0x02, 0x01, 0x02, 0x02, 0x00]);
    }

    #[test]
    fn slot_display_smithing_trim_three_nested() {
        let st = SlotDisplay::SmithingTrim {
            base: Box::new(SlotDisplay::Item { item_id: 10 }),
            material: Box::new(SlotDisplay::Item { item_id: 20 }),
            pattern: Box::new(SlotDisplay::Item { item_id: 30 }),
        };
        let bytes = roundtrip(&st);
        // tag 5 | (tag 2 + 10) | (tag 2 + 20) | (tag 2 + 30)
        assert_eq!(bytes, vec![0x05, 0x02, 0x0a, 0x02, 0x14, 0x02, 0x1e]);
    }

    #[test]
    fn recipe_display_crafting_shaped_layout() {
        let rd = RecipeDisplay::CraftingShaped {
            width: 2,
            height: 2,
            ingredients: vec![
                SlotDisplay::Item { item_id: 1 },
                SlotDisplay::Item { item_id: 1 },
                SlotDisplay::Item { item_id: 1 },
                SlotDisplay::Item { item_id: 1 },
            ],
            result: SlotDisplay::Empty,
            crafting_station: SlotDisplay::Empty,
        };
        let bytes = roundtrip(&rd);
        // tag 1 | varint 2 | varint 2 | count(4) + 4×(tag 2 + 1) | empty result | empty station
        assert_eq!(
            bytes,
            vec![
                0x01, 0x02, 0x02, // tag, width, height
                0x04, // ingredients count
                0x02, 0x01, 0x02, 0x01, 0x02, 0x01, 0x02, 0x01, 0x00, // result: empty
                0x00, // crafting_station: empty
            ]
        );
    }

    #[test]
    fn recipe_display_crafting_shapeless_layout() {
        let rd = RecipeDisplay::CraftingShapeless {
            ingredients: vec![SlotDisplay::Item { item_id: 5 }],
            result: SlotDisplay::Empty,
            crafting_station: SlotDisplay::Empty,
        };
        let bytes = roundtrip(&rd);
        // tag 0 | count(1) + (tag 2 + 5) | empty | empty
        assert_eq!(bytes, vec![0x00, 0x01, 0x02, 0x05, 0x00, 0x00]);
    }

    #[test]
    fn recipe_display_furnace_with_duration_and_xp() {
        let rd = RecipeDisplay::Furnace {
            ingredient: SlotDisplay::Empty,
            fuel: SlotDisplay::AnyFuel,
            result: SlotDisplay::Empty,
            crafting_station: SlotDisplay::Empty,
            duration: 200,
            experience: 0.5,
        };
        let bytes = roundtrip(&rd);
        // tag 2 | empty | any_fuel | empty | empty | varint(200) | f32 0.5
        let mut expected = vec![0x02, 0x00, 0x01, 0x00, 0x00];
        // 200 = 0xc8 | 0x80 ⇒ 0xc8 0x01 in varint
        expected.extend_from_slice(&[0xc8, 0x01]);
        expected.extend_from_slice(&0.5f32.to_be_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn recipe_display_stonecutter_layout() {
        let rd = RecipeDisplay::Stonecutter {
            ingredient: SlotDisplay::Item { item_id: 1 },
            result: SlotDisplay::Item { item_id: 2 },
            crafting_station: SlotDisplay::Empty,
        };
        let bytes = roundtrip(&rd);
        assert_eq!(bytes, vec![0x03, 0x02, 0x01, 0x02, 0x02, 0x00]);
    }

    #[test]
    fn recipe_display_smithing_layout() {
        let rd = RecipeDisplay::Smithing {
            template: SlotDisplay::Item { item_id: 1 },
            base: SlotDisplay::Item { item_id: 2 },
            addition: SlotDisplay::Item { item_id: 3 },
            result: SlotDisplay::Item { item_id: 4 },
            crafting_station: SlotDisplay::Empty,
        };
        let bytes = roundtrip(&rd);
        assert_eq!(
            bytes,
            vec![0x04, 0x02, 0x01, 0x02, 0x02, 0x02, 0x03, 0x02, 0x04, 0x00]
        );
    }

    #[test]
    fn recipe_book_entry_no_requirements_writes_false_byte() {
        let entry = RecipeBookEntry {
            display_id: 7,
            display: RecipeDisplay::CraftingShapeless {
                ingredients: vec![SlotDisplay::Item { item_id: 1 }],
                result: SlotDisplay::Empty,
                crafting_station: SlotDisplay::Empty,
            },
            group: 0,
            category: 3,
            crafting_requirements: None,
            flags: 0x01,
        };
        let bytes = roundtrip(&entry);
        assert_eq!(
            bytes,
            vec![
                0x07, // display_id
                0x00, 0x01, 0x02, 0x01, 0x00, 0x00, // display
                0x00, // group
                0x03, // category
                0x00, // crafting_requirements: false
                0x01, // flags
            ]
        );
    }

    #[test]
    fn recipe_book_entry_with_requirements_writes_optional_vec() {
        let entry = RecipeBookEntry {
            display_id: 1,
            display: RecipeDisplay::CraftingShapeless {
                ingredients: vec![],
                result: SlotDisplay::Empty,
                crafting_station: SlotDisplay::Empty,
            },
            group: 0,
            category: 0,
            crafting_requirements: Some(vec![
                IDSet::Tag("minecraft:logs".into()),
                IDSet::Ids(vec![43, 44]),
            ]),
            flags: 0,
        };
        let bytes = roundtrip(&entry);
        // display_id, display (tag 0 + count 0 + empty + empty), group, cat,
        // true, len 2, IDSet[0] (tag 0 + "minecraft:logs"), IDSet[1] (tag 3 + 43, 44), flags
        let mut expected = vec![
            0x01, // display_id
            0x00, 0x00, 0x00, 0x00, // display: shapeless, no ingredients, empty, empty
            0x00, 0x00, // group, category
            0x01, // crafting_requirements present
            0x02, // 2 entries
            0x00, // IDSet::Tag tag = 0
            14,   // string length 14
        ];
        expected.extend_from_slice(b"minecraft:logs");
        expected.extend_from_slice(&[
            0x03, // IDSet::Ids tag = ids.len() + 1 = 3
            0x2b, 0x2c, // 43, 44 as varint
            0x00, // flags
        ]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn id_set_tag_round_trip() {
        let set = IDSet::Tag("minecraft:planks".into());
        let bytes = roundtrip(&set);
        // tag 0 | varint length 16 | "minecraft:planks"
        let mut expected = vec![0x00, 16];
        expected.extend_from_slice(b"minecraft:planks");
        assert_eq!(bytes, expected);
    }

    #[test]
    fn id_set_ids_writes_offset_tag() {
        let set = IDSet::Ids(vec![1, 2, 3]);
        let bytes = roundtrip(&set);
        // tag = 4 (3 ids + 1) | 1 | 2 | 3
        assert_eq!(bytes, vec![0x04, 0x01, 0x02, 0x03]);
    }

    #[test]
    fn id_set_empty_ids_uses_tag_one() {
        let set = IDSet::Ids(vec![]);
        let bytes = roundtrip(&set);
        // tag = 1 (0 ids + 1)
        assert_eq!(bytes, vec![0x01]);
    }
}
