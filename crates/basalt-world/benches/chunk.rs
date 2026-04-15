#![feature(test)]
extern crate test;

use test::{Bencher, black_box};

use basalt_world::palette::PalettedContainer;
use basalt_world::{ChunkColumn, FlatWorldGenerator, NoiseTerrainGenerator};

#[bench]
fn palette_encode_single_value(b: &mut Bencher) {
    let container = PalettedContainer::filled(0);
    let mut buf = Vec::with_capacity(64);
    b.iter(|| {
        buf.clear();
        container.encode_to(black_box(&mut buf));
    });
}

#[bench]
fn palette_encode_two_states(b: &mut Bencher) {
    let mut container = PalettedContainer::filled(0);
    for x in 0..16 {
        for z in 0..16 {
            container.set(x, 0, z, 1);
        }
    }
    let mut buf = Vec::with_capacity(4096);
    b.iter(|| {
        buf.clear();
        container.encode_to(black_box(&mut buf));
    });
}

#[bench]
fn palette_encode_diverse(b: &mut Bencher) {
    let mut container = PalettedContainer::filled(0);
    for i in 0..4096u16 {
        container.set(
            i as usize % 16,
            i as usize / 256,
            (i as usize / 16) % 16,
            i % 16,
        );
    }
    let mut buf = Vec::with_capacity(8192);
    b.iter(|| {
        buf.clear();
        container.encode_to(black_box(&mut buf));
    });
}

#[bench]
fn chunk_to_packet_flat(b: &mut Bencher) {
    let mut col = ChunkColumn::new(0, 0);
    FlatWorldGenerator.generate(&mut col);
    b.iter(|| {
        black_box(col.to_packet());
    });
}

#[bench]
fn chunk_to_packet_noise(b: &mut Bencher) {
    let noise = NoiseTerrainGenerator::new(42);
    let mut col = ChunkColumn::new(0, 0);
    noise.generate(&mut col);
    b.iter(|| {
        black_box(col.to_packet());
    });
}
