//! Recursive folder scan that reads tags into [`Track`]s.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use lofty::prelude::*;
use walkdir::WalkDir;

use crate::track::{Track, TrackId, UNKNOWN_ALBUM, UNKNOWN_ARTIST, UNKNOWN_GENRE};

/// File extensions the scanner accepts, lower case.
const EXTENSIONS: &[&str] = &["m4a", "mp3", "wav", "flac", "ogg", "opus", "aac"];

/// Errors that abort a scan. Per-file failures never abort; they are logged.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// The library root does not exist or is not a directory.
    #[error("library folder does not exist: {0}")]
    RootMissing(PathBuf),
}

/// Walk `root` recursively and read every supported audio file.
///
/// Files that cannot be parsed are skipped with a warning. Returned tracks are
/// sorted by path and carry sequential ids starting at zero.
pub fn scan(root: &Path) -> Result<Vec<Track>, ScanError> {
    if !root.is_dir() {
        return Err(ScanError::RootMissing(root.to_path_buf()));
    }

    let mut paths: Vec<PathBuf> = WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|path| is_audio(path))
        .collect();
    paths.sort();

    let mut tracks: Vec<Track> = paths.iter().filter_map(|path| read_track(path)).collect();
    for (index, track) in tracks.iter_mut().enumerate() {
        track.id = TrackId(u32::try_from(index).unwrap_or(u32::MAX));
    }
    tracing::info!(count = tracks.len(), root = %root.display(), "library scanned");
    Ok(tracks)
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn read_track(path: &Path) -> Option<Track> {
    let tagged = match lofty::read_from_path(path) {
        Ok(tagged) => tagged,
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "skipping unreadable file");
            return None;
        }
    };

    let duration = tagged.properties().duration();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    Some(Track {
        id: TrackId(0),
        path: path.to_path_buf(),
        title: text(tag.and_then(Accessor::title), &stem),
        artist: text(tag.and_then(Accessor::artist), UNKNOWN_ARTIST),
        album: text(tag.and_then(Accessor::album), UNKNOWN_ALBUM),
        genre: text(tag.and_then(Accessor::genre), UNKNOWN_GENRE),
        track_no: tag.and_then(Accessor::track),
        disc_no: tag.and_then(Accessor::disk),
        year: tag.and_then(Accessor::date).map(|d| u32::from(d.year)),
        duration,
        has_art: tag.is_some_and(|t| !t.pictures().is_empty()),
    })
}

fn text(value: Option<Cow<'_, str>>, fallback: &str) -> String {
    value
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn find<'a>(tracks: &'a [Track], name: &str) -> &'a Track {
        tracks
            .iter()
            .find(|t| t.path.file_name().is_some_and(|f| f == name))
            .unwrap_or_else(|| panic!("fixture {name} not scanned"))
    }

    #[test]
    fn scans_m4a_and_mp3_tags() {
        let tracks = scan(&fixtures()).unwrap();

        let alpha = find(&tracks, "alpha.m4a");
        assert_eq!(alpha.title, "Alpha Song");
        assert_eq!(alpha.artist, "Alpha Artist");
        assert_eq!(alpha.album, "Alpha Album");
        assert_eq!(alpha.genre, "Rock");
        assert_eq!(alpha.track_no, Some(1));
        assert_eq!(alpha.year, Some(2001));
        assert!(alpha.duration >= Duration::from_millis(800));
        assert!(alpha.duration <= Duration::from_millis(1500));
        assert!(!alpha.has_art);

        let beta = find(&tracks, "beta.mp3");
        assert_eq!(beta.title, "Beta Song");
        assert_eq!(beta.artist, "Beta Artist");
        assert_eq!(beta.genre, "Jazz");
        assert_eq!(beta.track_no, Some(2));
    }

    #[test]
    fn untagged_wav_falls_back_to_stem_and_unknowns() {
        let tracks = scan(&fixtures()).unwrap();
        let gamma = find(&tracks, "gamma.wav");
        assert_eq!(gamma.title, "gamma");
        assert_eq!(gamma.artist, UNKNOWN_ARTIST);
        assert_eq!(gamma.album, UNKNOWN_ALBUM);
        assert!(gamma.duration >= Duration::from_millis(800));
    }

    #[test]
    fn corrupt_file_is_skipped_and_ids_are_sequential() {
        let tracks = scan(&fixtures()).unwrap();
        assert!(
            tracks
                .iter()
                .all(|t| t.path.file_name().is_none_or(|f| f != "corrupt.mp3"))
        );
        assert_eq!(tracks.len(), 3);
        let ids: Vec<u32> = tracks.iter().map(|t| t.id.0).collect();
        assert_eq!(ids, vec![0, 1, 2]);
    }

    #[test]
    fn missing_root_is_an_error() {
        let err = scan(&fixtures().join("does-not-exist")).unwrap_err();
        assert!(matches!(err, ScanError::RootMissing(_)));
    }
}
