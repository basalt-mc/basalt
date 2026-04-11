mod byte_array;
pub mod error;
mod primitives;
mod string;
pub mod traits;
mod varint;

pub use error::{Error, Result};
pub use traits::{Decode, Encode, EncodedSize};
pub use varint::{VarInt, VarLong};
