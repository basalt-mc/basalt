use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Result};

use crate::decode::derive_decode;
use crate::encode::derive_encode_fields_only;
use crate::size::derive_encoded_size_fields_only;

/// Generates `Encode`, `Decode`, `EncodedSize` implementations and a
/// `PACKET_ID` constant for a packet struct.
///
/// The `packet_id` is passed directly from the attribute macro's argument
/// (e.g., `#[packet(id = 0x00)]` passes `0x00`).
pub fn expand_packet(input: &DeriveInput, packet_id: i32) -> Result<TokenStream> {
    let name = &input.ident;

    let encode_impl = derive_encode_fields_only(input)?;
    let decode_impl = derive_decode(input)?;
    let size_impl = derive_encoded_size_fields_only(input)?;

    Ok(quote! {
        impl #name {
            /// The packet ID used by the registry to dispatch this packet.
            ///
            /// This value is declared via `#[packet(id = N)]` and corresponds to
            /// the VarInt packet ID read/written by the framing layer. The struct's
            /// own Encode/Decode does NOT include this ID — it only encodes the
            /// packet's payload fields.
            pub const PACKET_ID: i32 = #packet_id;
        }

        #encode_impl
        #decode_impl
        #size_impl
    })
}
