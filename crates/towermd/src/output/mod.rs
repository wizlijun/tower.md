//! CLI contract C0: error codes, envelope, language and rendering.
// Some codes and `Failure::data` are first produced by later tasks (daemon
// error codes, dev commands); drop these allows once they are.
#[allow(dead_code)]
mod code;
#[allow(dead_code)]
mod envelope;
pub mod lang;
pub mod render;

pub use code::ErrorCode;
pub use envelope::{Done, Envelope, Failure, Next};
