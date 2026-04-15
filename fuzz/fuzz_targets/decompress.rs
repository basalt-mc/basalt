#![no_main]

use libfuzzer_sys::fuzz_target;

use basalt_net::compression::decompress_packet;

fuzz_target!(|data: &[u8]| {
    // Fuzz packet decompression — a crafted payload could declare a
    // huge decompressed size or contain invalid zlib data.
    // Must not panic or OOM on any input.
    let _ = decompress_packet(data);
});
