//! Backend for gpui-tunes: scanning, library model, queue and playback.
//!
//! This crate has no UI dependency. The UI crate calls into it and renders
//! the plain structs it returns.
#![deny(missing_docs)]

pub mod scanner;
pub mod track;

pub use scanner::{ScanError, scan};
pub use track::{Track, TrackId, UNKNOWN_ALBUM, UNKNOWN_ARTIST, UNKNOWN_GENRE};
