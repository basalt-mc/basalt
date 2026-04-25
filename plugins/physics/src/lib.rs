//! Physics plugin — gravity, AABB collision, and movement resolution.
//!
//! Registers a system in the Simulate phase that applies gravity to
//! entities with [`Velocity`], resolves movement against solid blocks
//! via AABB collision, and updates [`Position`].

use basalt_api::components::{BoundingBox, Position, Velocity};
use basalt_api::prelude::*;
use basalt_api::system::{Phase, SystemContext, SystemContextExt};
use basalt_api::world::collision::Aabb;

/// Minecraft gravity constant: -0.08 blocks per tick² (downward).
const GRAVITY: f64 = 0.08;

/// Physics plugin: gravity, collision, and movement resolution.
///
/// Entities need [`Position`], [`Velocity`], and [`BoundingBox`]
/// components to be affected by physics. Collision queries use
/// `ctx.resolve_movement()` inside the system runner.
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
        registrar
            .system("physics")
            .phase(Phase::Simulate)
            .every(1)
            .reads::<BoundingBox>()
            .writes::<Position>()
            .writes::<Velocity>()
            .run(physics_tick);
    }
}

/// Runs one physics tick: gravity → collision resolution → position update.
///
/// Iterates all entities with Position + Velocity. Entities without
/// BoundingBox are treated as points (no collision, just gravity + move).
fn physics_tick(ctx: &mut dyn SystemContext) {
    let entities = ctx.query::<Velocity>();

    for id in entities {
        let Some(vel) = ctx.get_mut::<Velocity>(id) else {
            continue;
        };

        // Apply gravity
        vel.dy -= GRAVITY;

        let dx = vel.dx;
        let dy = vel.dy;
        let dz = vel.dz;

        let Some(pos) = ctx.get::<Position>(id) else {
            continue;
        };
        let (px, py, pz) = (pos.x, pos.y, pos.z);

        // Resolve movement against solid blocks
        let (resolved_dx, resolved_dy, resolved_dz) = if let Some(bb) = ctx.get::<BoundingBox>(id) {
            let aabb = Aabb::from_entity(px, py, pz, bb.width, bb.height);
            ctx.resolve_movement(&aabb, dx, dy, dz)
        } else {
            (dx, dy, dz)
        };

        // Update velocity to resolved values (if we hit ground, dy → 0)
        if let Some(vel) = ctx.get_mut::<Velocity>(id) {
            if (resolved_dy - dy).abs() > f64::EPSILON {
                vel.dy = 0.0;
            }
            if (resolved_dx - dx).abs() > f64::EPSILON {
                vel.dx = 0.0;
            }
            if (resolved_dz - dz).abs() > f64::EPSILON {
                vel.dz = 0.0;
            }
        }

        // Apply resolved movement to position
        if let Some(pos) = ctx.get_mut::<Position>(id) {
            pos.x += resolved_dx;
            pos.y += resolved_dy;
            pos.z += resolved_dz;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_api::testing::SystemTestContext;

    #[test]
    fn gravity_applies_to_velocity() {
        let mut ctx = SystemTestContext::new();
        let e = ctx.spawn();
        ctx.set(
            e,
            Position {
                x: 0.0,
                y: -40.0,
                z: 0.0,
            },
        );
        ctx.set(
            e,
            Velocity {
                dx: 0.0,
                dy: 0.0,
                dz: 0.0,
            },
        );

        physics_tick(&mut ctx);

        let vel = ctx.get::<Velocity>(e).unwrap();
        assert!((vel.dy - (-GRAVITY)).abs() < f64::EPSILON);

        let pos = ctx.get::<Position>(e).unwrap();
        assert!((pos.y - (-40.0 - GRAVITY)).abs() < f64::EPSILON);
    }

    #[test]
    fn entity_lands_on_ground() {
        let mut ctx = SystemTestContext::new();
        let e = ctx.spawn();
        ctx.set(
            e,
            Position {
                x: 0.5,
                y: -59.9,
                z: 0.5,
            },
        );
        ctx.set(
            e,
            Velocity {
                dx: 0.0,
                dy: -1.0,
                dz: 0.0,
            },
        );
        ctx.set(
            e,
            BoundingBox {
                width: 0.6,
                height: 1.8,
            },
        );

        physics_tick(&mut ctx);

        let pos = ctx.get::<Position>(e).unwrap();
        assert!(
            pos.y >= -60.0,
            "entity should land on ground, got y={}",
            pos.y
        );

        let vel = ctx.get::<Velocity>(e).unwrap();
        assert_eq!(vel.dy, 0.0);
    }

    #[test]
    fn entity_falls_in_air() {
        let mut ctx = SystemTestContext::new();
        let e = ctx.spawn();
        ctx.set(
            e,
            Position {
                x: 0.5,
                y: -40.0,
                z: 0.5,
            },
        );
        ctx.set(
            e,
            Velocity {
                dx: 0.0,
                dy: 0.0,
                dz: 0.0,
            },
        );
        ctx.set(
            e,
            BoundingBox {
                width: 0.6,
                height: 1.8,
            },
        );

        for _ in 0..10 {
            physics_tick(&mut ctx);
        }

        let pos = ctx.get::<Position>(e).unwrap();
        assert!(pos.y < -40.0, "entity should have fallen");
        assert!(pos.y > -60.0, "entity should not have reached ground yet");
    }

    #[test]
    fn entity_without_velocity_unaffected() {
        let mut ctx = SystemTestContext::new();
        let e = ctx.spawn();
        ctx.set(
            e,
            Position {
                x: 5.0,
                y: -40.0,
                z: 5.0,
            },
        );

        physics_tick(&mut ctx);

        let pos = ctx.get::<Position>(e).unwrap();
        assert_eq!(pos.y, -40.0);
    }

    #[test]
    fn horizontal_movement() {
        let mut ctx = SystemTestContext::new();
        let e = ctx.spawn();
        ctx.set(
            e,
            Position {
                x: 0.0,
                y: -40.0,
                z: 0.0,
            },
        );
        ctx.set(
            e,
            Velocity {
                dx: 1.0,
                dy: 0.0,
                dz: 0.5,
            },
        );

        physics_tick(&mut ctx);

        let pos = ctx.get::<Position>(e).unwrap();
        assert!((pos.x - 1.0).abs() < f64::EPSILON);
        assert!((pos.z - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn entity_falls_and_lands_after_multiple_ticks() {
        let mut ctx = SystemTestContext::new();
        let e = ctx.spawn();
        ctx.set(
            e,
            Position {
                x: 0.5,
                y: -58.0,
                z: 0.5,
            },
        );
        ctx.set(
            e,
            Velocity {
                dx: 0.0,
                dy: 0.0,
                dz: 0.0,
            },
        );
        ctx.set(
            e,
            BoundingBox {
                width: 0.6,
                height: 1.8,
            },
        );

        for _ in 0..100 {
            physics_tick(&mut ctx);
        }

        let pos = ctx.get::<Position>(e).unwrap();
        assert!(
            (pos.y - (-60.0)).abs() < 0.01,
            "entity should have landed at y=-60, got y={}",
            pos.y
        );

        let vel = ctx.get::<Velocity>(e).unwrap();
        assert_eq!(vel.dy, 0.0, "velocity should be zero after landing");
    }
}
