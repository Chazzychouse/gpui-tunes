//! The track model shared by every other module.

use std::path::PathBuf;
use std::time::Duration;

/// Identifier for a track, stable for the lifetime of one [`crate::Library`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TrackId(pub u32);

/// Metadata for one audio file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// Library-assigned identifier.
    pub id: TrackId,
    /// Absolute path of the audio file.
    pub path: PathBuf,
    /// Title tag, or the file stem when missing.
    pub title: String,
    /// Artist tag, or "Unknown Artist".
    pub artist: String,
    /// Album tag, or "Unknown Album".
    pub album: String,
    /// Genre tag, or "Unknown Genre".
    pub genre: String,
    /// Track number within the disc, when tagged.
    pub track_no: Option<u32>,
    /// Disc number, when tagged.
    pub disc_no: Option<u32>,
    /// Release year, when tagged.
    pub year: Option<u32>,
    /// Decoded length of the audio.
    pub duration: Duration,
    /// Whether the file carries an embedded picture.
    pub has_art: bool,
}

/// Placeholder artist for untagged files.
pub const UNKNOWN_ARTIST: &str = "Unknown Artist";
/// Placeholder album for untagged files.
pub const UNKNOWN_ALBUM: &str = "Unknown Album";
/// Placeholder genre for untagged files.
pub const UNKNOWN_GENRE: &str = "Unknown Genre";
