#![no_main]

use libfuzzer_sys::fuzz_target;

use basalt_types::{Decode, Encode, EncodedSize, VarInt};

fuzz_target!(|data: &[u8]| {
    let mut cursor = data;

    // Fuzz VarInt decode — must not panic on arbitrary input
    if let Ok(varint) = VarInt::decode(&mut cursor) {
        // If decode succeeds, roundtrip must produce identical bytes
        let mut buf = Vec::with_capacity(varint.encoded_size());
        varint.encode(&mut buf).unwrap();

        let mut check = buf.as_slice();
        let roundtrip = VarInt::decode(&mut check).unwrap();
        assert_eq!(varint.0, roundtrip.0);
    }
});
