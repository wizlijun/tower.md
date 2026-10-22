//! CLI contract C0: error codes, envelope, language and rendering.
// Wired into main.rs by Task 3; until then only tests use these items.
#![allow(dead_code, unused_imports)]

mod code;
mod envelope;

pub use code::ErrorCode;
pub use envelope::{Done, Envelope, ErrorBody, Failure, Next, Retry};
