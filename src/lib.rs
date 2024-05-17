#![warn(clippy::pedantic)]

pub use netfn_core::*;
pub use netfn_macro::*;

#[cfg(feature = "serde")]
pub use serde;
