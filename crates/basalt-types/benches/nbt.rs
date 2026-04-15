#![feature(test)]
extern crate test;

use test::{Bencher, black_box};

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

#[bench]
fn nbt_encode_small(b: &mut Bencher) {
    let mut compound = NbtCompound::new();
    compound.insert("x", NbtTag::Int(10));
    compound.insert("y", NbtTag::Int(64));
    compound.insert("z", NbtTag::Int(-20));
    let mut buf = Vec::with_capacity(compound.encoded_size());
    b.iter(|| {
        buf.clear();
        compound.encode(black_box(&mut buf)).unwrap();
    });
}

#[bench]
fn nbt_encode_registry(b: &mut Bencher) {
    let compound = build_registry_compound();
    let mut buf = Vec::with_capacity(compound.encoded_size());
    b.iter(|| {
        buf.clear();
        compound.encode(black_box(&mut buf)).unwrap();
    });
}

#[bench]
fn nbt_decode_small(b: &mut Bencher) {
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
}

#[bench]
fn nbt_decode_registry(b: &mut Bencher) {
    let compound = build_registry_compound();
    let mut buf = Vec::new();
    compound.encode(&mut buf).unwrap();
    b.iter(|| {
        let mut cursor = black_box(buf.as_slice());
        NbtCompound::decode(&mut cursor).unwrap()
    });
}

#[bench]
fn nbt_compound_lookup(b: &mut Bencher) {
    let compound = build_registry_compound();
    b.iter(|| {
        black_box(compound.get("entry_23"));
        black_box(compound.get("entry_46"));
        black_box(compound.get("nonexistent"));
    });
}
