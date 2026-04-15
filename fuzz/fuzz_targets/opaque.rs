#![no_main]

use libfuzzer_sys::fuzz_target;

use basalt_types::{Decode, Encode, EncodedSize, OpaqueBytes};

fuzz_target!(|data: &[u8]| {
    let mut cursor = data;

    // Fuzz OpaqueBytes decode — must not panic on arbitrary input
    if let Ok(opaque) = OpaqueBytes::decode(&mut cursor) {
        // If decode succeeds, roundtrip must produce identical bytes
        let mut buf = Vec::with_capacity(opaque.encoded_size());
        opaque.encode(&mut buf).unwrap();

        let mut check = buf.as_slice();
        let roundtrip = OpaqueBytes::decode(&mut check).unwrap();
        assert_eq!(opaque, roundtrip);
    }
});
