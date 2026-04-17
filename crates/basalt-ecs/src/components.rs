//! Core components provided by the ECS.
//!
//! These are the basic building blocks for entity state. Plugins
//! can register additional custom components via the registrar.

use crate::ecs::Component;

/// World position of an entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Z coordinate.
    pub z: f64,
}
impl Component for Position {}

/// Facing direction of an entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotation {
    /// Horizontal angle in degrees.
    pub yaw: f32,
    /// Vertical angle in degrees.
    pub pitch: f32,
}
impl Component for Rotation {}

/// Movement vector per tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Velocity {
    /// X movement per tick.
    pub dx: f64,
    /// Y movement per tick.
    pub dy: f64,
    /// Z movement per tick.
    pub dz: f64,
}
impl Component for Velocity {}

/// AABB hitbox dimensions.
///
/// The bounding box is axis-aligned and centered on the entity's
/// position. Used for collision detection and entity interactions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    /// Width (X and Z extent).
    pub width: f32,
    /// Height (Y extent).
    pub height: f32,
}
impl Component for BoundingBox {}

/// Minecraft entity type ID.
///
/// Maps to the registry entity type (e.g., 147 = player in 1.21.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityKind {
    /// Registry type ID.
    pub type_id: u32,
}
impl Component for EntityKind {}

/// Hit points for damageable entities.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Health {
    /// Current health.
    pub current: f32,
    /// Maximum health.
    pub max: f32,
}
impl Component for Health {}

/// Auto-despawn countdown.
///
/// Decremented each tick. When it reaches zero, the entity is
/// despawned. Used for dropped items (5 minutes = 6000 ticks),
/// arrows, experience orbs, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lifetime {
    /// Remaining ticks before despawn.
    pub remaining_ticks: u32,
}
impl Component for Lifetime {}

/// Links an entity to a player connection.
///
/// Present on player entities to map between the ECS entity and
/// the player's network state (UUID, username, output channel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRef {
    /// Player UUID (from Mojang or offline-mode).
    pub uuid: basalt_types::Uuid,
    /// Player display name.
    pub username: String,
}
impl Component for PlayerRef {}

/// Player hotbar inventory.
///
/// Tracks the 9 hotbar slots and which one is currently selected.
/// The held item determines block placement state.
#[derive(Debug, Clone)]
pub struct Inventory {
    /// Currently selected hotbar slot (0-8).
    pub held_slot: u8,
    /// Hotbar items (slots 0-8).
    pub hotbar: [basalt_types::Slot; 9],
}

impl Inventory {
    /// Creates an empty inventory with slot 0 selected.
    pub fn empty() -> Self {
        Self {
            held_slot: 0,
            hotbar: std::array::from_fn(|_| basalt_types::Slot::empty()),
        }
    }

    /// Returns the currently held item.
    pub fn held_item(&self) -> &basalt_types::Slot {
        &self.hotbar[self.held_slot as usize]
    }
}
impl Component for Inventory {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Ecs;

    #[test]
    fn all_components_work_with_ecs() {
        let mut ecs = Ecs::new();
        let e = ecs.spawn();

        ecs.set(
            e,
            Position {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
        );
        ecs.set(
            e,
            Rotation {
                yaw: 90.0,
                pitch: 0.0,
            },
        );
        ecs.set(
            e,
            Velocity {
                dx: 0.1,
                dy: -0.08,
                dz: 0.0,
            },
        );
        ecs.set(
            e,
            BoundingBox {
                width: 0.6,
                height: 1.8,
            },
        );
        ecs.set(e, EntityKind { type_id: 147 });
        ecs.set(
            e,
            Health {
                current: 20.0,
                max: 20.0,
            },
        );
        ecs.set(
            e,
            Lifetime {
                remaining_ticks: 6000,
            },
        );
        ecs.set(
            e,
            PlayerRef {
                uuid: basalt_types::Uuid::default(),
                username: "Steve".into(),
            },
        );

        assert!(ecs.has::<Position>(e));
        assert!(ecs.has::<Rotation>(e));
        assert!(ecs.has::<Velocity>(e));
        assert!(ecs.has::<BoundingBox>(e));
        assert!(ecs.has::<EntityKind>(e));
        assert!(ecs.has::<Health>(e));
        assert!(ecs.has::<Lifetime>(e));
        assert!(ecs.has::<PlayerRef>(e));
    }
}
