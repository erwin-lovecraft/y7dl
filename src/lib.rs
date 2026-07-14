//! Rust port of [kkdai/youtube](https://github.com/kkdai/youtube):
//! extract video info (itag, quality, mimeType) from a YouTube link and
//! download a stream at a chosen quality. Transcoding is out of scope.

pub mod client;
mod decipher;
pub mod error;
mod response;
pub mod utils;
pub mod video;

pub use client::Client;
pub use error::{Error, Result};
pub use video::{Format, Video};
