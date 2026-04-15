#![no_main]

use libfuzzer_sys::fuzz_target;

use basalt_types::nbt::NbtCompound;
use basalt_types::{Decode, Encode, EncodedSize};

fuzz_target!(|data: &[u8]| {
    let mut cursor = data;

    // Fuzz NbtCompound decode — must not panic on arbitrary input
    if let Ok(compound) = NbtCompound::decode(&mut cursor) {
        // If decode succeeds, roundtrip must produce identical bytes.
        // We compare encoded bytes rather than struct equality because
        // NaN floats break PartialEq (NaN != NaN in IEEE 754).
        let mut buf = Vec::with_capacity(compound.encoded_size());
        compound.encode(&mut buf).unwrap();

        let mut buf2 = Vec::new();
        let mut check = buf.as_slice();
        let roundtrip = NbtCompound::decode(&mut check).unwrap();
        roundtrip.encode(&mut buf2).unwrap();

        assert_eq!(buf, buf2);
    }
});
