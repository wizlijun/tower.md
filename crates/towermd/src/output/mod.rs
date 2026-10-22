//! CLI contract C0: error codes, envelope, language and rendering.
mod code;
mod envelope;
pub mod lang;
pub mod render;

pub use code::ErrorCode;
pub use envelope::{Done, Envelope, Failure, Next};
