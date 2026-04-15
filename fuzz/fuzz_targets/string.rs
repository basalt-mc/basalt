#![no_main]

use libfuzzer_sys::fuzz_target;

use basalt_types::{Decode, Encode, EncodedSize};

fuzz_target!(|data: &[u8]| {
    let mut cursor = data;

    // Fuzz String decode — must not panic on arbitrary input
    if let Ok(s) = String::decode(&mut cursor) {
        // If decode succeeds, roundtrip must produce identical string
        let mut buf = Vec::with_capacity(s.encoded_size());
        s.encode(&mut buf).unwrap();

        let mut check = buf.as_slice();
        let roundtrip = String::decode(&mut check).unwrap();
        assert_eq!(s, roundtrip);
    }
});
