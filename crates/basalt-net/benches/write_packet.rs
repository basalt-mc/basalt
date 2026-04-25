//! Baseline benchmarks for the outbound packet-write hot path
//! (issue #175, Phase 1).
//!
//! `framing::write_raw_packet` allocates a fresh `Vec<u8>` per call.
//! These benches replicate the synchronous alloc + encode pattern
//! verbatim so the allocator signal is not masked by tokio runtime
//! overhead — the goal is to decide whether a buffer pool would
//! actually help, not to measure TCP throughput.
//!
//! The single-packet benches cover four representative payload sizes
//! (20 B / 28 B / 256 B / 10 KB). The burst benches run a tick's worth
//! of packets in a single iteration to surface allocator amortization
//! effects. The `prealloc` reference bench reuses one `Vec` across
//! iterations — the delta against the fresh-alloc bench is the upper
//! bound on what a pool can deliver.

#![feature(test)]
extern crate test;

use test::{Bencher, black_box};

use basalt_types::{Encode, EncodedSize, VarInt};

/// Frame a single packet exactly as `framing::write_raw_packet` does:
/// VarInt frame length, VarInt packet id, then payload bytes.
#[inline(always)]
fn write_one_packet(packet_id: i32, payload: &[u8]) -> Vec<u8> {
    let id_varint = VarInt(packet_id);
    let frame_length = id_varint.encoded_size() + payload.len();
    let mut buf = Vec::with_capacity(VarInt(frame_length as i32).encoded_size() + frame_length);
    VarInt(frame_length as i32).encode(&mut buf).unwrap();
    id_varint.encode(&mut buf).unwrap();
    buf.extend_from_slice(payload);
    buf
}

// -- Single-packet allocation cost -----------------------------------

#[bench]
fn write_packet_block_change(b: &mut Bencher) {
    let payload = vec![0xBBu8; 20];
    b.iter(|| {
        let buf = write_one_packet(black_box(0x09), black_box(&payload));
        black_box(buf);
    });
}

#[bench]
fn write_packet_movement(b: &mut Bencher) {
    let payload = vec![0xAAu8; 28];
    b.iter(|| {
        let buf = write_one_packet(black_box(0x2E), black_box(&payload));
        black_box(buf);
    });
}

#[bench]
fn write_packet_chat(b: &mut Bencher) {
    let payload = vec![0xCCu8; 256];
    b.iter(|| {
        let buf = write_one_packet(black_box(0x6C), black_box(&payload));
        black_box(buf);
    });
}

#[bench]
fn write_packet_chunk(b: &mut Bencher) {
    let payload = vec![0xDDu8; 10_240];
    b.iter(|| {
        let buf = write_one_packet(black_box(0x27), black_box(&payload));
        black_box(buf);
    });
}

// -- Burst (one tick's worth of packets) -----------------------------

#[bench]
fn write_packet_burst_50_movement(b: &mut Bencher) {
    let payload = vec![0xAAu8; 28];
    b.iter(|| {
        for _ in 0..50 {
            let buf = write_one_packet(black_box(0x2E), black_box(&payload));
            black_box(buf);
        }
    });
}

#[bench]
fn write_packet_burst_200_mixed(b: &mut Bencher) {
    let movement = vec![0xAAu8; 28];
    let block_change = vec![0xBBu8; 20];
    let chat = vec![0xCCu8; 256];
    let chunk = vec![0xDDu8; 10_240];
    b.iter(|| {
        for _ in 0..150 {
            let buf = write_one_packet(black_box(0x2E), black_box(&movement));
            black_box(buf);
        }
        for _ in 0..30 {
            let buf = write_one_packet(black_box(0x09), black_box(&block_change));
            black_box(buf);
        }
        for _ in 0..15 {
            let buf = write_one_packet(black_box(0x6C), black_box(&chat));
            black_box(buf);
        }
        for _ in 0..5 {
            let buf = write_one_packet(black_box(0x27), black_box(&chunk));
            black_box(buf);
        }
    });
}

// -- Pool-ceiling reference ------------------------------------------
//
// Same logical work as `write_packet_movement`, but the buffer is
// reused across iterations (cleared instead of dropped). The gap
// between this bench and `write_packet_movement` is the maximum
// improvement an ideal buffer pool could deliver — without writing
// the pool itself. If the gap is below 10% the issue's threshold,
// Phase 2 should not ship.

#[bench]
fn write_packet_prealloc_movement(b: &mut Bencher) {
    let payload = vec![0xAAu8; 28];
    let id_varint = VarInt(0x2E);
    let frame_length = id_varint.encoded_size() + payload.len();
    let mut buf = Vec::with_capacity(VarInt(frame_length as i32).encoded_size() + frame_length);
    b.iter(|| {
        buf.clear();
        VarInt(frame_length as i32).encode(&mut buf).unwrap();
        id_varint.encode(&mut buf).unwrap();
        buf.extend_from_slice(black_box(&payload));
        black_box(&buf);
    });
}
