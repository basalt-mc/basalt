//! Round-trip tests for the codegen-emitted `RecipeDisplay` and
//! `SlotDisplay` enums.
//!
//! These mirror the byte fixtures from the previous hand-rolled
//! `crates/basalt-protocol/src/types/recipe_display.rs` so the wire
//! format stays identical after the codegen migration. Any divergence
//! here means the codegen produces different bytes and would break
//! 1.21.4 client compatibility.

use basalt_protocol::packets::play::types::{RecipeDisplay, SlotDisplay};
use basalt_types::{Decode, Encode, EncodedSize, Slot};

/// Encode → check `encoded_size` → decode → compare.
fn roundtrip<T>(value: &T) -> Vec<u8>
where
    T: Encode + Decode + EncodedSize + std::fmt::Debug + PartialEq,
{
    let mut buf = Vec::new();
    value.encode(&mut buf).expect("encode");
    assert_eq!(
        buf.len(),
        value.encoded_size(),
        "encoded_size disagrees with encode output length"
    );
    let mut cursor: &[u8] = &buf;
    let decoded = T::decode(&mut cursor).expect("decode");
    assert!(
        cursor.is_empty(),
        "decode left {} unread bytes",
        cursor.len()
    );
    assert_eq!(&decoded, value, "encode→decode round trip mismatch");
    buf
}

#[test]
fn slot_display_empty_writes_only_tag() {
    assert_eq!(roundtrip(&SlotDisplay::Empty), vec![0x00]);
}

#[test]
fn slot_display_any_fuel() {
    assert_eq!(roundtrip(&SlotDisplay::AnyFuel), vec![0x01]);
}

#[test]
fn slot_display_item_writes_tag_plus_varint() {
    let bytes = roundtrip(&SlotDisplay::Item { data: 879 });
    // tag 2 + varint(879) — 879 = 0xef + 0x06 in varint.
    assert_eq!(bytes, vec![0x02, 0xef, 0x06]);
}

#[test]
fn slot_display_tag_writes_string() {
    let bytes = roundtrip(&SlotDisplay::Tag {
        data: "minecraft:logs".into(),
    });
    let mut expected = vec![0x04, 14];
    expected.extend_from_slice(b"minecraft:logs");
    assert_eq!(bytes, expected);
}

#[test]
fn slot_display_smithing_trim_three_nested() {
    let st = SlotDisplay::SmithingTrim {
        base: Box::new(SlotDisplay::Item { data: 10 }),
        material: Box::new(SlotDisplay::Item { data: 20 }),
        pattern: Box::new(SlotDisplay::Item { data: 30 }),
    };
    let bytes = roundtrip(&st);
    // tag 5 | (tag 2 + 10) | (tag 2 + 20) | (tag 2 + 30)
    assert_eq!(bytes, vec![0x05, 0x02, 0x0a, 0x02, 0x14, 0x02, 0x1e]);
}

#[test]
fn slot_display_with_remainder_recursive() {
    let inner = SlotDisplay::WithRemainder {
        input: Box::new(SlotDisplay::Item { data: 1 }),
        remainder: Box::new(SlotDisplay::Empty),
    };
    let outer = SlotDisplay::WithRemainder {
        input: Box::new(SlotDisplay::Item { data: 2 }),
        remainder: Box::new(inner),
    };
    let bytes = roundtrip(&outer);
    // tag 6 | (tag 2 + 2) | tag 6 | (tag 2 + 1) | tag 0
    assert_eq!(bytes, vec![0x06, 0x02, 0x02, 0x06, 0x02, 0x01, 0x00]);
}

#[test]
fn slot_display_composite_count_prefixed_vec() {
    let composite = SlotDisplay::Composite {
        data: vec![
            SlotDisplay::Item { data: 1 },
            SlotDisplay::Item { data: 2 },
            SlotDisplay::Empty,
        ],
    };
    let bytes = roundtrip(&composite);
    // tag 7 | varint(3) | tag 2 + 1 | tag 2 + 2 | tag 0
    assert_eq!(bytes, vec![0x07, 3, 0x02, 0x01, 0x02, 0x02, 0x00]);
}

#[test]
fn recipe_display_crafting_shaped_layout() {
    let rd = RecipeDisplay::CraftingShaped {
        width: 2,
        height: 2,
        ingredients: vec![
            SlotDisplay::Item { data: 1 },
            SlotDisplay::Item { data: 1 },
            SlotDisplay::Item { data: 1 },
            SlotDisplay::Item { data: 1 },
        ],
        result: SlotDisplay::Empty,
        crafting_station: SlotDisplay::Empty,
    };
    let bytes = roundtrip(&rd);
    assert_eq!(
        bytes,
        vec![
            0x01, 0x02, 0x02, // tag, width, height
            0x04, // ingredients count
            0x02, 0x01, 0x02, 0x01, 0x02, 0x01, 0x02, 0x01, // 4 × Item(1)
            0x00, // result: empty
            0x00, // crafting_station: empty
        ]
    );
}

#[test]
fn recipe_display_crafting_shapeless_layout() {
    let rd = RecipeDisplay::CraftingShapeless {
        ingredients: vec![SlotDisplay::Item { data: 5 }],
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
    expected.extend_from_slice(&[0xc8, 0x01]); // 200 as varint
    expected.extend_from_slice(&0.5f32.to_be_bytes());
    assert_eq!(bytes, expected);
}

#[test]
fn recipe_display_stonecutter_layout() {
    let rd = RecipeDisplay::Stonecutter {
        ingredient: SlotDisplay::Item { data: 1 },
        result: SlotDisplay::Item { data: 2 },
        crafting_station: SlotDisplay::Empty,
    };
    assert_eq!(roundtrip(&rd), vec![0x03, 0x02, 0x01, 0x02, 0x02, 0x00]);
}

#[test]
fn recipe_display_smithing_layout() {
    let rd = RecipeDisplay::Smithing {
        template: SlotDisplay::Item { data: 1 },
        base: SlotDisplay::Item { data: 2 },
        addition: SlotDisplay::Item { data: 3 },
        result: SlotDisplay::Item { data: 4 },
        crafting_station: SlotDisplay::Empty,
    };
    assert_eq!(
        roundtrip(&rd),
        vec![0x04, 0x02, 0x01, 0x02, 0x02, 0x02, 0x03, 0x02, 0x04, 0x00]
    );
}

#[test]
fn slot_display_item_stack_carries_full_slot() {
    let bytes = roundtrip(&SlotDisplay::ItemStack {
        data: Slot::new(280, 4),
    });
    // tag 3 + Slot encoding (varint count + varint id + components count).
    // We just check the leading tag and that decode round-trips
    // structurally — the Slot encoding itself is tested elsewhere.
    assert_eq!(bytes[0], 0x03);
}

/// `crafting_requirements: Option<Vec<u8>>` round-trips for both the
/// `None` (single `false` byte) and `Some(payload)` (`true` + varint
/// length + payload) wire layouts.
///
/// Replaces the byte fixture from the deleted hand-rolled
/// `RecipeBookEntry::crafting_requirements` test. The codegen leaves
/// the field as `Option<Vec<u8>>` until the derive macros gain
/// combined `optional + length` attribute support, so the fixture
/// validates the bool-prefix encoding rather than typed `IDSet`
/// content.
mod crafting_requirements {
    use basalt_protocol::packets::play::misc::ClientboundPlayRecipeBookAddEntriesRecipe;
    use basalt_protocol::packets::play::types::{RecipeDisplay, SlotDisplay};
    use basalt_types::{Decode, Encode, EncodedSize};

    fn entry(req: Option<Vec<u8>>) -> ClientboundPlayRecipeBookAddEntriesRecipe {
        ClientboundPlayRecipeBookAddEntriesRecipe {
            display_id: 7,
            display: RecipeDisplay::CraftingShapeless {
                ingredients: vec![SlotDisplay::Item { data: 1 }],
                result: SlotDisplay::Empty,
                crafting_station: SlotDisplay::Empty,
            },
            group: 0,
            category: 3,
            crafting_requirements: req,
        }
    }

    #[test]
    fn none_writes_single_false_byte() {
        let value = entry(None);
        let mut buf = Vec::new();
        value.encode(&mut buf).expect("encode");
        // display_id (varint 7) | display | group (varint 0) | category (varint 3) | crafting_requirements (false)
        assert_eq!(
            buf,
            vec![
                0x07, // display_id
                0x00, 0x01, 0x02, 0x01, 0x00,
                0x00, // display: shapeless | 1 ingredient (Item 1) | empty | empty
                0x00, // group
                0x03, // category
                0x00, // crafting_requirements: false
            ]
        );
        assert_eq!(buf.len(), value.encoded_size());

        let mut cursor: &[u8] = &buf;
        let decoded =
            ClientboundPlayRecipeBookAddEntriesRecipe::decode(&mut cursor).expect("decode");
        assert_eq!(decoded, value);
    }

    #[test]
    fn some_writes_true_then_varint_length_then_bytes() {
        let payload = vec![0xaa, 0xbb, 0xcc];
        let value = entry(Some(payload.clone()));
        let mut buf = Vec::new();
        value.encode(&mut buf).expect("encode");
        // ... | crafting_requirements (true | varint 3 | 0xaa 0xbb 0xcc)
        let mut expected = vec![
            0x07, 0x00, 0x01, 0x02, 0x01, 0x00, 0x00, 0x00, 0x03, // header bytes
            0x01, // crafting_requirements: true
            0x03, // varint length 3
        ];
        expected.extend_from_slice(&payload);
        assert_eq!(buf, expected);
        assert_eq!(buf.len(), value.encoded_size());

        let mut cursor: &[u8] = &buf;
        let decoded =
            ClientboundPlayRecipeBookAddEntriesRecipe::decode(&mut cursor).expect("decode");
        assert_eq!(decoded, value);
    }
}
