#![no_main]

use libfuzzer_sys::fuzz_target;

use basalt_types::{Decode, Encode, EncodedSize, Position};

fuzz_target!(|data: &[u8]| {
    let mut cursor = data;

    // Fuzz Position decode — packed i64 with signed bit extraction
    // for x (26 bits), z (26 bits), y (12 bits).
    // Must not panic on any input.
    if let Ok(pos) = Position::decode(&mut cursor) {
        let mut buf = Vec::with_capacity(pos.encoded_size());
        pos.encode(&mut buf).unwrap();

        let mut check = buf.as_slice();
        let roundtrip = Position::decode(&mut check).unwrap();
        assert_eq!(pos, roundtrip);
    }
});
