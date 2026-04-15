#![no_main]

use libfuzzer_sys::fuzz_target;

use basalt_types::{Decode, Encode, EncodedSize, TextComponent};

fuzz_target!(|data: &[u8]| {
    let mut cursor = data;

    // Fuzz TextComponent decode — recursive NBT-based text parsing.
    // Must not panic on any input.
    if let Ok(component) = TextComponent::decode(&mut cursor) {
        // If decode succeeds, roundtrip must produce identical bytes
        let mut buf = Vec::with_capacity(component.encoded_size());
        component.encode(&mut buf).unwrap();

        let mut buf2 = Vec::new();
        let mut check = buf.as_slice();
        if let Ok(roundtrip) = TextComponent::decode(&mut check) {
            roundtrip.encode(&mut buf2).unwrap();
            assert_eq!(buf, buf2);
        }
    }
});
