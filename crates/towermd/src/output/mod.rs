//! CLI contract C0: error codes, envelope, language and rendering.
// `Failure::data` is first produced by a later task (dev commands); drop this
// allow once it is.
mod code;
#[allow(dead_code)]
mod envelope;
pub mod lang;
pub mod render;

pub use code::ErrorCode;
pub use envelope::{Done, Envelope, Failure, Next};
