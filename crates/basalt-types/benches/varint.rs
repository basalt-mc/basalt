#![feature(test)]
extern crate test;

use test::{Bencher, black_box};

use basalt_types::{Decode, Encode, EncodedSize, VarInt};

#[bench]
fn varint_encode_small(b: &mut Bencher) {
    let v = VarInt(1);
    let mut buf = Vec::with_capacity(v.encoded_size());
    b.iter(|| {
        buf.clear();
        v.encode(black_box(&mut buf)).unwrap();
    });
}

#[bench]
fn varint_encode_medium(b: &mut Bencher) {
    let v = VarInt(25565);
    let mut buf = Vec::with_capacity(v.encoded_size());
    b.iter(|| {
        buf.clear();
        v.encode(black_box(&mut buf)).unwrap();
    });
}

#[bench]
fn varint_encode_max(b: &mut Bencher) {
    let v = VarInt(i32::MAX);
    let mut buf = Vec::with_capacity(v.encoded_size());
    b.iter(|| {
        buf.clear();
        v.encode(black_box(&mut buf)).unwrap();
    });
}

#[bench]
fn varint_decode_small(b: &mut Bencher) {
    let v = VarInt(1);
    let mut buf = Vec::with_capacity(v.encoded_size());
    v.encode(&mut buf).unwrap();
    b.iter(|| {
        let mut cursor = black_box(buf.as_slice());
        VarInt::decode(&mut cursor).unwrap()
    });
}

#[bench]
fn varint_decode_medium(b: &mut Bencher) {
    let v = VarInt(25565);
    let mut buf = Vec::with_capacity(v.encoded_size());
    v.encode(&mut buf).unwrap();
    b.iter(|| {
        let mut cursor = black_box(buf.as_slice());
        VarInt::decode(&mut cursor).unwrap()
    });
}

#[bench]
fn varint_decode_max(b: &mut Bencher) {
    let v = VarInt(i32::MAX);
    let mut buf = Vec::with_capacity(v.encoded_size());
    v.encode(&mut buf).unwrap();
    b.iter(|| {
        let mut cursor = black_box(buf.as_slice());
        VarInt::decode(&mut cursor).unwrap()
    });
}

#[bench]
fn string_encode_short(b: &mut Bencher) {
    let s = "a".repeat(16);
    let mut buf = Vec::with_capacity(s.encoded_size());
    b.iter(|| {
        buf.clear();
        s.encode(black_box(&mut buf)).unwrap();
    });
}

#[bench]
fn string_encode_chat(b: &mut Bencher) {
    let s = "a".repeat(256);
    let mut buf = Vec::with_capacity(s.encoded_size());
    b.iter(|| {
        buf.clear();
        s.encode(black_box(&mut buf)).unwrap();
    });
}
