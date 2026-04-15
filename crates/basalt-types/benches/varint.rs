use criterion::{Criterion, black_box, criterion_group, criterion_main};

use basalt_types::{Decode, Encode, EncodedSize, VarInt};

fn bench_varint_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("varint_encode");

    group.bench_function("small (1 byte)", |b| {
        let v = VarInt(1);
        let mut buf = Vec::with_capacity(v.encoded_size());
        b.iter(|| {
            buf.clear();
            v.encode(black_box(&mut buf)).unwrap();
        });
    });

    group.bench_function("medium (3 bytes)", |b| {
        let v = VarInt(25565);
        let mut buf = Vec::with_capacity(v.encoded_size());
        b.iter(|| {
            buf.clear();
            v.encode(black_box(&mut buf)).unwrap();
        });
    });

    group.bench_function("max (5 bytes)", |b| {
        let v = VarInt(i32::MAX);
        let mut buf = Vec::with_capacity(v.encoded_size());
        b.iter(|| {
            buf.clear();
            v.encode(black_box(&mut buf)).unwrap();
        });
    });

    group.finish();
}

fn bench_varint_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("varint_decode");

    for (name, value) in [("small", 1i32), ("medium", 25565), ("max", i32::MAX)] {
        let v = VarInt(value);
        let mut buf = Vec::with_capacity(v.encoded_size());
        v.encode(&mut buf).unwrap();

        group.bench_function(name, |b| {
            b.iter(|| {
                let mut cursor = black_box(buf.as_slice());
                VarInt::decode(&mut cursor).unwrap()
            });
        });
    }

    group.finish();
}

fn bench_string_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("string_encode");

    for (name, len) in [("short", 16), ("chat_msg", 256), ("max", 32767)] {
        let s = "a".repeat(len);
        let mut buf = Vec::with_capacity(s.encoded_size());

        group.bench_function(name, |b| {
            b.iter(|| {
                buf.clear();
                s.encode(black_box(&mut buf)).unwrap();
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_varint_encode,
    bench_varint_decode,
    bench_string_encode
);
criterion_main!(benches);
