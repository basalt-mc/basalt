use criterion::{Criterion, black_box, criterion_group, criterion_main};

use basalt_types::nbt::{NbtCompound, NbtTag};
use basalt_types::{Decode, Encode, EncodedSize};

fn build_registry_compound() -> NbtCompound {
    let mut compound = NbtCompound::new();
    for i in 0..47 {
        let mut entry = NbtCompound::new();
        entry.insert("id", NbtTag::Int(i));
        entry.insert("name", NbtTag::String(format!("minecraft:damage_type_{i}")));
        entry.insert(
            "scaling",
            NbtTag::String("when_caused_by_living_non_player".into()),
        );
        entry.insert("exhaustion", NbtTag::Float(0.1));
        compound.insert(format!("entry_{i}"), NbtTag::Compound(entry));
    }
    compound
}

fn bench_nbt_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("nbt_encode");

    group.bench_function("small (3 entries)", |b| {
        let mut compound = NbtCompound::new();
        compound.insert("x", NbtTag::Int(10));
        compound.insert("y", NbtTag::Int(64));
        compound.insert("z", NbtTag::Int(-20));
        let mut buf = Vec::with_capacity(compound.encoded_size());
        b.iter(|| {
            buf.clear();
            compound.encode(black_box(&mut buf)).unwrap();
        });
    });

    group.bench_function("registry (47 entries)", |b| {
        let compound = build_registry_compound();
        let mut buf = Vec::with_capacity(compound.encoded_size());
        b.iter(|| {
            buf.clear();
            compound.encode(black_box(&mut buf)).unwrap();
        });
    });

    group.finish();
}

fn bench_nbt_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("nbt_decode");

    group.bench_function("small (3 entries)", |b| {
        let mut compound = NbtCompound::new();
        compound.insert("x", NbtTag::Int(10));
        compound.insert("y", NbtTag::Int(64));
        compound.insert("z", NbtTag::Int(-20));
        let mut buf = Vec::new();
        compound.encode(&mut buf).unwrap();
        b.iter(|| {
            let mut cursor = black_box(buf.as_slice());
            NbtCompound::decode(&mut cursor).unwrap()
        });
    });

    group.bench_function("registry (47 entries)", |b| {
        let compound = build_registry_compound();
        let mut buf = Vec::new();
        compound.encode(&mut buf).unwrap();
        b.iter(|| {
            let mut cursor = black_box(buf.as_slice());
            NbtCompound::decode(&mut cursor).unwrap()
        });
    });

    group.finish();
}

fn bench_nbt_lookup(c: &mut Criterion) {
    let compound = build_registry_compound();

    c.bench_function("nbt_compound_get (47 entries)", |b| {
        b.iter(|| {
            black_box(compound.get("entry_23"));
            black_box(compound.get("entry_46"));
            black_box(compound.get("nonexistent"));
        });
    });
}

criterion_group!(
    benches,
    bench_nbt_encode,
    bench_nbt_decode,
    bench_nbt_lookup
);
criterion_main!(benches);
