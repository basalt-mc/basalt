pub mod error;
mod primitives;
pub mod traits;
mod varint;

pub use error::{Error, Result};
pub use traits::{Decode, Encode, EncodedSize};
pub use varint::{VarInt, VarLong};
