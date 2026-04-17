#![feature(test)]
extern crate test;

use test::{Bencher, black_box};

use basalt_ecs::{
    BoundingBox, Ecs, Health, Inventory, Phase, Position, Rotation, SystemBuilder, Velocity,
};

/// Spawns N entities with Position + Velocity + BoundingBox.
fn populated_ecs(n: u32) -> Ecs {
    let mut ecs = Ecs::new();
    for _ in 0..n {
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
    }
    ecs
}

// -- Spawn/despawn --

#[bench]
fn spawn_1000_entities(b: &mut Bencher) {
    b.iter(|| {
        let mut ecs = Ecs::new();
        for _ in 0..1000 {
            black_box(ecs.spawn());
        }
    });
}

#[bench]
fn despawn_1000_entities(b: &mut Bencher) {
    b.iter(|| {
        let mut ecs = populated_ecs(1000);
        let entities: Vec<_> = ecs.entities().to_vec();
        for e in entities {
            ecs.despawn(black_box(e));
        }
    });
}

// -- Get/set --

#[bench]
fn set_component_1000(b: &mut Bencher) {
    let mut ecs = Ecs::new();
    let entities: Vec<_> = (0..1000).map(|_| ecs.spawn()).collect();
    b.iter(|| {
        for &e in &entities {
            ecs.set(
                e,
                Position {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
            );
        }
    });
}

#[bench]
fn get_component_1000(b: &mut Bencher) {
    let ecs = populated_ecs(1000);
    let entities: Vec<_> = ecs.entities().to_vec();
    b.iter(|| {
        for &e in &entities {
            black_box(ecs.get::<Position>(e));
        }
    });
}

#[bench]
fn get_mut_component_1000(b: &mut Bencher) {
    let mut ecs = populated_ecs(1000);
    let entities: Vec<_> = ecs.entities().to_vec();
    b.iter(|| {
        for &e in &entities {
            if let Some(pos) = ecs.get_mut::<Position>(e) {
                pos.x += 0.1;
            }
        }
    });
}

// -- Iteration --

#[bench]
fn iter_position_100(b: &mut Bencher) {
    let ecs = populated_ecs(100);
    b.iter(|| {
        for (_, pos) in ecs.iter::<Position>() {
            black_box(pos);
        }
    });
}

#[bench]
fn iter_position_1000(b: &mut Bencher) {
    let ecs = populated_ecs(1000);
    b.iter(|| {
        for (_, pos) in ecs.iter::<Position>() {
            black_box(pos);
        }
    });
}

#[bench]
fn iter_position_10000(b: &mut Bencher) {
    let ecs = populated_ecs(10000);
    b.iter(|| {
        for (_, pos) in ecs.iter::<Position>() {
            black_box(pos);
        }
    });
}

// -- UUID lookup --

#[bench]
fn find_by_uuid_100_players(b: &mut Bencher) {
    let mut ecs = Ecs::new();
    let mut uuids = Vec::new();
    for i in 0u32..100 {
        let e = ecs.spawn();
        let uuid = basalt_types::Uuid::from_bytes(i.to_le_bytes().repeat(4).try_into().unwrap());
        ecs.index_uuid(uuid, e);
        uuids.push(uuid);
    }
    let target = uuids[50];
    b.iter(|| {
        black_box(ecs.find_by_uuid(target));
    });
}

// -- System dispatch --

#[bench]
fn run_all_empty_systems(b: &mut Bencher) {
    let mut ecs = populated_ecs(1000);
    b.iter(|| {
        ecs.run_all(black_box(0));
    });
}

#[bench]
fn run_all_gravity_system_1000(b: &mut Bencher) {
    let mut ecs = populated_ecs(1000);
    ecs.add_system(
        SystemBuilder::new("gravity")
            .phase(Phase::Simulate)
            .writes::<Velocity>()
            .run(|ecs| {
                let entities: Vec<_> = ecs.iter::<Velocity>().map(|(id, _)| id).collect();
                for id in entities {
                    if let Some(vel) = ecs.get_mut::<Velocity>(id) {
                        vel.dy -= 0.08;
                    }
                }
            }),
    );
    b.iter(|| {
        ecs.run_all(black_box(0));
    });
}

#[bench]
fn run_all_movement_system_1000(b: &mut Bencher) {
    let mut ecs = populated_ecs(1000);
    ecs.add_system(
        SystemBuilder::new("movement")
            .phase(Phase::Simulate)
            .reads::<Velocity>()
            .writes::<Position>()
            .run(|ecs| {
                let updates: Vec<_> = ecs
                    .iter::<Velocity>()
                    .map(|(id, vel)| (id, vel.dx, vel.dy, vel.dz))
                    .collect();
                for (id, dx, dy, dz) in updates {
                    if let Some(pos) = ecs.get_mut::<Position>(id) {
                        pos.x += dx;
                        pos.y += dy;
                        pos.z += dz;
                    }
                }
            }),
    );
    b.iter(|| {
        ecs.run_all(black_box(0));
    });
}
