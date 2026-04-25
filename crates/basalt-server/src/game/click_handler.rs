//! Pure click computation functions for inventory interactions.
//!
//! These functions compute the result of inventory click operations
//! (left-click, right-click, drag, double-click) without touching ECS,
//! networking, or any game state. They take the current state of slots
//! and the cursor, and return the new state.

use basalt_types::Slot;

/// Maximum number of items in a single stack.
const MAX_STACK: i32 = 64;

/// Result of a single-slot click (left or right click).
///
/// Contains the new state of the clicked slot and the cursor after
/// the click is applied.
#[derive(Debug, Clone)]
pub(super) struct ClickResult {
    /// New value of the clicked slot.
    pub clicked: Slot,
    /// New value of the cursor (item held by the mouse).
    pub cursor: Slot,
}

/// Result of a drag operation across multiple slots.
///
/// Contains the new state of each affected slot and the remaining
/// cursor after distribution.
#[derive(Debug, Clone)]
pub(super) struct DragResult {
    /// New values for each slot in the drag (same order as input).
    pub slots: Vec<Slot>,
    /// Remaining cursor after distribution.
    pub cursor: Slot,
}

/// Result of a double-click collect operation.
///
/// Contains per-slot updates (only changed slots) and the new cursor
/// after collecting matching items.
#[derive(Debug, Clone)]
pub(super) struct CollectResult {
    /// Per-slot update: `None` = unchanged, `Some(slot)` = new value.
    pub updates: Vec<Option<Slot>>,
    /// New cursor after collecting.
    pub cursor: Slot,
}

/// Returns true if two slots contain the same item type and can stack.
///
/// Both must have the same `item_id` (both `Some` with equal value) and
/// identical `component_data`. Empty slots never stack with anything.
fn can_stack(a: &Slot, b: &Slot) -> bool {
    match (a.item_id, b.item_id) {
        (Some(a_id), Some(b_id)) => a_id == b_id && a.component_data == b.component_data,
        _ => false,
    }
}

/// Creates a copy of a slot with a different count.
///
/// Preserves `item_id` and `component_data` from the source slot.
/// If `count` is zero or negative, returns an empty slot instead.
fn with_count(slot: &Slot, count: i32) -> Slot {
    if count <= 0 {
        return Slot::empty();
    }
    Slot {
        item_count: count,
        item_id: slot.item_id,
        component_data: slot.component_data.clone(),
    }
}

/// Computes the result of a left-click on a slot.
///
/// Behavior depends on the state of the clicked slot and cursor:
/// - Both empty: no-op
/// - Cursor empty, slot has item: pick up entire stack to cursor
/// - Slot empty, cursor has item: place entire cursor into slot
/// - Same item type: stack cursor onto slot (up to 64), remainder stays on cursor
/// - Different items: swap slot and cursor
///
pub(super) fn left_click(clicked: &Slot, cursor: &Slot) -> ClickResult {
    let clicked_empty = clicked.is_empty();
    let cursor_empty = cursor.is_empty();

    // Both empty: no-op
    if clicked_empty && cursor_empty {
        return ClickResult {
            clicked: Slot::empty(),
            cursor: Slot::empty(),
        };
    }

    // Cursor empty, pick up entire stack
    if cursor_empty {
        return ClickResult {
            clicked: Slot::empty(),
            cursor: clicked.clone(),
        };
    }

    // Slot empty, place entire cursor
    if clicked_empty {
        return ClickResult {
            clicked: cursor.clone(),
            cursor: Slot::empty(),
        };
    }

    // Same item type: stack
    if can_stack(clicked, cursor) {
        let total = clicked.item_count + cursor.item_count;
        if total <= MAX_STACK {
            // Everything fits in the slot
            ClickResult {
                clicked: with_count(clicked, total),
                cursor: Slot::empty(),
            }
        } else {
            // Slot fills to 64, remainder stays on cursor
            ClickResult {
                clicked: with_count(clicked, MAX_STACK),
                cursor: with_count(cursor, total - MAX_STACK),
            }
        }
    } else {
        // Different items: swap
        ClickResult {
            clicked: cursor.clone(),
            cursor: clicked.clone(),
        }
    }
}

/// Computes the result of a right-click on a slot.
///
/// Behavior depends on the state of the clicked slot and cursor:
/// - Both empty: no-op
/// - Cursor empty, slot has item: pick up half (ceil) to cursor, leave floor in slot
/// - Slot empty, cursor has item: place 1 from cursor into slot
/// - Same item type, slot not full: place 1 from cursor onto slot
/// - Same item type, slot full (64): no-op
/// - Different items: swap (same as left-click)
///
pub(super) fn right_click(clicked: &Slot, cursor: &Slot) -> ClickResult {
    let clicked_empty = clicked.is_empty();
    let cursor_empty = cursor.is_empty();

    // Both empty: no-op
    if clicked_empty && cursor_empty {
        return ClickResult {
            clicked: Slot::empty(),
            cursor: Slot::empty(),
        };
    }

    // Cursor empty: pick up half (rounded up) to cursor
    if cursor_empty {
        let half_up = (clicked.item_count + 1) / 2;
        let remain = clicked.item_count - half_up;
        return ClickResult {
            clicked: with_count(clicked, remain),
            cursor: with_count(clicked, half_up),
        };
    }

    // Slot empty: place 1 from cursor
    if clicked_empty {
        return ClickResult {
            clicked: with_count(cursor, 1),
            cursor: with_count(cursor, cursor.item_count - 1),
        };
    }

    // Same item type
    if can_stack(clicked, cursor) {
        if clicked.item_count >= MAX_STACK {
            // Slot already full, no change
            return ClickResult {
                clicked: clicked.clone(),
                cursor: cursor.clone(),
            };
        }
        // Place 1 from cursor onto slot
        return ClickResult {
            clicked: with_count(clicked, clicked.item_count + 1),
            cursor: with_count(cursor, cursor.item_count - 1),
        };
    }

    // Different items: swap
    ClickResult {
        clicked: cursor.clone(),
        cursor: clicked.clone(),
    }
}

/// Distributes cursor items across multiple slots (drag end).
///
/// When `left_drag` is true, divides cursor items evenly across compatible
/// slots. When false, places exactly 1 item per compatible slot.
///
/// A slot is "compatible" if it is empty or contains the same item type
/// (matching `item_id` and `component_data`) with space remaining (count < 64).
/// Incompatible slots are left unchanged.
///
pub(super) fn distribute_drag(cursor: &Slot, slots: &[Slot], left_drag: bool) -> DragResult {
    if cursor.is_empty() || slots.is_empty() {
        return DragResult {
            slots: slots.to_vec(),
            cursor: cursor.clone(),
        };
    }

    // Identify compatible slot indices and how much space each has
    let compatible: Vec<(usize, i32)> = slots
        .iter()
        .enumerate()
        .filter_map(|(i, slot)| {
            if slot.is_empty() {
                Some((i, MAX_STACK))
            } else if can_stack(slot, cursor) && slot.item_count < MAX_STACK {
                Some((i, MAX_STACK - slot.item_count))
            } else {
                None
            }
        })
        .collect();

    if compatible.is_empty() {
        return DragResult {
            slots: slots.to_vec(),
            cursor: cursor.clone(),
        };
    }

    let mut result: Vec<Slot> = slots.to_vec();
    let mut remaining = cursor.item_count;

    if left_drag {
        // Divide evenly: each compatible slot gets floor(total / count) items,
        // capped by available space
        let per_slot = cursor.item_count / compatible.len() as i32;
        if per_slot == 0 {
            // Not enough items to give at least 1 to each slot
            // Give 1 each until we run out
            for &(idx, space) in &compatible {
                if remaining <= 0 {
                    break;
                }
                let add = 1.min(space);
                let existing = result[idx].item_count;
                result[idx] = with_count(cursor, existing + add);
                remaining -= add;
            }
        } else {
            for &(idx, space) in &compatible {
                let add = per_slot.min(space);
                let existing = result[idx].item_count;
                result[idx] = with_count(cursor, existing + add);
                remaining -= add;
            }
        }
    } else {
        // Right drag: place exactly 1 per compatible slot
        for &(idx, space) in &compatible {
            if remaining <= 0 {
                break;
            }
            let add = 1.min(space);
            let existing = result[idx].item_count;
            result[idx] = with_count(cursor, existing + add);
            remaining -= add;
        }
    }

    DragResult {
        slots: result,
        cursor: with_count(cursor, remaining),
    }
}

/// Collects items matching the cursor into the cursor (double-click).
///
/// Scans all provided slots for items with the same `item_id` and
/// `component_data` as the cursor. Takes items from matching slots
/// until the cursor reaches 64 or all matching items are collected.
///
/// Returns `(updated slots, new cursor)`. Each entry in the slot vector
/// is `None` if unchanged, or `Some(new_slot)` if items were taken.
pub(super) fn collect_double_click(cursor: &Slot, all_slots: &[Slot]) -> CollectResult {
    if cursor.is_empty() {
        return CollectResult {
            updates: vec![None; all_slots.len()],
            cursor: cursor.clone(),
        };
    }

    let mut result: Vec<Option<Slot>> = vec![None; all_slots.len()];
    let mut cursor_count = cursor.item_count;

    for (i, slot) in all_slots.iter().enumerate() {
        if cursor_count >= MAX_STACK {
            break;
        }
        if slot.is_empty() || !can_stack(slot, cursor) {
            continue;
        }
        let take = slot.item_count.min(MAX_STACK - cursor_count);
        cursor_count += take;
        let new_count = slot.item_count - take;
        result[i] = Some(with_count(slot, new_count));
    }

    CollectResult {
        updates: result,
        cursor: with_count(cursor, cursor_count),
    }
}

#[cfg(test)]
mod tests {
    use basalt_types::Slot;

    use super::*;

    // ── left_click ──────────────────────────────────────────────

    #[test]
    fn left_click_both_empty() {
        let ClickResult { clicked, cursor } = left_click(&Slot::empty(), &Slot::empty());
        assert!(clicked.is_empty());
        assert!(cursor.is_empty());
    }

    #[test]
    fn left_click_pick_up() {
        let item = Slot::new(1, 10);
        let ClickResult { clicked, cursor } = left_click(&item, &Slot::empty());
        assert!(clicked.is_empty());
        assert_eq!(cursor.item_id, Some(1));
        assert_eq!(cursor.item_count, 10);
    }

    #[test]
    fn left_click_place() {
        let item = Slot::new(1, 10);
        let ClickResult { clicked, cursor } = left_click(&Slot::empty(), &item);
        assert_eq!(clicked.item_id, Some(1));
        assert_eq!(clicked.item_count, 10);
        assert!(cursor.is_empty());
    }

    #[test]
    fn left_click_stack_same_item() {
        let slot_item = Slot::new(1, 30);
        let cursor_item = Slot::new(1, 20);
        let ClickResult { clicked, cursor } = left_click(&slot_item, &cursor_item);
        assert_eq!(clicked.item_count, 50);
        assert_eq!(clicked.item_id, Some(1));
        assert!(cursor.is_empty());
    }

    #[test]
    fn left_click_stack_overflow() {
        let slot_item = Slot::new(1, 50);
        let cursor_item = Slot::new(1, 30);
        let ClickResult { clicked, cursor } = left_click(&slot_item, &cursor_item);
        assert_eq!(clicked.item_count, 64);
        assert_eq!(clicked.item_id, Some(1));
        assert_eq!(cursor.item_count, 16);
        assert_eq!(cursor.item_id, Some(1));
    }

    #[test]
    fn left_click_swap_different_items() {
        let slot_item = Slot::new(1, 10);
        let cursor_item = Slot::new(2, 5);
        let ClickResult { clicked, cursor } = left_click(&slot_item, &cursor_item);
        assert_eq!(clicked.item_id, Some(2));
        assert_eq!(clicked.item_count, 5);
        assert_eq!(cursor.item_id, Some(1));
        assert_eq!(cursor.item_count, 10);
    }

    #[test]
    fn left_click_stack_preserves_component_data() {
        let data = vec![0xAA, 0xBB];
        let slot_item = Slot {
            item_count: 10,
            item_id: Some(1),
            component_data: data.clone(),
        };
        let cursor_item = Slot {
            item_count: 5,
            item_id: Some(1),
            component_data: data.clone(),
        };
        let ClickResult { clicked, cursor } = left_click(&slot_item, &cursor_item);
        assert_eq!(clicked.item_count, 15);
        assert_eq!(clicked.component_data, data);
        assert!(cursor.is_empty());
    }

    // ── right_click ─────────────────────────────────────────────

    #[test]
    fn right_click_both_empty() {
        let ClickResult { clicked, cursor } = right_click(&Slot::empty(), &Slot::empty());
        assert!(clicked.is_empty());
        assert!(cursor.is_empty());
    }

    #[test]
    fn right_click_pick_up_half_even() {
        let item = Slot::new(1, 10);
        let ClickResult { clicked, cursor } = right_click(&item, &Slot::empty());
        assert_eq!(clicked.item_count, 5);
        assert_eq!(cursor.item_count, 5);
    }

    #[test]
    fn right_click_pick_up_half_odd() {
        let item = Slot::new(1, 7);
        let ClickResult { clicked, cursor } = right_click(&item, &Slot::empty());
        // ceil(7/2) = 4 to cursor, floor(7/2) = 3 stays
        assert_eq!(clicked.item_count, 3);
        assert_eq!(cursor.item_count, 4);
    }

    #[test]
    fn right_click_pick_up_half_one() {
        let item = Slot::new(1, 1);
        let ClickResult { clicked, cursor } = right_click(&item, &Slot::empty());
        assert!(clicked.is_empty());
        assert_eq!(cursor.item_count, 1);
    }

    #[test]
    fn right_click_place_one_into_empty() {
        let cursor_item = Slot::new(1, 10);
        let ClickResult { clicked, cursor } = right_click(&Slot::empty(), &cursor_item);
        assert_eq!(clicked.item_count, 1);
        assert_eq!(clicked.item_id, Some(1));
        assert_eq!(cursor.item_count, 9);
    }

    #[test]
    fn right_click_place_one_onto_stack() {
        let slot_item = Slot::new(1, 5);
        let cursor_item = Slot::new(1, 10);
        let ClickResult { clicked, cursor } = right_click(&slot_item, &cursor_item);
        assert_eq!(clicked.item_count, 6);
        assert_eq!(cursor.item_count, 9);
    }

    #[test]
    fn right_click_place_one_last() {
        let slot_item = Slot::new(1, 5);
        let cursor_item = Slot::new(1, 1);
        let ClickResult { clicked, cursor } = right_click(&slot_item, &cursor_item);
        assert_eq!(clicked.item_count, 6);
        assert!(cursor.is_empty());
    }

    #[test]
    fn right_click_swap_different() {
        let slot_item = Slot::new(1, 10);
        let cursor_item = Slot::new(2, 5);
        let ClickResult { clicked, cursor } = right_click(&slot_item, &cursor_item);
        assert_eq!(clicked.item_id, Some(2));
        assert_eq!(clicked.item_count, 5);
        assert_eq!(cursor.item_id, Some(1));
        assert_eq!(cursor.item_count, 10);
    }

    #[test]
    fn right_click_slot_full_no_place() {
        let slot_item = Slot::new(1, 64);
        let cursor_item = Slot::new(1, 5);
        let ClickResult { clicked, cursor } = right_click(&slot_item, &cursor_item);
        assert_eq!(clicked.item_count, 64);
        assert_eq!(cursor.item_count, 5);
    }

    // ── distribute_drag ─────────────────────────────────────────

    #[test]
    fn left_drag_even() {
        let cursor = Slot::new(1, 8);
        let slots = vec![Slot::empty(), Slot::empty(), Slot::empty(), Slot::empty()];
        let DragResult {
            slots: result,
            cursor: rem,
        } = distribute_drag(&cursor, &slots, true);
        for s in &result {
            assert_eq!(s.item_count, 2);
            assert_eq!(s.item_id, Some(1));
        }
        assert!(rem.is_empty());
    }

    #[test]
    fn left_drag_uneven() {
        let cursor = Slot::new(1, 7);
        let slots = vec![Slot::empty(), Slot::empty(), Slot::empty()];
        let DragResult {
            slots: result,
            cursor: rem,
        } = distribute_drag(&cursor, &slots, true);
        for s in &result {
            assert_eq!(s.item_count, 2);
        }
        assert_eq!(rem.item_count, 1);
    }

    #[test]
    fn left_drag_skip_incompatible() {
        let cursor = Slot::new(1, 6);
        let slots = vec![Slot::empty(), Slot::new(2, 5), Slot::empty()];
        let DragResult {
            slots: result,
            cursor: rem,
        } = distribute_drag(&cursor, &slots, true);
        // Only slots 0 and 2 are compatible
        assert_eq!(result[0].item_count, 3);
        assert_eq!(result[0].item_id, Some(1));
        assert_eq!(result[1].item_count, 5);
        assert_eq!(result[1].item_id, Some(2));
        assert_eq!(result[2].item_count, 3);
        assert_eq!(result[2].item_id, Some(1));
        assert!(rem.is_empty());
    }

    #[test]
    fn left_drag_respect_max() {
        let cursor = Slot::new(1, 20);
        let slots = vec![Slot::new(1, 60), Slot::empty()];
        let DragResult {
            slots: result,
            cursor: rem,
        } = distribute_drag(&cursor, &slots, true);
        // Slot 0 has space for 4, slot 1 has space for 64
        // per_slot = 20 / 2 = 10, but slot 0 capped at 4
        assert_eq!(result[0].item_count, 64);
        assert_eq!(result[1].item_count, 10);
        // Used 4 + 10 = 14, remaining = 6
        assert_eq!(rem.item_count, 6);
    }

    #[test]
    fn right_drag() {
        let cursor = Slot::new(1, 10);
        let slots = vec![Slot::empty(), Slot::empty(), Slot::empty()];
        let DragResult {
            slots: result,
            cursor: rem,
        } = distribute_drag(&cursor, &slots, false);
        for s in &result {
            assert_eq!(s.item_count, 1);
        }
        assert_eq!(rem.item_count, 7);
    }

    #[test]
    fn right_drag_not_enough() {
        let cursor = Slot::new(1, 2);
        let slots = vec![
            Slot::empty(),
            Slot::empty(),
            Slot::empty(),
            Slot::empty(),
            Slot::empty(),
        ];
        let DragResult {
            slots: result,
            cursor: rem,
        } = distribute_drag(&cursor, &slots, false);
        assert_eq!(result[0].item_count, 1);
        assert_eq!(result[1].item_count, 1);
        assert!(result[2].is_empty());
        assert!(result[3].is_empty());
        assert!(result[4].is_empty());
        assert!(rem.is_empty());
    }

    #[test]
    fn distribute_drag_empty_cursor() {
        let slots = vec![Slot::new(1, 10), Slot::empty()];
        let DragResult {
            slots: result,
            cursor: rem,
        } = distribute_drag(&Slot::empty(), &slots, true);
        assert_eq!(result[0].item_count, 10);
        assert!(result[1].is_empty());
        assert!(rem.is_empty());
    }

    // ── collect_double_click ────────────────────────────────────

    #[test]
    fn collect_from_multiple() {
        let cursor = Slot::new(1, 5);
        let slots = vec![Slot::new(1, 10), Slot::new(1, 8), Slot::new(2, 3)];
        let CollectResult {
            updates,
            cursor: new_cursor,
        } = collect_double_click(&cursor, &slots);
        assert_eq!(new_cursor.item_count, 23);
        // Slot 0 fully taken
        assert!(updates[0].as_ref().unwrap().is_empty());
        // Slot 1 fully taken
        assert!(updates[1].as_ref().unwrap().is_empty());
        // Slot 2 different item, unchanged
        assert!(updates[2].is_none());
    }

    #[test]
    fn collect_stops_at_64() {
        let cursor = Slot::new(1, 50);
        let slots = vec![Slot::new(1, 20), Slot::new(1, 10)];
        let CollectResult {
            updates,
            cursor: new_cursor,
        } = collect_double_click(&cursor, &slots);
        assert_eq!(new_cursor.item_count, 64);
        // Slot 0: took 14 of 20, leaving 6
        assert_eq!(updates[0].as_ref().unwrap().item_count, 6);
        // Slot 1: not touched because cursor already at 64
        assert!(updates[1].is_none());
    }

    #[test]
    fn collect_skips_different() {
        let cursor = Slot::new(1, 5);
        let slots = vec![Slot::new(2, 10), Slot::new(3, 8)];
        let CollectResult {
            updates,
            cursor: new_cursor,
        } = collect_double_click(&cursor, &slots);
        assert_eq!(new_cursor.item_count, 5);
        assert!(updates[0].is_none());
        assert!(updates[1].is_none());
    }

    #[test]
    fn collect_empties_source() {
        let cursor = Slot::new(1, 1);
        let slots = vec![Slot::new(1, 3), Slot::new(1, 2)];
        let CollectResult {
            updates,
            cursor: new_cursor,
        } = collect_double_click(&cursor, &slots);
        assert_eq!(new_cursor.item_count, 6);
        assert!(updates[0].as_ref().unwrap().is_empty());
        assert!(updates[1].as_ref().unwrap().is_empty());
    }

    // ── helpers ─────────────────────────────────────────────────

    #[test]
    fn can_stack_same_item() {
        assert!(can_stack(&Slot::new(1, 5), &Slot::new(1, 10)));
    }

    #[test]
    fn can_stack_different_id() {
        assert!(!can_stack(&Slot::new(1, 5), &Slot::new(2, 10)));
    }

    #[test]
    fn can_stack_different_component_data() {
        let a = Slot {
            item_count: 5,
            item_id: Some(1),
            component_data: vec![0xAA],
        };
        let b = Slot {
            item_count: 5,
            item_id: Some(1),
            component_data: vec![0xBB],
        };
        assert!(!can_stack(&a, &b));
    }

    #[test]
    fn can_stack_empty_slots() {
        assert!(!can_stack(&Slot::empty(), &Slot::empty()));
    }

    #[test]
    fn with_count_zero_returns_empty() {
        let slot = Slot::new(1, 10);
        let result = with_count(&slot, 0);
        assert!(result.is_empty());
        assert!(result.item_id.is_none());
    }

    #[test]
    fn with_count_preserves_data() {
        let slot = Slot {
            item_count: 10,
            item_id: Some(42),
            component_data: vec![0xAA, 0xBB],
        };
        let result = with_count(&slot, 5);
        assert_eq!(result.item_count, 5);
        assert_eq!(result.item_id, Some(42));
        assert_eq!(result.component_data, vec![0xAA, 0xBB]);
    }
}
