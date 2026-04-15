#![no_main]

use libfuzzer_sys::fuzz_target;

use basalt_types::nbt::NbtCompound;
use basalt_types::{Decode, Encode, EncodedSize};

fuzz_target!(|data: &[u8]| {
    let mut cursor = data;

    // Fuzz NbtCompound decode — must not panic on arbitrary input
    if let Ok(compound) = NbtCompound::decode(&mut cursor) {
        // If decode succeeds, roundtrip must produce identical NBT
        let mut buf = Vec::with_capacity(compound.encoded_size());
        compound.encode(&mut buf).unwrap();

        let mut check = buf.as_slice();
        let roundtrip = NbtCompound::decode(&mut check).unwrap();
        assert_eq!(compound, roundtrip);
    }
});
