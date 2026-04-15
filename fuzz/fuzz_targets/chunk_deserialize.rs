#![no_main]

use libfuzzer_sys::fuzz_target;

use basalt_world::format::deserialize_chunk;

fuzz_target!(|data: &[u8]| {
    // Fuzz chunk deserialization from the BSR on-disk format.
    // A corrupted region file must not crash the server.
    // Must not panic on any input.
    let _ = deserialize_chunk(data, 0, 0);
});
