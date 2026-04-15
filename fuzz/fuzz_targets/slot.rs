#![no_main]

use libfuzzer_sys::fuzz_target;

use basalt_types::{Decode, Encode, EncodedSize, Slot};

fuzz_target!(|data: &[u8]| {
    let mut cursor = data;

    // Fuzz Slot decode — must not panic on arbitrary input
    if let Ok(slot) = Slot::decode(&mut cursor) {
        // If decode succeeds, roundtrip must produce identical encoding
        let mut buf = Vec::with_capacity(slot.encoded_size());
        slot.encode(&mut buf).unwrap();

        let mut check = buf.as_slice();
        let roundtrip = Slot::decode(&mut check).unwrap();
        assert_eq!(slot.item_count, roundtrip.item_count);
        assert_eq!(slot.item_id, roundtrip.item_id);
    }
});
