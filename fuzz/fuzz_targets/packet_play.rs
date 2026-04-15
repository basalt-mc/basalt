#![no_main]

use libfuzzer_sys::fuzz_target;

use basalt_protocol::packets::play::ServerboundPlayPacket;

fuzz_target!(|data: &[u8]| {
    // First byte is the packet ID, rest is payload
    if data.is_empty() {
        return;
    }
    let id = data[0] as i32;
    let mut cursor = &data[1..];

    // Fuzz the main protocol entry point for untrusted client data.
    // Must not panic on any input.
    let _ = ServerboundPlayPacket::decode_by_id(id, &mut cursor);
});
