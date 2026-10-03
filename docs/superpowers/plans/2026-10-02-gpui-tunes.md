# gpui-tunes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A classic-iTunes-style desktop player for `~/music/library` built on GPUI Kit, with a backend crate that has no UI dependency.

**Architecture:** Cargo workspace with two crates. `tunes-core` (library crate) scans tags with lofty, holds the `Library` query model, a pure `Queue`, and a rodio `Player` driven over a channel from a dedicated audio thread. `gpui-tunes` (binary) renders four views (toolbar, sidebar, column browser, song table) from plain structs, talks to core through method calls, and polls playback status on a 250 ms timer.

**Tech Stack:** Rust 1.96 edition 2024, gpui-kit 0.7 (gpui + gpui-base + gpui-component), rodio 0.22 (symphonia AAC/MP4), lofty 0.25, walkdir, crossbeam-channel, thiserror, tracing.

## Global Constraints

- Workspace layout: `crates/tunes-core` (lib) and `crates/gpui-tunes` (bin). `crates/tunes-core/Cargo.toml` MUST NOT list any gpui crate.
- Library root: `~/music/library`, overridable by env var `GPUI_TUNES_LIBRARY`.
- Dark theme is set at startup: `Theme::change(ThemeMode::Dark, None, cx)`. No light toggle.
- Lints: `unsafe_code = "forbid"`, `clippy::all` + `clippy::pedantic` warn, `clippy::unwrap_used` deny (tests may `#[allow(clippy::unwrap_used)]`). `tunes-core` is `#![deny(missing_docs)]`.
- `cargo fmt --all` and `cargo clippy --workspace --all-targets` clean before every commit.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Verified crate sources are unpacked for reference at `/tmp/claude-1000/-home-chazzy-development-projects-gpui-tunes/9a9a20d2-0411-499b-8215-aa57f64b87c4/scratchpad/` (`gpui-component-0.7.0`, `gpui-base-0.7.0`, `gpui-pre-0.3.7`, `rodio-0.22.2`, `lofty-0.25.4`, and the `gk` repo clone with `crates/story/src/stories/*.rs` examples). When a signature in this plan does not compile, grep there before guessing.
- Reference icon names come from `gpui_kit::assets::IconName` (the full Lucide set: `SkipBack`, `SkipForward`, `Play`, `Pause`, `Search`, `Music`, `Volume2`, `Disc`, `User`, `ListMusic`, `Library`, `LoaderCircle`). `gpui_kit::component::IconName` is a smaller subset; do not use it for the transport icons.

---

## File map

```
Cargo.toml                                  workspace, shared lints
crates/tunes-core/Cargo.toml
crates/tunes-core/src/lib.rs                module list + re-exports
crates/tunes-core/src/track.rs              Track, TrackId
crates/tunes-core/src/scanner.rs            scan(root) -> Vec<Track>
crates/tunes-core/src/library.rs            Library, Filter, Sort, SortColumn, cover_art
crates/tunes-core/src/queue.rs              Queue
crates/tunes-core/src/player.rs             Player, PlaybackStatus, Event, audio thread
crates/tunes-core/tests/fixtures/*          alpha.m4a beta.mp3 gamma.wav corrupt.mp3
crates/gpui-tunes/Cargo.toml
crates/gpui-tunes/src/main.rs               startup, window, theme
crates/gpui-tunes/src/ui/mod.rs             module list
crates/gpui-tunes/src/ui/format.rs          fmt_duration
crates/gpui-tunes/src/ui/art.rs             bytes -> Arc<gpui::Image>
crates/gpui-tunes/src/ui/section.rs         Section enum (Songs/Artists/Albums/Genres)
crates/gpui-tunes/src/ui/sidebar.rs         LibrarySidebar view + SidebarEvent
crates/gpui-tunes/src/ui/browser.rs         ColumnBrowser view + BrowserEvent
crates/gpui-tunes/src/ui/song_table.rs      SongTableDelegate
crates/gpui-tunes/src/ui/toolbar.rs         Toolbar view + ToolbarEvent + ToolbarModel
crates/gpui-tunes/src/ui/app.rs             AppView: owns core objects, wires events, poll loop
README.md
```

---

### Task 1: Workspace scaffold that opens a dark GPUI window

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `crates/tunes-core/Cargo.toml`, `crates/tunes-core/src/lib.rs`, `crates/gpui-tunes/Cargo.toml`, `crates/gpui-tunes/src/main.rs`

**Interfaces:**
- Produces: a compiling workspace; `tunes_core` crate name; `gpui-tunes` binary.

- [ ] **Step 1: Write workspace Cargo.toml and .gitignore**

`Cargo.toml`:
```toml
[workspace]
resolver = "3"
members = ["crates/tunes-core", "crates/gpui-tunes"]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"

[workspace.dependencies]
tunes-core = { path = "crates/tunes-core" }
gpui-kit = "0.7"
rodio = { version = "0.22", features = ["symphonia-aac", "symphonia-isomp4"] }
lofty = "0.25"
walkdir = "2"
crossbeam-channel = "0.5"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
dirs = "6"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
unwrap_used = "deny"
module_name_repetitions = "allow"
must_use_candidate = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"

# gpui is slow in unoptimized builds; optimize dependencies even in dev.
[profile.dev.package."*"]
opt-level = 2
```

`.gitignore`:
```
/target
```

- [ ] **Step 2: Write tunes-core crate skeleton**

`crates/tunes-core/Cargo.toml`:
```toml
[package]
name = "tunes-core"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Library scanning, querying and playback for gpui-tunes. No UI dependencies."

[dependencies]
lofty.workspace = true
rodio.workspace = true
walkdir.workspace = true
crossbeam-channel.workspace = true
thiserror.workspace = true
tracing.workspace = true

[lints]
workspace = true
```

`crates/tunes-core/src/lib.rs`:
```rust
//! Backend for gpui-tunes: scanning, library model, queue and playback.
//!
//! This crate has no UI dependency. The UI crate calls into it and renders
//! the plain structs it returns.
#![deny(missing_docs)]
```

- [ ] **Step 3: Write gpui-tunes crate with a hello window in dark mode**

`crates/gpui-tunes/Cargo.toml`:
```toml
[package]
name = "gpui-tunes"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Classic-iTunes-style desktop player built on GPUI Kit."

[dependencies]
tunes-core.workspace = true
gpui-kit.workspace = true
dirs.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true

[lints]
workspace = true
```

`crates/gpui-tunes/src/main.rs`:
```rust
//! gpui-tunes binary entry point.

use gpui_kit::component::{ActiveTheme, Theme, ThemeMode, TitleBar, v_flex};
use gpui_kit::*;

struct Hello;

impl Render for Hello {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(TitleBar::new())
            .child(div().p_4().child("gpui-tunes"))
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);

        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("gpui-tunes".into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1200.), px(760.)),
                cx,
            ))),
            ..Default::default()
        };

        gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| Hello))
            .expect("failed to open window");
    });
}
```

If `TitleBar` is not found, remove that child; it is optional.

- [ ] **Step 4: Build and run**

Run: `cargo build 2>&1 | tail -20`
Expected: `Finished` with no errors (first build takes several minutes).

Run: `timeout 8 cargo run 2>&1 | tail -5` and confirm a dark window titled gpui-tunes appears, then is killed by the timeout. If the build fails on a missing system library (fontconfig, xkbcommon, wayland, alsa), report the exact missing package in the task result; do not try to install it.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets 2>&1 | tail -5`
Expected: no warnings.

```bash
git add -A
git commit -m "Scaffold workspace with tunes-core and gpui-tunes crates

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Track model and tag scanner

**Files:**
- Create: `crates/tunes-core/src/track.rs`, `crates/tunes-core/src/scanner.rs`, `crates/tunes-core/tests/fixtures/{alpha.m4a,beta.mp3,gamma.wav,corrupt.mp3}`
- Modify: `crates/tunes-core/src/lib.rs`

**Interfaces:**
- Produces: `TrackId(pub u32)`, `Track { id, path, title, artist, album, genre, track_no, disc_no, year, duration, has_art }`, `scanner::scan(root: &Path) -> Result<Vec<Track>, ScanError>`, `ScanError::RootMissing(PathBuf)`.

- [ ] **Step 1: Generate fixtures with ffmpeg**

```bash
mkdir -p crates/tunes-core/tests/fixtures && cd crates/tunes-core/tests/fixtures
ffmpeg -y -loglevel error -f lavfi -i anullsrc=r=44100:cl=mono -t 1 \
  -metadata title="Alpha Song" -metadata artist="Alpha Artist" -metadata album="Alpha Album" \
  -metadata genre="Rock" -metadata track=1 -metadata disc=1 -metadata date=2001 \
  -c:a aac -b:a 32k alpha.m4a
ffmpeg -y -loglevel error -f lavfi -i anullsrc=r=44100:cl=mono -t 1 \
  -metadata title="Beta Song" -metadata artist="Beta Artist" -metadata album="Beta Album" \
  -metadata genre="Jazz" -metadata track=2 -metadata date=2002 \
  -c:a libmp3lame -b:a 32k -id3v2_version 3 beta.mp3
ffmpeg -y -loglevel error -f lavfi -i anullsrc=r=44100:cl=mono -t 1 -c:a pcm_s16le gamma.wav
printf 'this is not audio\n' > corrupt.mp3
ls -la
```
Expected: four files, each under 30 KB.

- [ ] **Step 2: Write track.rs**

`crates/tunes-core/src/track.rs`:
```rust
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
```

- [ ] **Step 3: Write the failing scanner test**

Append to a new `crates/tunes-core/src/scanner.rs` (implementation comes in Step 5; write the test module first so the test fails on missing items):
```rust
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
```

- [ ] **Step 4: Run tests to verify they fail**

Add to `lib.rs`:
```rust
pub mod scanner;
pub mod track;

pub use scanner::{ScanError, scan};
pub use track::{Track, TrackId, UNKNOWN_ALBUM, UNKNOWN_ARTIST, UNKNOWN_GENRE};
```

Run: `cargo test -p tunes-core 2>&1 | tail -20`
Expected: compile error, `scan` and `ScanError` not found.

- [ ] **Step 5: Implement the scanner**

Prepend to `crates/tunes-core/src/scanner.rs` (above the test module):
```rust
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
        year: tag.and_then(Accessor::year),
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
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p tunes-core 2>&1 | tail -15`
Expected: `test result: ok. 4 passed`.

If the mp3 assertions fail on `genre` or `track_no`, inspect with `ffprobe -show_format crates/tunes-core/tests/fixtures/beta.mp3` and adjust the ffmpeg `-metadata` keys, not the test expectations.

- [ ] **Step 7: Lint and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets 2>&1 | tail -5`

```bash
git add -A
git commit -m "Add track model and lofty-based folder scanner with fixtures

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Library query model

**Files:**
- Create: `crates/tunes-core/src/library.rs`
- Modify: `crates/tunes-core/src/lib.rs`

**Interfaces:**
- Consumes: `Track`, `TrackId` from Task 2.
- Produces:
  - `Filter { genre: Option<String>, artist: Option<String>, album: Option<String>, search: String }` (Default = everything).
  - `SortColumn { Title, Artist, Album, Genre, Duration, TrackNo }`, `Sort { column, ascending }`, `Sort::default()` = Album ascending.
  - `Library::new(Vec<Track>)`, `len()`, `is_empty()`, `track(TrackId) -> Option<&Track>`, `genres() -> &[String]`, `artists_in(Option<&str>) -> Vec<String>`, `albums_in(Option<&str>, Option<&str>) -> Vec<String>`, `query(&Filter, Sort) -> Vec<TrackId>`, `sort_ids(&mut [TrackId], Sort)`, `cover_art(TrackId) -> Option<Arc<[u8]>>`.

- [ ] **Step 1: Write the failing tests**

`crates/tunes-core/src/library.rs` test module:
```rust
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    fn track(
        title: &str,
        artist: &str,
        album: &str,
        genre: &str,
        disc: u32,
        no: u32,
        secs: u64,
    ) -> Track {
        Track {
            id: TrackId(0),
            path: PathBuf::from(format!("/music/{title}.m4a")),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            genre: genre.into(),
            track_no: Some(no),
            disc_no: Some(disc),
            year: None,
            duration: Duration::from_secs(secs),
            has_art: false,
        }
    }

    fn library() -> Library {
        Library::new(vec![
            track("Zeta", "bou", "Swerve It", "Drum & Bass", 1, 2, 200),
            track("Alpha", "Bou", "Swerve It", "Drum & Bass", 1, 1, 180),
            track("Mid", "Shakewell", "Big Juice", "Hip-Hop", 1, 1, 150),
            track("Late", "Shakewell", "Big Juice", "Hip-Hop", 2, 1, 160),
            track("Solo", "Shakewell", "Other", "Hip-Hop", 1, 1, 90),
        ])
    }

    fn titles(lib: &Library, ids: &[TrackId]) -> Vec<String> {
        ids.iter().map(|id| lib.track(*id).unwrap().title.clone()).collect()
    }

    #[test]
    fn assigns_sequential_ids() {
        let lib = library();
        let ids: Vec<u32> = lib.tracks().iter().map(|t| t.id.0).collect();
        assert_eq!(ids, vec![0, 1, 2, 3, 4]);
        assert_eq!(lib.track(TrackId(2)).unwrap().title, "Mid");
        assert!(lib.track(TrackId(99)).is_none());
    }

    #[test]
    fn lists_are_sorted_case_insensitively_and_deduplicated() {
        let lib = library();
        assert_eq!(lib.genres(), &["Drum & Bass", "Hip-Hop"]);
        // "bou" and "Bou" are distinct artists but sort together.
        assert_eq!(lib.artists_in(None), vec!["bou", "Bou", "Shakewell"]);
        assert_eq!(lib.artists_in(Some("Hip-Hop")), vec!["Shakewell"]);
        assert_eq!(
            lib.albums_in(Some("Hip-Hop"), Some("Shakewell")),
            vec!["Big Juice", "Other"]
        );
        assert_eq!(lib.albums_in(None, None), vec!["Big Juice", "Other", "Swerve It"]);
    }

    #[test]
    fn default_sort_is_album_disc_track_title() {
        let lib = library();
        let ids = lib.query(&Filter::default(), Sort::default());
        assert_eq!(
            titles(&lib, &ids),
            vec!["Mid", "Late", "Solo", "Alpha", "Zeta"]
        );
    }

    #[test]
    fn filters_combine_and_search_is_case_insensitive() {
        let lib = library();
        let filter = Filter {
            genre: Some("Hip-Hop".into()),
            album: Some("Big Juice".into()),
            ..Filter::default()
        };
        assert_eq!(titles(&lib, &lib.query(&filter, Sort::default())), vec!["Mid", "Late"]);

        let filter = Filter { search: "sWERVE".into(), ..Filter::default() };
        assert_eq!(titles(&lib, &lib.query(&filter, Sort::default())), vec!["Alpha", "Zeta"]);

        let filter = Filter { search: "nothing here".into(), ..Filter::default() };
        assert!(lib.query(&filter, Sort::default()).is_empty());
    }

    #[test]
    fn sorts_by_column_in_both_directions() {
        let lib = library();
        let asc = Sort { column: SortColumn::Title, ascending: true };
        let desc = Sort { column: SortColumn::Duration, ascending: false };
        assert_eq!(
            titles(&lib, &lib.query(&Filter::default(), asc)),
            vec!["Alpha", "Late", "Mid", "Solo", "Zeta"]
        );
        assert_eq!(
            titles(&lib, &lib.query(&Filter::default(), desc)),
            vec!["Zeta", "Alpha", "Late", "Mid", "Solo"]
        );

        let mut ids = vec![TrackId(0), TrackId(1)];
        lib.sort_ids(&mut ids, asc);
        assert_eq!(titles(&lib, &ids), vec!["Alpha", "Zeta"]);
    }

    #[test]
    fn cover_art_is_none_for_tracks_without_art() {
        let lib = Library::new(vec![track("x", "a", "b", "c", 1, 1, 1)]);
        assert!(lib.cover_art(TrackId(0)).is_none());
        assert!(lib.cover_art(TrackId(5)).is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Add `pub mod library;` and `pub use library::{Filter, Library, Sort, SortColumn};` to `lib.rs`.

Run: `cargo test -p tunes-core library 2>&1 | tail -10`
Expected: compile errors for missing `Library`, `Filter`, `Sort`.

- [ ] **Step 3: Implement library.rs**

Prepend above the test module:
```rust
//! In-memory library: lookup, column-browser lists, filtering, sorting, art.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use lofty::prelude::*;

use crate::track::{Track, TrackId};

/// Narrowing criteria for [`Library::query`]. `None` or empty means "all".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Exact genre match.
    pub genre: Option<String>,
    /// Exact artist match.
    pub artist: Option<String>,
    /// Exact album match.
    pub album: Option<String>,
    /// Case-insensitive substring over title, artist and album.
    pub search: String,
}

/// Column a track list is ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    /// Title, then artist.
    Title,
    /// Artist, then album, disc, track, title.
    Artist,
    /// Album, then disc, track, title. This is the default ordering.
    Album,
    /// Genre, then the album ordering.
    Genre,
    /// Duration, then title.
    Duration,
    /// Track number, then title.
    TrackNo,
}

/// Column plus direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    /// Column to order by.
    pub column: SortColumn,
    /// `true` for ascending.
    pub ascending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self { column: SortColumn::Album, ascending: true }
    }
}

/// All scanned tracks plus derived lists and a cover-art cache.
pub struct Library {
    tracks: Vec<Track>,
    genres: Vec<String>,
    art_cache: Mutex<HashMap<TrackId, Option<Arc<[u8]>>>>,
}

impl std::fmt::Debug for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Library").field("tracks", &self.tracks.len()).finish()
    }
}

impl Library {
    /// Build a library, assigning sequential ids in the given order.
    pub fn new(mut tracks: Vec<Track>) -> Self {
        for (index, track) in tracks.iter_mut().enumerate() {
            track.id = TrackId(u32::try_from(index).unwrap_or(u32::MAX));
        }
        let genres = unique_sorted(tracks.iter().map(|t| t.genre.as_str()));
        Self { tracks, genres, art_cache: Mutex::new(HashMap::new()) }
    }

    /// Number of tracks.
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Whether the library has no tracks.
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Every track in id order.
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Look up one track.
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.get(id.0 as usize)
    }

    /// Distinct genres, sorted case-insensitively.
    pub fn genres(&self) -> &[String] {
        &self.genres
    }

    /// Distinct artists, optionally restricted to one genre.
    pub fn artists_in(&self, genre: Option<&str>) -> Vec<String> {
        unique_sorted(
            self.tracks
                .iter()
                .filter(|t| genre.is_none_or(|g| t.genre == g))
                .map(|t| t.artist.as_str()),
        )
    }

    /// Distinct albums, optionally restricted to one genre and/or artist.
    pub fn albums_in(&self, genre: Option<&str>, artist: Option<&str>) -> Vec<String> {
        unique_sorted(
            self.tracks
                .iter()
                .filter(|t| genre.is_none_or(|g| t.genre == g))
                .filter(|t| artist.is_none_or(|a| t.artist == a))
                .map(|t| t.album.as_str()),
        )
    }

    /// Ids of the tracks matching `filter`, ordered by `sort`.
    pub fn query(&self, filter: &Filter, sort: Sort) -> Vec<TrackId> {
        let needle = filter.search.trim().to_lowercase();
        let mut ids: Vec<TrackId> = self
            .tracks
            .iter()
            .filter(|t| filter.genre.as_deref().is_none_or(|g| t.genre == g))
            .filter(|t| filter.artist.as_deref().is_none_or(|a| t.artist == a))
            .filter(|t| filter.album.as_deref().is_none_or(|a| t.album == a))
            .filter(|t| needle.is_empty() || matches_search(t, &needle))
            .map(|t| t.id)
            .collect();
        self.sort_ids(&mut ids, sort);
        ids
    }

    /// Reorder `ids` in place by `sort`. Unknown ids sort last.
    pub fn sort_ids(&self, ids: &mut [TrackId], sort: Sort) {
        ids.sort_by(|a, b| {
            let ordering = match (self.track(*a), self.track(*b)) {
                (Some(a), Some(b)) => compare(a, b, sort.column),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            };
            if sort.ascending { ordering } else { ordering.reverse() }
        });
    }

    /// Raw bytes of the embedded front cover, read lazily and memoized.
    pub fn cover_art(&self, id: TrackId) -> Option<Arc<[u8]>> {
        let track = self.track(id)?;
        if !track.has_art {
            return None;
        }
        let mut cache = self.art_cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache.entry(id).or_insert_with(|| read_art(track)).clone()
    }
}

fn read_art(track: &Track) -> Option<Arc<[u8]>> {
    let tagged = lofty::read_from_path(&track.path)
        .map_err(|err| tracing::warn!(path = %track.path.display(), %err, "cover art unreadable"))
        .ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let picture = tag.pictures().first()?;
    Some(Arc::from(picture.data()))
}

fn matches_search(track: &Track, needle: &str) -> bool {
    track.title.to_lowercase().contains(needle)
        || track.artist.to_lowercase().contains(needle)
        || track.album.to_lowercase().contains(needle)
}

fn unique_sorted<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = values.map(str::to_owned).collect();
    out.sort_by(|a, b| ci(a, b).then_with(|| a.cmp(b)));
    out.dedup();
    out
}

fn ci(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

fn album_order(a: &Track, b: &Track) -> Ordering {
    ci(&a.album, &b.album)
        .then_with(|| a.disc_no.unwrap_or(0).cmp(&b.disc_no.unwrap_or(0)))
        .then_with(|| a.track_no.unwrap_or(0).cmp(&b.track_no.unwrap_or(0)))
        .then_with(|| ci(&a.title, &b.title))
}

fn compare(a: &Track, b: &Track, column: SortColumn) -> Ordering {
    match column {
        SortColumn::Title => ci(&a.title, &b.title).then_with(|| ci(&a.artist, &b.artist)),
        SortColumn::Artist => ci(&a.artist, &b.artist).then_with(|| album_order(a, b)),
        SortColumn::Album => album_order(a, b),
        SortColumn::Genre => ci(&a.genre, &b.genre).then_with(|| album_order(a, b)),
        SortColumn::Duration => a.duration.cmp(&b.duration).then_with(|| ci(&a.title, &b.title)),
        SortColumn::TrackNo => a
            .track_no
            .unwrap_or(0)
            .cmp(&b.track_no.unwrap_or(0))
            .then_with(|| ci(&a.title, &b.title)),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p tunes-core 2>&1 | tail -15`
Expected: `test result: ok. 10 passed`.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets 2>&1 | tail -5`

```bash
git add -A
git commit -m "Add Library query model with filtering, sorting and cover art

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Queue

**Files:**
- Create: `crates/tunes-core/src/queue.rs`
- Modify: `crates/tunes-core/src/lib.rs`

**Interfaces:**
- Produces: `Queue::new()`, `set_order(Vec<TrackId>)`, `order() -> &[TrackId]`, `current() -> Option<TrackId>`, `jump_to(TrackId) -> bool`, `next() -> Option<TrackId>`, `prev() -> Option<TrackId>`.

- [ ] **Step 1: Write the failing tests**

`crates/tunes-core/src/queue.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[u32]) -> Vec<TrackId> {
        list.iter().map(|n| TrackId(*n)).collect()
    }

    #[test]
    fn empty_queue_yields_nothing() {
        let mut q = Queue::new();
        assert_eq!(q.current(), None);
        assert_eq!(q.next(), None);
        assert_eq!(q.prev(), None);
        assert!(!q.jump_to(TrackId(1)));
    }

    #[test]
    fn next_and_prev_wrap_around() {
        let mut q = Queue::new();
        q.set_order(ids(&[5, 6, 7]));
        assert!(q.jump_to(TrackId(6)));
        assert_eq!(q.current(), Some(TrackId(6)));
        assert_eq!(q.next(), Some(TrackId(7)));
        assert_eq!(q.next(), Some(TrackId(5)));
        assert_eq!(q.prev(), Some(TrackId(7)));
    }

    #[test]
    fn next_without_current_starts_at_front() {
        let mut q = Queue::new();
        q.set_order(ids(&[1, 2]));
        assert_eq!(q.next(), Some(TrackId(1)));
        let mut q = Queue::new();
        q.set_order(ids(&[1, 2]));
        assert_eq!(q.prev(), Some(TrackId(2)));
    }

    #[test]
    fn replacing_order_keeps_current_if_present() {
        let mut q = Queue::new();
        q.set_order(ids(&[1, 2, 3]));
        q.jump_to(TrackId(2));
        q.set_order(ids(&[3, 2]));
        assert_eq!(q.current(), Some(TrackId(2)));
        assert_eq!(q.next(), Some(TrackId(3)));
        q.set_order(ids(&[9]));
        assert_eq!(q.current(), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Add `pub mod queue;` and `pub use queue::Queue;` to `lib.rs`.

Run: `cargo test -p tunes-core queue 2>&1 | tail -5`
Expected: compile error, `Queue` not found.

- [ ] **Step 3: Implement**

Prepend to `queue.rs`:
```rust
//! Play order and cursor. Pure data, no I/O.

use crate::track::TrackId;

/// The ordered list of tracks that next/previous move through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Queue {
    order: Vec<TrackId>,
    current: Option<usize>,
}

impl Queue {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the order. The current track is kept if it is still present.
    pub fn set_order(&mut self, order: Vec<TrackId>) {
        let current_id = self.current();
        self.order = order;
        self.current = current_id.and_then(|id| self.position_of(id));
    }

    /// The current order.
    pub fn order(&self) -> &[TrackId] {
        &self.order
    }

    /// The track the cursor is on.
    pub fn current(&self) -> Option<TrackId> {
        self.current.and_then(|i| self.order.get(i).copied())
    }

    /// Move the cursor to `id`. Returns `false` if it is not in the queue.
    pub fn jump_to(&mut self, id: TrackId) -> bool {
        match self.position_of(id) {
            Some(i) => {
                self.current = Some(i);
                true
            }
            None => false,
        }
    }

    /// Advance, wrapping at the end. Starts at the front when there is no cursor.
    pub fn next(&mut self) -> Option<TrackId> {
        if self.order.is_empty() {
            return None;
        }
        self.current = Some(self.current.map_or(0, |i| (i + 1) % self.order.len()));
        self.current()
    }

    /// Step back, wrapping at the front. Starts at the end when there is no cursor.
    pub fn prev(&mut self) -> Option<TrackId> {
        if self.order.is_empty() {
            return None;
        }
        let len = self.order.len();
        self.current = Some(self.current.map_or(len - 1, |i| (i + len - 1) % len));
        self.current()
    }

    fn position_of(&self, id: TrackId) -> Option<usize> {
        self.order.iter().position(|t| *t == id)
    }
}
```

- [ ] **Step 4: Run tests, lint, commit**

Run: `cargo test -p tunes-core 2>&1 | tail -5` → `14 passed`.
Run: `cargo fmt --all && cargo clippy --workspace --all-targets 2>&1 | tail -5`

```bash
git add -A
git commit -m "Add Queue with wrapping next/prev and order replacement

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Player on a dedicated audio thread

**Files:**
- Create: `crates/tunes-core/src/player.rs`
- Modify: `crates/tunes-core/src/lib.rs`

**Interfaces:**
- Produces:
  - `PlaybackState { Stopped, Playing, Paused }`
  - `PlaybackStatus { state, path: Option<PathBuf>, position: Duration, duration: Option<Duration>, volume: f32, last_error: Option<String> }` (Default = Stopped, volume 1.0)
  - `Event { TrackEnded, Error(String) }`
  - `PlayerError::Output(String)`
  - `Player::new() -> Result<Player, PlayerError>`; methods `play(&Path)`, `pause()`, `resume()`, `toggle()`, `seek(Duration)`, `set_volume(f32)`, `stop()`, `status() -> PlaybackStatus`, `try_recv_event() -> Option<Event>`. `Player: Clone + Send`.

- [ ] **Step 1: Write the failing smoke test**

`crates/tunes-core/src/player.rs` test module:
```rust
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/alpha.m4a")
    }

    #[test]
    fn status_defaults_to_stopped() {
        let status = PlaybackStatus::default();
        assert_eq!(status.state, PlaybackState::Stopped);
        assert!((status.volume - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn plays_fixture_to_the_end_when_a_device_exists() {
        let Ok(player) = Player::new() else {
            eprintln!("no audio device; skipping playback smoke test");
            return;
        };
        player.set_volume(0.0);
        player.play(&fixture());

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut ended = false;
        while Instant::now() < deadline {
            if let Some(Event::TrackEnded) = player.try_recv_event() {
                ended = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(ended, "TrackEnded was not reported within 5s");
        let status = player.status();
        assert_eq!(status.state, PlaybackState::Stopped);
        assert!(status.duration.is_some_and(|d| d >= Duration::from_millis(800)));
    }

    #[test]
    fn missing_file_reports_error_event() {
        let Ok(player) = Player::new() else {
            return;
        };
        player.play(Path::new("/nonexistent/file.mp3"));
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut got_error = false;
        while Instant::now() < deadline {
            if let Some(Event::Error(_)) = player.try_recv_event() {
                got_error = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(got_error);
        assert!(player.status().last_error.is_some());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Add `pub mod player;` and `pub use player::{Event, PlaybackState, PlaybackStatus, Player, PlayerError};` to `lib.rs`.

Run: `cargo test -p tunes-core player 2>&1 | tail -5`
Expected: compile error.

- [ ] **Step 3: Implement**

Prepend to `player.rs`:
```rust
//! Audio playback on a dedicated thread, controlled over a channel.
//!
//! rodio's output stream is created and owned by the audio thread. The
//! [`Player`] handle only sends commands and reads a shared status snapshot.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};
use rodio::stream::DeviceSinkBuilder;
use rodio::{Decoder, Source};

/// Whether audio is flowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
    /// Nothing loaded, or the track finished.
    #[default]
    Stopped,
    /// Audio is playing.
    Playing,
    /// A track is loaded but paused.
    Paused,
}

/// Snapshot of the audio thread's state.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackStatus {
    /// Current playback state.
    pub state: PlaybackState,
    /// Path of the loaded track, if any.
    pub path: Option<PathBuf>,
    /// Elapsed time in the current track.
    pub position: Duration,
    /// Total length of the current track, when the decoder knows it.
    pub duration: Option<Duration>,
    /// Output volume, 0.0 to 1.0.
    pub volume: f32,
    /// Most recent error message, cleared on the next successful play.
    pub last_error: Option<String>,
}

impl Default for PlaybackStatus {
    fn default() -> Self {
        Self {
            state: PlaybackState::Stopped,
            path: None,
            position: Duration::ZERO,
            duration: None,
            volume: 1.0,
            last_error: None,
        }
    }
}

/// Notifications from the audio thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The current track played to its end.
    TrackEnded,
    /// Loading, decoding or seeking failed.
    Error(String),
}

/// Why a [`Player`] could not be created.
#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    /// The default output device could not be opened.
    #[error("could not open audio output: {0}")]
    Output(String),
}

#[derive(Debug)]
enum Command {
    Play(PathBuf),
    Pause,
    Resume,
    Toggle,
    Seek(Duration),
    SetVolume(f32),
    Stop,
}

/// Cloneable handle to the audio thread.
#[derive(Clone)]
pub struct Player {
    commands: Sender<Command>,
    events: Receiver<Event>,
    status: Arc<Mutex<PlaybackStatus>>,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player").field("status", &self.status()).finish()
    }
}

impl Player {
    /// Open the default output device on a new audio thread.
    pub fn new() -> Result<Self, PlayerError> {
        let (commands, command_rx) = unbounded();
        let (event_tx, events) = unbounded();
        let (ready_tx, ready_rx) = bounded(1);
        let status = Arc::new(Mutex::new(PlaybackStatus::default()));
        let thread_status = Arc::clone(&status);

        std::thread::Builder::new()
            .name("tunes-audio".into())
            .spawn(move || audio_thread(&command_rx, &event_tx, &thread_status, &ready_tx))
            .map_err(|err| PlayerError::Output(err.to_string()))?;

        ready_rx
            .recv()
            .map_err(|_| PlayerError::Output("audio thread exited early".into()))??;

        Ok(Self { commands, events, status })
    }

    /// Load and start playing `path`, replacing whatever was playing.
    pub fn play(&self, path: &Path) {
        self.send(Command::Play(path.to_path_buf()));
    }

    /// Pause if playing.
    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    /// Resume if paused.
    pub fn resume(&self) {
        self.send(Command::Resume);
    }

    /// Pause if playing, resume if paused.
    pub fn toggle(&self) {
        self.send(Command::Toggle);
    }

    /// Seek within the current track.
    pub fn seek(&self, position: Duration) {
        self.send(Command::Seek(position));
    }

    /// Set the output volume, clamped to 0.0..=1.0.
    pub fn set_volume(&self, volume: f32) {
        self.send(Command::SetVolume(volume.clamp(0.0, 1.0)));
    }

    /// Stop and unload the current track.
    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    /// Latest status snapshot.
    pub fn status(&self) -> PlaybackStatus {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Pop the next pending event without blocking.
    pub fn try_recv_event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            tracing::error!("audio thread is gone");
        }
    }
}

type Loaded = (Decoder<BufReader<File>>, Option<Duration>);

fn open_source(path: &Path) -> Result<Loaded, String> {
    let describe = |err: &dyn std::fmt::Display| format!("{}: {err}", path.display());
    let file = File::open(path).map_err(|e| describe(&e))?;
    let byte_len = file.metadata().ok().map(|m| m.len());
    let mut builder = Decoder::builder().with_data(BufReader::new(file)).with_seekable(true);
    if let Some(len) = byte_len {
        builder = builder.with_byte_len(len);
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        builder = builder.with_hint(ext);
    }
    let decoder = builder.build().map_err(|e| describe(&e))?;
    let duration = decoder.total_duration();
    Ok((decoder, duration))
}

fn audio_thread(
    commands: &Receiver<Command>,
    events: &Sender<Event>,
    status: &Mutex<PlaybackStatus>,
    ready: &Sender<Result<(), PlayerError>>,
) {
    let sink = match DeviceSinkBuilder::open_default_sink() {
        Ok(sink) => sink,
        Err(err) => {
            let _ = ready.send(Err(PlayerError::Output(err.to_string())));
            return;
        }
    };
    let player = rodio::Player::connect_new(sink.mixer());
    let _ = ready.send(Ok(()));

    let update = |f: &dyn Fn(&mut PlaybackStatus)| {
        f(&mut status.lock().unwrap_or_else(PoisonError::into_inner));
    };
    let mut state = PlaybackState::Stopped;

    loop {
        match commands.recv_timeout(Duration::from_millis(100)) {
            Ok(Command::Play(path)) => {
                player.clear();
                match open_source(&path) {
                    Ok((source, duration)) => {
                        player.append(source);
                        player.play();
                        state = PlaybackState::Playing;
                        update(&|s| {
                            s.path = Some(path.clone());
                            s.duration = duration;
                            s.position = Duration::ZERO;
                            s.last_error = None;
                        });
                    }
                    Err(message) => {
                        state = PlaybackState::Stopped;
                        tracing::warn!(%message, "could not play");
                        update(&|s| {
                            s.path = None;
                            s.duration = None;
                            s.last_error = Some(message.clone());
                        });
                        let _ = events.send(Event::Error(message));
                    }
                }
            }
            Ok(Command::Pause) => {
                if state == PlaybackState::Playing {
                    player.pause();
                    state = PlaybackState::Paused;
                }
            }
            Ok(Command::Resume) => {
                if state == PlaybackState::Paused {
                    player.play();
                    state = PlaybackState::Playing;
                }
            }
            Ok(Command::Toggle) => match state {
                PlaybackState::Playing => {
                    player.pause();
                    state = PlaybackState::Paused;
                }
                PlaybackState::Paused => {
                    player.play();
                    state = PlaybackState::Playing;
                }
                PlaybackState::Stopped => {}
            },
            Ok(Command::Seek(position)) => {
                if let Err(err) = player.try_seek(position) {
                    let _ = events.send(Event::Error(format!("seek failed: {err}")));
                }
            }
            Ok(Command::SetVolume(volume)) => player.set_volume(volume),
            Ok(Command::Stop) => {
                player.clear();
                state = PlaybackState::Stopped;
                update(&|s| {
                    s.path = None;
                    s.duration = None;
                    s.position = Duration::ZERO;
                });
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if state == PlaybackState::Playing && player.empty() {
            state = PlaybackState::Stopped;
            let _ = events.send(Event::TrackEnded);
        }

        let position = player.get_pos();
        let volume = player.volume();
        update(&|s| {
            s.state = state;
            s.position = position;
            s.volume = volume;
        });
    }
}
```

If `rodio::stream::DeviceSinkBuilder::open_default_sink` does not resolve, grep `pub fn open_default_sink` in the vendored `rodio-0.22.2/src/stream.rs` and use the path it is exported under (`rodio::DeviceSinkBuilder` is also re-exported at the crate root).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p tunes-core player 2>&1 | tail -15`
Expected: `3 passed`. On this machine an audio device exists, so the smoke test must actually run and pass; if it prints "no audio device", investigate `open_default_sink` rather than accepting the skip.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets 2>&1 | tail -5`

```bash
git add -A
git commit -m "Add rodio Player on a dedicated audio thread with status and events

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: UI foundations: format, art, section, AppView with scan and empty states

**Files:**
- Create: `crates/gpui-tunes/src/ui/mod.rs`, `ui/format.rs`, `ui/art.rs`, `ui/section.rs`, `ui/app.rs`
- Modify: `crates/gpui-tunes/src/main.rs`

**Interfaces:**
- Consumes: `tunes_core::{scan, ScanError, Library, Player}`.
- Produces: `fmt_duration(Duration) -> String`; `art::decode(&[u8]) -> Arc<Image>`; `Section { Songs, Artists, Albums, Genres }` with `label()`, `ALL`; `AppView::new(root: PathBuf, window, cx)`; `Phase { Scanning, Missing(PathBuf), Ready(Arc<Library>) }` is private to app.rs.

- [ ] **Step 1: Write format tests and implementation**

`crates/gpui-tunes/src/ui/format.rs`:
```rust
//! Text formatting helpers for the views.

use std::time::Duration;

/// Format as `m:ss`, or `h:mm:ss` from one hour.
pub fn fmt_duration(d: Duration) -> String {
    let total = d.as_secs();
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_minutes_and_seconds() {
        assert_eq!(fmt_duration(Duration::from_secs(0)), "0:00");
        assert_eq!(fmt_duration(Duration::from_secs(65)), "1:05");
        assert_eq!(fmt_duration(Duration::from_millis(185_900)), "3:05");
    }

    #[test]
    fn formats_hours() {
        assert_eq!(fmt_duration(Duration::from_secs(3_600)), "1:00:00");
        assert_eq!(fmt_duration(Duration::from_secs(3_661)), "1:01:01");
    }
}
```

- [ ] **Step 2: Write art.rs and section.rs**

`crates/gpui-tunes/src/ui/art.rs`:
```rust
//! Turn embedded picture bytes into a gpui image.

use std::sync::Arc;

use gpui_kit::{Image, ImageFormat};

/// Wrap encoded picture bytes for `img()`. Format is sniffed from magic bytes.
pub fn decode(bytes: &[u8]) -> Arc<Image> {
    let format = match bytes {
        [0x89, b'P', b'N', b'G', ..] => ImageFormat::Png,
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => ImageFormat::Webp,
        [b'G', b'I', b'F', ..] => ImageFormat::Gif,
        [b'B', b'M', ..] => ImageFormat::Bmp,
        _ => ImageFormat::Jpeg,
    };
    Arc::new(Image::from_bytes(format, bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_png_and_defaults_to_jpeg() {
        assert!(matches!(decode(b"\x89PNG\r\n").format(), ImageFormat::Png));
        assert!(matches!(decode(b"\xFF\xD8\xFF").format(), ImageFormat::Jpeg));
    }
}
```
If `Image::format()` is not a public accessor, delete that test and keep the function.

`crates/gpui-tunes/src/ui/section.rs`:
```rust
//! The sidebar's library sections.

/// Which column-browser arrangement is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Genres, Artists and Albums panes over the song table.
    Songs,
    /// Artists and Albums panes.
    Artists,
    /// Albums pane only.
    Albums,
    /// Genres, Artists and Albums panes.
    Genres,
}

impl Section {
    /// Sidebar order.
    pub const ALL: [Section; 4] = [Section::Songs, Section::Artists, Section::Albums, Section::Genres];

    /// Sidebar label.
    pub fn label(self) -> &'static str {
        match self {
            Section::Songs => "Songs",
            Section::Artists => "Artists",
            Section::Albums => "Albums",
            Section::Genres => "Genres",
        }
    }

    /// Whether the Genres pane is visible in this section.
    pub fn shows_genres(self) -> bool {
        matches!(self, Section::Songs | Section::Genres)
    }

    /// Whether the Artists pane is visible in this section.
    pub fn shows_artists(self) -> bool {
        !matches!(self, Section::Albums)
    }
}
```

- [ ] **Step 3: Write AppView with scanning, missing and ready phases**

`crates/gpui-tunes/src/ui/mod.rs`:
```rust
//! Views. Everything here renders from plain data and talks to `tunes_core`.

pub mod app;
pub mod art;
pub mod format;
pub mod section;
```

`crates/gpui-tunes/src/ui/app.rs`:
```rust
//! Root view: owns the core objects and wires the child views together.

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;
use tunes_core::{Library, Player, PlayerError, ScanError, scan};

enum Phase {
    Scanning,
    Missing(PathBuf),
    Ready(Arc<Library>),
}

/// The application window's root view.
pub struct AppView {
    root: PathBuf,
    player: Result<Player, PlayerError>,
    phase: Phase,
    _tasks: Vec<Task<()>>,
}

impl AppView {
    /// Build the view and kick off the background scan of `root`.
    pub fn new(root: PathBuf, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let player = Player::new();
        if let Err(err) = &player {
            tracing::error!(%err, "audio output unavailable");
        }

        let scan_root = root.clone();
        let scan_task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { scan(&scan_root) })
                .await;
            this.update(cx, |this, cx| {
                this.phase = match result {
                    Ok(tracks) => Phase::Ready(Arc::new(Library::new(tracks))),
                    Err(ScanError::RootMissing(path)) => Phase::Missing(path),
                };
                cx.notify();
            })
            .ok();
        });

        Self { root, player, phase: Phase::Scanning, _tasks: vec![scan_task] }
    }

    fn render_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        match &self.phase {
            Phase::Scanning => h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .text_color(muted)
                .child(Spinner::new())
                .child(format!("Scanning {}", self.root.display()))
                .into_any_element(),
            Phase::Missing(path) => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(muted)
                .child("No library folder found")
                .child(format!("Looked in {}", path.display()))
                .child("Set GPUI_TUNES_LIBRARY to point at your music")
                .into_any_element(),
            Phase::Ready(library) => div()
                .p_4()
                .child(format!("{} tracks", library.len()))
                .into_any_element(),
        }
    }
}

impl Render for AppView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.render_content(cx);
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(content)
    }
}
```

Replace `main.rs` with:
```rust
//! gpui-tunes binary entry point.

mod ui;

use std::path::PathBuf;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

use crate::ui::app::AppView;

fn library_root() -> PathBuf {
    std::env::var_os("GPUI_TUNES_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("music/library"))
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let root = library_root();

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);

        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("gpui-tunes".into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1200.), px(760.)),
                cx,
            ))),
            ..Default::default()
        };

        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| AppView::new(root.clone(), window, cx))
        })
        .expect("failed to open window");
    });
}
```

- [ ] **Step 4: Test, run, verify both states**

Run: `cargo test -p gpui-tunes 2>&1 | tail -8` → format and art tests pass.

Run: `timeout 10 cargo run 2>&1 | grep -E "library scanned|error" ` → expect a `library scanned count=4xx` log line and the window briefly showing "4xx tracks".

Run: `GPUI_TUNES_LIBRARY=/nope timeout 6 cargo run` → window shows the "No library folder found" state (observe it; the process is killed by timeout).

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets 2>&1 | tail -5`

```bash
git add -A
git commit -m "Add AppView with background scan, spinner and empty states

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Sidebar and column browser

**Files:**
- Create: `crates/gpui-tunes/src/ui/sidebar.rs`, `crates/gpui-tunes/src/ui/browser.rs`
- Modify: `crates/gpui-tunes/src/ui/mod.rs`, `crates/gpui-tunes/src/ui/app.rs`

**Interfaces:**
- Consumes: `Section`, `Library::{genres, artists_in, albums_in}`, `Filter`.
- Produces:
  - `LibrarySidebar::new(cx)`, emits `SidebarEvent::Select(Section)`.
  - `ColumnBrowser::new(window, cx)`, `set_library(Option<Arc<Library>>, window, cx)`, `set_section(Section, window, cx)`, `filter() -> Filter` (browser part only, search empty), emits `BrowserEvent::FilterChanged`.

- [ ] **Step 1: Write sidebar.rs**

```rust
//! Left sidebar listing the library sections.

use gpui_kit::component::sidebar::{Sidebar, SidebarGroup, SidebarMenu, SidebarMenuItem};
use gpui_kit::component::{ActiveTheme, Icon};
use gpui_kit::*;

use crate::ui::section::Section;

/// Emitted when the user picks a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarEvent {
    /// A section was clicked.
    Select(Section),
}

/// The sidebar view.
pub struct LibrarySidebar {
    active: Section,
}

impl EventEmitter<SidebarEvent> for LibrarySidebar {}

impl LibrarySidebar {
    /// Sidebar with Songs selected.
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self { active: Section::Songs }
    }

    fn icon(section: Section) -> gpui_kit::assets::IconName {
        use gpui_kit::assets::IconName;
        match section {
            Section::Songs => IconName::Music,
            Section::Artists => IconName::User,
            Section::Albums => IconName::Disc,
            Section::Genres => IconName::ListMusic,
        }
    }
}

impl Render for LibrarySidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = Section::ALL.map(|section| {
            SidebarMenuItem::new(section.label())
                .icon(Icon::new(Self::icon(section)))
                .active(self.active == section)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.active = section;
                    cx.emit(SidebarEvent::Select(section));
                    cx.notify();
                }))
        });

        Sidebar::new("library-sidebar")
            .w_full()
            .h_full()
            .bg(cx.theme().sidebar)
            .child(SidebarGroup::new("Library").child(SidebarMenu::new().children(items)))
    }
}
```

- [ ] **Step 2: Write browser.rs**

```rust
//! Classic column browser: Genres | Artists | Albums, each with an "All" row.

use std::sync::Arc;

use gpui_kit::component::list::{List, ListDelegate, ListEvent, ListItem, ListState};
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::{ActiveTheme, IndexPath, Selectable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use tunes_core::{Filter, Library};

use crate::ui::section::Section;

/// Emitted whenever a pane selection changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserEvent {
    /// The genre/artist/album narrowing changed.
    FilterChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Genres,
    Artists,
    Albums,
}

impl Pane {
    fn title(self) -> &'static str {
        match self {
            Pane::Genres => "Genres",
            Pane::Artists => "Artists",
            Pane::Albums => "Albums",
        }
    }
}

/// One row in a pane.
#[derive(IntoElement)]
struct BrowserRow {
    base: ListItem,
    label: SharedString,
    selected: bool,
}

impl Selectable for BrowserRow {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self.base = self.base.selected(selected);
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for BrowserRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = if self.selected { cx.theme().accent_foreground } else { cx.theme().foreground };
        self.base.px_2().py_0p5().child(
            div()
                .text_sm()
                .text_color(color)
                .whitespace_nowrap()
                .overflow_hidden()
                .child(self.label),
        )
    }
}

/// Items of one pane. Row 0 is always "All (n)".
struct PaneDelegate {
    values: Vec<String>,
    selected: Option<usize>,
}

impl PaneDelegate {
    fn new() -> Self {
        Self { values: Vec::new(), selected: Some(0) }
    }

    /// The chosen value, or `None` when "All" is selected.
    fn chosen(&self) -> Option<&str> {
        match self.selected {
            Some(0) | None => None,
            Some(i) => self.values.get(i - 1).map(String::as_str),
        }
    }
}

impl ListDelegate for PaneDelegate {
    type Item = BrowserRow;

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.values.len() + 1
    }

    fn render_item(&mut self, ix: IndexPath, _: &mut Window, _: &mut Context<ListState<Self>>) -> Option<Self::Item> {
        let label: SharedString = if ix.row == 0 {
            format!("All ({})", self.values.len()).into()
        } else {
            self.values.get(ix.row - 1)?.clone().into()
        };
        let selected = self.selected == Some(ix.row);
        Some(BrowserRow { base: ListItem::new(ix).selected(selected), label, selected })
    }

    fn set_selected_index(&mut self, ix: Option<IndexPath>, _: &mut Window, cx: &mut Context<ListState<Self>>) {
        self.selected = ix.map(|ix| ix.row);
        cx.notify();
    }
}

/// The three-pane browser view.
pub struct ColumnBrowser {
    library: Option<Arc<Library>>,
    section: Section,
    genres: Entity<ListState<PaneDelegate>>,
    artists: Entity<ListState<PaneDelegate>>,
    albums: Entity<ListState<PaneDelegate>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<BrowserEvent> for ColumnBrowser {}

impl ColumnBrowser {
    /// Empty browser; call [`Self::set_library`] once the scan finishes.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let make = |cx: &mut Context<Self>| {
            cx.new(|cx| ListState::new(PaneDelegate::new(), window, cx))
        };
        let genres = make(cx);
        let artists = make(cx);
        let albums = make(cx);

        let subscriptions = vec![
            cx.subscribe_in(&genres, window, |this, _, ev, window, cx| {
                if matches!(ev, ListEvent::Select(_) | ListEvent::Confirm(_)) {
                    this.on_pane_changed(Pane::Genres, window, cx);
                }
            }),
            cx.subscribe_in(&artists, window, |this, _, ev, window, cx| {
                if matches!(ev, ListEvent::Select(_) | ListEvent::Confirm(_)) {
                    this.on_pane_changed(Pane::Artists, window, cx);
                }
            }),
            cx.subscribe_in(&albums, window, |this, _, ev, window, cx| {
                if matches!(ev, ListEvent::Select(_) | ListEvent::Confirm(_)) {
                    this.on_pane_changed(Pane::Albums, window, cx);
                }
            }),
        ];

        Self { library: None, section: Section::Songs, genres, artists, albums, _subscriptions: subscriptions }
    }

    /// Install (or clear) the library and reset every pane to "All".
    pub fn set_library(&mut self, library: Option<Arc<Library>>, window: &mut Window, cx: &mut Context<Self>) {
        self.library = library;
        self.reset_from(Pane::Genres, window, cx);
    }

    /// Switch section. Clears pane selections.
    pub fn set_section(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        self.section = section;
        self.reset_from(Pane::Genres, window, cx);
    }

    /// The narrowing the panes currently express (search is left empty).
    pub fn filter(&self, cx: &App) -> Filter {
        let pick = |list: &Entity<ListState<PaneDelegate>>| list.read(cx).delegate().chosen().map(str::to_owned);
        Filter {
            genre: if self.section.shows_genres() { pick(&self.genres) } else { None },
            artist: if self.section.shows_artists() { pick(&self.artists) } else { None },
            album: pick(&self.albums),
            search: String::new(),
        }
    }

    fn on_pane_changed(&mut self, pane: Pane, window: &mut Window, cx: &mut Context<Self>) {
        match pane {
            Pane::Genres => self.reset_from(Pane::Artists, window, cx),
            Pane::Artists => self.reset_from(Pane::Albums, window, cx),
            Pane::Albums => {}
        }
        cx.emit(BrowserEvent::FilterChanged);
        cx.notify();
    }

    /// Recompute the values of `from` and every pane to its right, selecting "All".
    fn reset_from(&mut self, from: Pane, window: &mut Window, cx: &mut Context<Self>) {
        let Some(library) = self.library.clone() else {
            for list in [&self.genres, &self.artists, &self.albums] {
                Self::fill(list, Vec::new(), window, cx);
            }
            return;
        };

        if from == Pane::Genres {
            Self::fill(&self.genres, library.genres().to_vec(), window, cx);
        }
        let genre = self.section.shows_genres().then(|| self.genres.read(cx).delegate().chosen().map(str::to_owned)).flatten();
        if matches!(from, Pane::Genres | Pane::Artists) {
            Self::fill(&self.artists, library.artists_in(genre.as_deref()), window, cx);
        }
        let artist = self.section.shows_artists().then(|| self.artists.read(cx).delegate().chosen().map(str::to_owned)).flatten();
        Self::fill(&self.albums, library.albums_in(genre.as_deref(), artist.as_deref()), window, cx);
    }

    fn fill(list: &Entity<ListState<PaneDelegate>>, values: Vec<String>, window: &mut Window, cx: &mut App) {
        list.update(cx, |state, cx| {
            state.delegate_mut().values = values;
            state.delegate_mut().selected = Some(0);
            state.set_selected_index(Some(IndexPath::new(0)), window, cx);
            cx.notify();
        });
    }

    fn pane(&self, pane: Pane, list: &Entity<ListState<PaneDelegate>>, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .min_w_0()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().muted_foreground)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(pane.title()),
            )
            .child(List::new(list).flex_1())
    }
}

impl Render for ColumnBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let section = self.section;
        h_flex()
            .size_full()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_resizable("column-browser")
                    .when(section.shows_genres(), |group| {
                        group.child(resizable_panel().child(self.pane(Pane::Genres, &self.genres, cx)))
                    })
                    .when(section.shows_artists(), |group| {
                        group.child(resizable_panel().child(self.pane(Pane::Artists, &self.artists, cx)))
                    })
                    .child(resizable_panel().child(self.pane(Pane::Albums, &self.albums, cx))),
            )
    }
}
```

Notes for the implementer:
- `set_selected_index` on `ListState` exists (`crates/gpui-component-0.7.0/src/list/list.rs:184`). If its signature differs, match it.
- `ListEvent::Select` fires on single click. If clicking does not fire `Select` on this build, also handle `Confirm` (already matched).
- If `h_resizable(...).when(...)` fails because `ResizablePanelGroup` lacks `FluentBuilder`, build the group in a `let mut group = ...; if ... { group = group.child(...) }` sequence instead.

- [ ] **Step 3: Wire sidebar and browser into AppView**

Add `pub mod browser; pub mod sidebar;` to `ui/mod.rs`. In `app.rs`:

Add fields and imports:
```rust
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use tunes_core::Filter;
use crate::ui::browser::{BrowserEvent, ColumnBrowser};
use crate::ui::section::Section;
use crate::ui::sidebar::{LibrarySidebar, SidebarEvent};
```
```rust
pub struct AppView {
    root: PathBuf,
    player: Result<Player, PlayerError>,
    phase: Phase,
    filter: Filter,
    sidebar: Entity<LibrarySidebar>,
    browser: Entity<ColumnBrowser>,
    _subscriptions: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}
```
In `new`, before building `Self` (and change the `_window` parameter name to `window`):
```rust
let sidebar = cx.new(LibrarySidebar::new);
let browser = cx.new(|cx| ColumnBrowser::new(window, cx));
let subscriptions = vec![
    cx.subscribe_in(&sidebar, window, |this, _, ev: &SidebarEvent, window, cx| {
        let SidebarEvent::Select(section) = *ev;
        this.browser.update(cx, |b, cx| b.set_section(section, window, cx));
        this.refresh_filter(window, cx);
    }),
    cx.subscribe_in(&browser, window, |this, _, ev: &BrowserEvent, window, cx| {
        let BrowserEvent::FilterChanged = ev;
        this.refresh_filter(window, cx);
    }),
];
```
In the scan task's `this.update` closure, after setting the phase, use `this.update_in` instead of `this.update` so a window is available, and add:
```rust
let library = match &this.phase { Phase::Ready(l) => Some(Arc::clone(l)), _ => None };
this.browser.update(cx, |b, cx| b.set_library(library, window, cx));
this.refresh_filter(window, cx);
```
(`cx.spawn` must become `cx.spawn_in(window, ...)` for `update_in` to be available.)

Add the method:
```rust
fn refresh_filter(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
    let search = std::mem::take(&mut self.filter.search);
    self.filter = self.browser.read(cx).filter(cx);
    self.filter.search = search;
    cx.notify();
}
```
Replace the `Phase::Ready` arm of `render_content` with a layout that shows the count of matching tracks for now:
```rust
Phase::Ready(library) => {
    let count = library.query(&self.filter, tunes_core::Sort::default()).len();
    h_resizable("main")
        .child(
            resizable_panel()
                .size(px(180.))
                .size_range(px(140.)..px(320.))
                .child(self.sidebar.clone()),
        )
        .child(
            resizable_panel().child(
                v_resizable("content")
                    .child(
                        resizable_panel()
                            .size(px(200.))
                            .size_range(px(120.)..px(400.))
                            .child(self.browser.clone()),
                    )
                    .child(resizable_panel().child(div().p_4().child(format!("{count} tracks match")))),
            ),
        )
        .into_any_element()
}
```

- [ ] **Step 4: Run and verify**

Run: `cargo run 2>&1 | tail -3` (leave it running; open the window). Verify:
- Sidebar shows Songs/Artists/Albums/Genres with icons, Songs highlighted.
- Three panes appear with "All (n)" at the top; clicking a genre narrows Artists and Albums and the count under the panes changes.
- Clicking Albums in the sidebar leaves only the Albums pane.
Then close the window.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets 2>&1 | tail -5`

```bash
git add -A
git commit -m "Add sidebar sections and the three-pane column browser

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Song table

**Files:**
- Create: `crates/gpui-tunes/src/ui/song_table.rs`
- Modify: `crates/gpui-tunes/src/ui/mod.rs`, `crates/gpui-tunes/src/ui/app.rs`

**Interfaces:**
- Consumes: `Library::{track, sort_ids}`, `Sort`, `SortColumn`, `fmt_duration`.
- Produces: `SongTableDelegate::new()`, fields `library: Option<Arc<Library>>`, `rows: Vec<TrackId>`, `now_playing: Option<TrackId>`, `sort: Sort`; method `track_at(usize) -> Option<TrackId>`. AppView gains `table: Entity<TableState<SongTableDelegate>>`, `queue: Queue`, `apply_query(window, cx)`.

- [ ] **Step 1: Write song_table.rs**

```rust
//! Table delegate for the song list.

use std::sync::Arc;

use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme, Icon, h_flex};
use gpui_kit::*;
use tunes_core::{Library, Sort, SortColumn, TrackId};

use crate::ui::format::fmt_duration;

const COLUMNS: [(&str, &str, f32, SortColumn); 6] = [
    ("title", "Name", 320., SortColumn::Title),
    ("time", "Time", 70., SortColumn::Duration),
    ("artist", "Artist", 200., SortColumn::Artist),
    ("album", "Album", 220., SortColumn::Album),
    ("genre", "Genre", 140., SortColumn::Genre),
    ("track", "#", 50., SortColumn::TrackNo),
];

/// Rows are track ids; cells are looked up in the shared library.
pub struct SongTableDelegate {
    /// Library the ids refer to. `None` until the scan finishes.
    pub library: Option<Arc<Library>>,
    /// Visible rows in display order.
    pub rows: Vec<TrackId>,
    /// Track to mark with a play glyph.
    pub now_playing: Option<TrackId>,
    /// Sort currently applied to `rows`.
    pub sort: Sort,
    columns: Vec<Column>,
}

impl SongTableDelegate {
    /// Empty delegate with the fixed column set.
    pub fn new() -> Self {
        let columns = COLUMNS
            .iter()
            .map(|(key, name, width, _)| {
                let col = Column::new(*key, *name).width(px(*width)).sortable();
                if *key == "time" || *key == "track" { col.text_right() } else { col }
            })
            .collect();
        Self { library: None, rows: Vec::new(), now_playing: None, sort: Sort::default(), columns }
    }

    /// Track id shown on row `ix`.
    pub fn track_at(&self, ix: usize) -> Option<TrackId> {
        self.rows.get(ix).copied()
    }

    fn cell(&self, row_ix: usize, col_ix: usize) -> SharedString {
        let Some((library, id)) = self.library.as_ref().zip(self.track_at(row_ix)) else {
            return SharedString::default();
        };
        let Some(track) = library.track(id) else {
            return SharedString::default();
        };
        match COLUMNS[col_ix].0 {
            "title" => track.title.clone().into(),
            "time" => fmt_duration(track.duration).into(),
            "artist" => track.artist.clone().into(),
            "album" => track.album.clone().into(),
            "genre" => track.genre.clone().into(),
            _ => track.track_no.map(|n| n.to_string()).unwrap_or_default().into(),
        }
    }
}

impl Default for SongTableDelegate {
    fn default() -> Self {
        Self::new()
    }
}

impl TableDelegate for SongTableDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn perform_sort(&mut self, col_ix: usize, sort: ColumnSort, _: &mut Window, cx: &mut Context<TableState<Self>>) {
        self.sort = match sort {
            ColumnSort::Default => Sort::default(),
            ColumnSort::Ascending => Sort { column: COLUMNS[col_ix].3, ascending: true },
            ColumnSort::Descending => Sort { column: COLUMNS[col_ix].3, ascending: false },
        };
        if let Some(library) = &self.library {
            library.sort_ids(&mut self.rows, self.sort);
        }
        cx.notify();
    }

    fn render_td(&mut self, row_ix: usize, col_ix: usize, _: &mut Window, cx: &mut Context<TableState<Self>>) -> impl IntoElement {
        let text = self.cell(row_ix, col_ix);
        let is_playing = col_ix == 0 && self.now_playing.is_some() && self.now_playing == self.track_at(row_ix);
        h_flex()
            .size_full()
            .items_center()
            .gap_1()
            .overflow_hidden()
            .when(is_playing, |el| {
                el.child(Icon::new(gpui_kit::assets::IconName::Play).text_color(cx.theme().primary))
            })
            .child(div().whitespace_nowrap().overflow_hidden().child(text))
    }
}
```
Add `use gpui_kit::prelude::FluentBuilder as _;` for `.when`. If `Icon::text_color` is not available, use `.color(cx.theme().primary)` or wrap the icon in `div().text_color(...)`.

- [ ] **Step 2: Wire into AppView**

Add `pub mod song_table;` to `ui/mod.rs`. In `app.rs` add:
```rust
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use tunes_core::{Queue, TrackId};
use crate::ui::song_table::SongTableDelegate;
```
Fields: `table: Entity<TableState<SongTableDelegate>>`, `queue: Queue`.

In `new`: `let table = cx.new(|cx| TableState::new(SongTableDelegate::new(), window, cx));` and subscribe:
```rust
cx.subscribe_in(&table, window, |this, _, ev: &TableEvent, window, cx| {
    if let TableEvent::DoubleClickedRow(row) = ev {
        let id = this.table.read(cx).delegate().track_at(*row);
        if let Some(id) = id {
            this.play_track(id, window, cx);
        }
    }
}),
```
Initialize `queue: Queue::new()`. In the scan completion closure, also set `this.table.update(cx, |t, _| t.delegate_mut().library = library.clone());` before `refresh_filter`.

Replace `refresh_filter` with:
```rust
fn refresh_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let search = std::mem::take(&mut self.filter.search);
    self.filter = self.browser.read(cx).filter(cx);
    self.filter.search = search;
    self.apply_query(window, cx);
}

/// Re-run the library query and push the rows into the table and queue.
fn apply_query(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
    let Phase::Ready(library) = &self.phase else { return };
    let sort = self.table.read(cx).delegate().sort;
    let ids = library.query(&self.filter, sort);
    self.queue.set_order(ids.clone());
    self.table.update(cx, |table, cx| {
        table.delegate_mut().rows = ids;
        table.refresh(cx);
    });
    cx.notify();
}

fn play_track(&mut self, id: TrackId, _window: &mut Window, cx: &mut Context<Self>) {
    let Phase::Ready(library) = &self.phase else { return };
    let Some(track) = library.track(id) else { return };
    if let Ok(player) = &self.player {
        player.play(&track.path);
    }
    self.queue.jump_to(id);
    self.table.update(cx, |table, cx| {
        table.delegate_mut().now_playing = Some(id);
        table.refresh(cx);
    });
    cx.notify();
}
```
Replace the placeholder "tracks match" panel with `resizable_panel().child(DataTable::new(&self.table).stripe(true))`.

- [ ] **Step 3: Run and verify**

Run: `cargo run`. Verify: table shows all tracks in album/track order; clicking the Name header sorts; narrowing with the browser updates rows; double-clicking a row starts audio playback (you will hear it) and shows the play glyph. Close the window.

- [ ] **Step 4: Lint and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets 2>&1 | tail -5`

```bash
git add -A
git commit -m "Add sortable song table wired to the library query and playback

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: Toolbar, transport, seek, volume, search, and the status poll loop

**Files:**
- Create: `crates/gpui-tunes/src/ui/toolbar.rs`
- Modify: `crates/gpui-tunes/src/ui/mod.rs`, `crates/gpui-tunes/src/ui/app.rs`

**Interfaces:**
- Consumes: `Player::{toggle, seek, set_volume, status, try_recv_event}`, `PlaybackStatus`, `PlaybackState`, `Event`, `Queue::{next, prev}`, `Library::cover_art`, `art::decode`, `fmt_duration`.
- Produces: `ToolbarModel`, `ToolbarEvent::{Prev, PlayPause, Next, Seek(Duration), Volume(f32), Search(String)}`, `Toolbar::new(window, cx)`, `Toolbar::set_model(ToolbarModel, window, cx)`.

- [ ] **Step 1: Write toolbar.rs**

```rust
//! Top bar: transport buttons, now-playing display with seek, volume, search.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{ActiveTheme, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use tunes_core::PlaybackState;

use crate::ui::format::fmt_duration;

/// What the toolbar shows. Built by `AppView` from core state.
#[derive(Debug, Clone, Default)]
pub struct ToolbarModel {
    /// Current track title, empty when nothing is loaded.
    pub title: String,
    /// Current track artist.
    pub artist: String,
    /// Elapsed time.
    pub position: Duration,
    /// Track length when known.
    pub duration: Option<Duration>,
    /// Playing, paused or stopped.
    pub state: PlaybackState,
    /// Cover art for the current track.
    pub art: Option<Arc<Image>>,
    /// `false` when no audio device is available.
    pub enabled: bool,
    /// Transient error or status line.
    pub notice: Option<String>,
}

/// User intents from the toolbar.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolbarEvent {
    /// Previous track.
    Prev,
    /// Toggle play/pause.
    PlayPause,
    /// Next track.
    Next,
    /// Seek to an absolute position.
    Seek(Duration),
    /// Volume 0.0..=1.0.
    Volume(f32),
    /// Search text changed.
    Search(String),
}

/// The toolbar view.
pub struct Toolbar {
    model: ToolbarModel,
    seek: Entity<SliderState>,
    volume: Entity<SliderState>,
    search: Entity<InputState>,
    seeking: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ToolbarEvent> for Toolbar {}

impl Toolbar {
    /// Toolbar with empty now-playing state.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let seek = cx.new(|_| SliderState::new().min(0.).max(1.).step(0.001).default_value(0.));
        let volume = cx.new(|_| SliderState::new().min(0.).max(1.).step(0.01).default_value(1.));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search").clean_on_escape());

        let subscriptions = vec![
            cx.subscribe(&seek, |this, _, ev: &SliderEvent, cx| match ev {
                SliderEvent::Change(_) => this.seeking = true,
                SliderEvent::Release(value) => {
                    this.seeking = false;
                    if let Some(duration) = this.model.duration {
                        cx.emit(ToolbarEvent::Seek(duration.mul_f32(value.start().clamp(0., 1.))));
                    }
                }
            }),
            cx.subscribe(&volume, |_, _, ev: &SliderEvent, cx| {
                if let SliderEvent::Change(value) | SliderEvent::Release(value) = ev {
                    cx.emit(ToolbarEvent::Volume(value.start()));
                }
            }),
            cx.subscribe(&search, |this, state, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    let text = state.read(cx).value().to_string();
                    let _ = &this;
                    cx.emit(ToolbarEvent::Search(text));
                }
            }),
        ];

        Self { model: ToolbarModel::default(), seek, volume, search, seeking: false, _subscriptions: subscriptions }
    }

    /// Push fresh state in. The seek slider is left alone while being dragged.
    pub fn set_model(&mut self, model: ToolbarModel, window: &mut Window, cx: &mut Context<Self>) {
        if !self.seeking {
            let fraction = match model.duration {
                Some(d) if !d.is_zero() => (model.position.as_secs_f32() / d.as_secs_f32()).clamp(0., 1.),
                _ => 0.,
            };
            self.seek.update(cx, |s, cx| s.set_value(fraction, window, cx));
        }
        self.model = model;
        cx.notify();
    }

    fn transport_button(&self, id: &'static str, icon: IconName, event: ToolbarEvent, cx: &mut Context<Self>) -> Button {
        Button::new(id)
            .ghost()
            .icon(Icon::new(icon))
            .disabled(!self.model.enabled)
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(event.clone())))
    }

    fn now_playing(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let has_track = !self.model.title.is_empty();
        let total = self.model.duration.map(fmt_duration).unwrap_or_else(|| "--:--".into());

        h_flex()
            .flex_1()
            .min_w_0()
            .h(px(52.))
            .gap_3()
            .px_3()
            .rounded(cx.theme().radius)
            .bg(cx.theme().secondary)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .size(px(40.))
                    .flex_shrink_0()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .overflow_hidden()
                    .when_some(self.model.art.clone(), |el, art| el.child(img(art).size_full())),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(if has_track { self.model.title.clone() } else { "Not Playing".into() }),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .text_xs()
                            .text_color(muted)
                            .child(fmt_duration(self.model.position))
                            .child(Slider::new(&self.seek).disabled(!has_track).flex_1())
                            .child(total),
                    )
                    .when_some(self.model.notice.clone(), |el, notice| {
                        el.child(div().text_xs().text_color(cx.theme().danger).child(notice))
                    }),
            )
            .when(has_track, |el| {
                el.child(div().text_xs().text_color(muted).whitespace_nowrap().child(self.model.artist.clone()))
            })
    }
}

impl Render for Toolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let play_icon = match self.model.state {
            PlaybackState::Playing => IconName::Pause,
            _ => IconName::Play,
        };

        h_flex()
            .w_full()
            .h(px(72.))
            .px_3()
            .gap_4()
            .items_center()
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(self.transport_button("prev", IconName::SkipBack, ToolbarEvent::Prev, cx))
                    .child(self.transport_button("play", play_icon, ToolbarEvent::PlayPause, cx))
                    .child(self.transport_button("next", IconName::SkipForward, ToolbarEvent::Next, cx)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .w(px(140.))
                    .child(Icon::new(IconName::Volume2).small())
                    .child(Slider::new(&self.volume).flex_1()),
            )
            .child(self.now_playing(cx))
            .child(div().w(px(220.)).child(Input::new(&self.search).small().cleanable(true)))
    }
}
```
Notes: `SliderValue::start()` returns the single value (see `gpui-base-0.7.0/src/slider.rs:91`). If `Icon::small()` is not from `Sizable`, drop it. If `.flex_1()` is not available on `Slider`, wrap in `div().flex_1().child(...)`.

- [ ] **Step 2: Wire the toolbar and poll loop into AppView**

Add `pub mod toolbar;` to `ui/mod.rs`. In `app.rs`:
```rust
use std::time::{Duration, Instant};
use tunes_core::{Event, PlaybackState, PlaybackStatus};
use crate::ui::art;
use crate::ui::toolbar::{Toolbar, ToolbarEvent, ToolbarModel};
```
Fields: `toolbar: Entity<Toolbar>`, `status: PlaybackStatus`, `now_playing: Option<TrackId>`, `art_cache: Option<(TrackId, Arc<Image>)>`, `notice: Option<(String, Instant)>`, `auto_skipped: bool`.

In `new`:
```rust
let toolbar = cx.new(|cx| Toolbar::new(window, cx));
```
subscription:
```rust
cx.subscribe_in(&toolbar, window, |this, _, ev: &ToolbarEvent, window, cx| match ev {
    ToolbarEvent::Prev => this.step(false, window, cx),
    ToolbarEvent::Next => this.step(true, window, cx),
    ToolbarEvent::PlayPause => this.play_pause(window, cx),
    ToolbarEvent::Seek(pos) => {
        if let Ok(p) = &this.player { p.seek(*pos); }
    }
    ToolbarEvent::Volume(v) => {
        if let Ok(p) = &this.player { p.set_volume(*v); }
    }
    ToolbarEvent::Search(text) => {
        this.filter.search = text.clone();
        this.apply_query(window, cx);
    }
}),
```
poll task (push onto `_tasks`):
```rust
let poll_task = cx.spawn_in(window, async move |this, cx| {
    loop {
        cx.background_executor().timer(Duration::from_millis(250)).await;
        if this.update_in(cx, |this, window, cx| this.tick(window, cx)).is_err() {
            break;
        }
    }
});
```
Methods:
```rust
fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let Ok(player) = &self.player else { return };
    self.status = player.status();
    while let Some(event) = player.try_recv_event() {
        match event {
            Event::TrackEnded => {
                self.auto_skipped = false;
                self.step(true, window, cx);
            }
            Event::Error(message) => {
                self.notice = Some((message, Instant::now()));
                if !self.auto_skipped {
                    self.auto_skipped = true;
                    self.step(true, window, cx);
                }
            }
        }
    }
    if self.notice.as_ref().is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(5)) {
        self.notice = None;
    }
    self.push_toolbar_model(window, cx);
}

fn step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
    let next = if forward { self.queue.next() } else { self.queue.prev() };
    if let Some(id) = next {
        self.play_track(id, window, cx);
    }
}

fn play_pause(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    match (self.status.state, self.now_playing) {
        (PlaybackState::Stopped, _) => {
            let id = self.queue.current().or_else(|| self.queue.next());
            if let Some(id) = id {
                self.play_track(id, window, cx);
            }
        }
        _ => {
            if let Ok(p) = &self.player { p.toggle(); }
        }
    }
}

fn push_toolbar_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let mut model = ToolbarModel {
        position: self.status.position,
        duration: self.status.duration,
        state: self.status.state,
        enabled: self.player.is_ok(),
        notice: self.notice.as_ref().map(|(m, _)| m.clone()),
        ..ToolbarModel::default()
    };
    if self.player.is_err() {
        model.notice = Some("No audio output".into());
    }
    if let (Phase::Ready(library), Some(id)) = (&self.phase, self.now_playing) {
        if let Some(track) = library.track(id) {
            model.title = track.title.clone();
            model.artist = format!("{} — {}", track.artist, track.album);
        }
        if self.art_cache.as_ref().is_none_or(|(cached, _)| *cached != id) {
            self.art_cache = library.cover_art(id).map(|bytes| (id, art::decode(&bytes)));
        }
        model.art = self.art_cache.as_ref().filter(|(cached, _)| *cached == id).map(|(_, img)| Arc::clone(img));
    }
    self.toolbar.update(cx, |t, cx| t.set_model(model, window, cx));
}
```
Update `play_track` to set `self.now_playing = Some(id);`, `self.auto_skipped = false;` for user-initiated plays (pass a flag or reset in the toolbar/table handlers; simplest: reset in `play_track` and set `auto_skipped = true` after calling `step` in the error branch).

Render: put `self.toolbar.clone()` as the first child of the root `v_flex`, above the content, in every phase.

- [ ] **Step 3: Run and verify end to end**

Run: `cargo run`. Verify:
- Toolbar shows transport, volume, "Not Playing", search box.
- Double-click a song: plays, title/artist/art appear, elapsed time advances, seek slider moves.
- Play/pause toggles icon and audio. Next/prev move through the visible order. A track ending advances automatically.
- Dragging seek jumps position. Volume slider changes loudness.
- Typing in search filters the table live; clearing restores.
- Selecting a genre in the browser while searching keeps the search text.
Close the window.

- [ ] **Step 4: Lint and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets 2>&1 | tail -5`

```bash
git add -A
git commit -m "Add toolbar with transport, seek, volume, search and status polling

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 10: README, final checks

**Files:**
- Create: `README.md`

- [ ] **Step 1: Write README.md**

```markdown
# gpui-tunes

A classic-iTunes-style player for a local audio library, built with
[GPUI Kit](https://github.com/longbridge/gpui-kit).

## Run

    cargo run --release

Scans `~/music/library` at startup. Override with:

    GPUI_TUNES_LIBRARY=/path/to/music cargo run --release

Supported formats: M4A/AAC, MP3, WAV, FLAC, OGG, OPUS.

## Layout

- `crates/tunes-core` — scanning (lofty), library queries, queue, playback (rodio). No UI dependency.
- `crates/gpui-tunes` — the GPUI views.

## Develop

    cargo test --workspace
    cargo clippy --workspace --all-targets
```

- [ ] **Step 2: Full verification**

Run: `cargo test --workspace 2>&1 | grep -E "test result|FAILED|panicked"`
Expected: every line `ok`, zero failed.

Run: `cargo clippy --workspace --all-targets 2>&1 | grep -c warning` → `0`.

Run: `grep -n gpui crates/tunes-core/Cargo.toml` → no output (boundary intact).

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "Add README

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

## Self-review notes

- Spec coverage: scan + per-file skip (T2), library lists/filter/sort/art (T3), queue wrap (T4), player commands/status/events/decode error (T5), dark theme + env var + spinner + missing-root state (T6), sidebar + browser narrowing + section clearing selections but keeping search (T7, T9), sortable table + play glyph + double-click (T8), transport/seek/volume/search/auto-advance/5 s notice/skip-once on decode error/"No audio output" (T9), README (T10).
- Names used across tasks: `Track`, `TrackId`, `scan`, `ScanError::RootMissing`, `Library::{new,len,track,genres,artists_in,albums_in,query,sort_ids,cover_art}`, `Filter`, `Sort`, `SortColumn`, `Queue::{new,set_order,current,jump_to,next,prev}`, `Player::{new,play,toggle,seek,set_volume,status,try_recv_event}`, `PlaybackStatus`, `PlaybackState`, `Event`, `fmt_duration`, `art::decode`, `Section`, `LibrarySidebar`/`SidebarEvent::Select`, `ColumnBrowser::{new,set_library,set_section,filter}`/`BrowserEvent::FilterChanged`, `SongTableDelegate::{new,track_at,rows,library,now_playing,sort}`, `Toolbar::{new,set_model}`/`ToolbarEvent`/`ToolbarModel`.
