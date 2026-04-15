use criterion::{Criterion, black_box, criterion_group, criterion_main};

use basalt_world::palette::PalettedContainer;
use basalt_world::{ChunkColumn, FlatWorldGenerator, NoiseTerrainGenerator};

fn bench_palette_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("palette_encode");

    group.bench_function("single_value", |b| {
        let container = PalettedContainer::filled(0);
        let mut buf = Vec::with_capacity(64);
        b.iter(|| {
            buf.clear();
            container.encode_to(black_box(&mut buf));
        });
    });

    group.bench_function("two_states", |b| {
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
    });

    group.bench_function("diverse (16 states)", |b| {
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
    });

    group.finish();
}

fn bench_chunk_to_packet(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunk_to_packet");

    group.bench_function("flat", |b| {
        let mut col = ChunkColumn::new(0, 0);
        FlatWorldGenerator.generate(&mut col);
        b.iter(|| {
            black_box(col.to_packet());
        });
    });

    group.bench_function("noise", |b| {
        let noise = NoiseTerrainGenerator::new(42);
        let mut col = ChunkColumn::new(0, 0);
        noise.generate(&mut col);
        b.iter(|| {
            black_box(col.to_packet());
        });
    });

    group.finish();
}

criterion_group!(benches, bench_palette_encode, bench_chunk_to_packet);
criterion_main!(benches);
