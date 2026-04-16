//! Physics plugin — gravity, AABB collision, and movement resolution.
//!
//! Registers a `PhysicsSystem` in the Simulate phase that applies
//! gravity to entities with [`Velocity`], resolves movement against
//! solid blocks via AABB collision, and updates [`Position`].
//!
//! Requires a shared `Arc<World>` captured in the system closure
//! for block solidity checks.

use basalt_api::prelude::*;
use basalt_ecs::{BoundingBox, Phase, Position, Velocity};
use basalt_world::World;
use basalt_world::collision::{Aabb, resolve_movement};

/// Minecraft gravity constant: -0.08 blocks per tick² (downward).
const GRAVITY: f64 = 0.08;

/// Physics plugin: gravity, collision, and movement resolution.
///
/// Entities need [`Position`], [`Velocity`], and [`BoundingBox`]
/// components to be affected by physics. The world is obtained
/// via `registrar.world()` — same API as any other plugin.
pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "physics",
            version: "0.1.0",
            author: Some("Basalt"),
            dependencies: &[],
        }
    }

    fn on_enable(&self, registrar: &mut PluginRegistrar) {
        let world = registrar.world();

        registrar
            .system("physics")
            .phase(Phase::Simulate)
            .every(1)
            .reads::<BoundingBox>()
            .writes::<Position>()
            .writes::<Velocity>()
            .run(move |ecs| {
                physics_tick(ecs, &world);
            });
    }
}

/// Runs one physics tick: gravity → collision resolution → position update.
///
/// Iterates all entities with Position + Velocity. Entities without
/// BoundingBox are treated as points (no collision, just gravity + move).
fn physics_tick(ecs: &mut basalt_ecs::Ecs, world: &World) {
    // Collect entity IDs to avoid borrow conflicts during mutation
    let entities: Vec<basalt_ecs::EntityId> = ecs.iter::<Velocity>().map(|(id, _)| id).collect();

    for id in entities {
        let Some(vel) = ecs.get_mut::<Velocity>(id) else {
            continue;
        };

        // Apply gravity
        vel.dy -= GRAVITY;

        let dx = vel.dx;
        let dy = vel.dy;
        let dz = vel.dz;

        let Some(pos) = ecs.get::<Position>(id) else {
            continue;
        };
        let (px, py, pz) = (pos.x, pos.y, pos.z);

        // Resolve movement against solid blocks
        let (resolved_dx, resolved_dy, resolved_dz) = if let Some(bb) = ecs.get::<BoundingBox>(id) {
            let aabb = Aabb::from_entity(px, py, pz, bb.width, bb.height);
            resolve_movement(world, &aabb, dx, dy, dz)
        } else {
            // No bounding box — move freely (point entity)
            (dx, dy, dz)
        };

        // Update velocity to resolved values (important: if we hit
        // the ground, dy becomes 0 so we don't accumulate gravity)
        if let Some(vel) = ecs.get_mut::<Velocity>(id) {
            if (resolved_dy - dy).abs() > f64::EPSILON {
                vel.dy = 0.0; // Hit ground or ceiling
            }
            if (resolved_dx - dx).abs() > f64::EPSILON {
                vel.dx = 0.0;
            }
            if (resolved_dz - dz).abs() > f64::EPSILON {
                vel.dz = 0.0;
            }
        }

        // Apply resolved movement to position
        if let Some(pos) = ecs.get_mut::<Position>(id) {
            pos.x += resolved_dx;
            pos.y += resolved_dy;
            pos.z += resolved_dz;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_ecs::Ecs;

    fn test_world() -> std::sync::Arc<World> {
        std::sync::Arc::new(World::flat())
    }

    #[test]
    fn gravity_applies_to_velocity() {
        let world = test_world();
        let mut ecs = Ecs::new();
        let e = ecs.spawn();
        ecs.set(
            e,
            Position {
                x: 0.0,
                y: -40.0,
                z: 0.0,
            },
        );
        ecs.set(
            e,
            Velocity {
                dx: 0.0,
                dy: 0.0,
                dz: 0.0,
            },
        );
        // No bounding box = point entity, no collision

        physics_tick(&mut ecs, &world);

        let vel = ecs.get::<Velocity>(e).unwrap();
        // After one tick: dy should be -GRAVITY (0.08 downward)
        // But since no collision, velocity stays at -0.08
        assert!((vel.dy - (-GRAVITY)).abs() < f64::EPSILON);

        let pos = ecs.get::<Position>(e).unwrap();
        // Position moved by the velocity
        assert!((pos.y - (-40.0 - GRAVITY)).abs() < f64::EPSILON);
    }

    #[test]
    fn entity_lands_on_ground() {
        let world = test_world();
        let mut ecs = Ecs::new();
        let e = ecs.spawn();
        // Start just above ground (flat world grass at y=-61, spawn at y=-60)
        ecs.set(
            e,
            Position {
                x: 0.5,
                y: -59.9,
                z: 0.5,
            },
        );
        ecs.set(
            e,
            Velocity {
                dx: 0.0,
                dy: -1.0,
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

        physics_tick(&mut ecs, &world);

        let pos = ecs.get::<Position>(e).unwrap();
        // Should have landed on ground (y=-60), not fallen through
        assert!(
            pos.y >= -60.0,
            "entity should land on ground, got y={}",
            pos.y
        );

        let vel = ecs.get::<Velocity>(e).unwrap();
        // Vertical velocity should be zeroed after hitting ground
        assert_eq!(vel.dy, 0.0);
    }

    #[test]
    fn entity_falls_in_air() {
        let world = test_world();
        let mut ecs = Ecs::new();
        let e = ecs.spawn();
        ecs.set(
            e,
            Position {
                x: 0.5,
                y: -40.0,
                z: 0.5,
            },
        );
        ecs.set(
            e,
            Velocity {
                dx: 0.0,
                dy: 0.0,
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

        // Run several ticks
        for _ in 0..10 {
            physics_tick(&mut ecs, &world);
        }

        let pos = ecs.get::<Position>(e).unwrap();
        // Should have fallen significantly
        assert!(pos.y < -40.0, "entity should have fallen");
        assert!(pos.y > -60.0, "entity should not have reached ground yet");
    }

    #[test]
    fn entity_without_velocity_unaffected() {
        let world = test_world();
        let mut ecs = Ecs::new();
        let e = ecs.spawn();
        ecs.set(
            e,
            Position {
                x: 5.0,
                y: -40.0,
                z: 5.0,
            },
        );
        // No Velocity component

        physics_tick(&mut ecs, &world);

        let pos = ecs.get::<Position>(e).unwrap();
        assert_eq!(pos.y, -40.0); // Unchanged
    }

    #[test]
    fn horizontal_movement() {
        let world = test_world();
        let mut ecs = Ecs::new();
        let e = ecs.spawn();
        ecs.set(
            e,
            Position {
                x: 0.0,
                y: -40.0,
                z: 0.0,
            },
        );
        ecs.set(
            e,
            Velocity {
                dx: 1.0,
                dy: 0.0,
                dz: 0.5,
            },
        );

        physics_tick(&mut ecs, &world);

        let pos = ecs.get::<Position>(e).unwrap();
        // Horizontal movement applied (gravity also kicks in)
        assert!((pos.x - 1.0).abs() < f64::EPSILON);
        assert!((pos.z - 0.5).abs() < f64::EPSILON);
    }
}
