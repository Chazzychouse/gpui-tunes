# gpui-tunes design

A desktop music player for a local audio library, in the style of classic iTunes,
built with GPUI and GPUI Kit (gpui-component). Linux first.

## Goals for v1

- Scan `~/music/library` (override with `GPUI_TUNES_LIBRARY`) and show every track.
- Browse by Songs, Artists, Albums, Genres using a classic column browser.
- Search across title, artist, album.
- Play, pause, previous, next, seek, volume. Advance automatically at end of track.
- Dark theme by default.

Out of scope for v1: playlists, queue editing, shuffle, repeat, lyrics, library
persistence, metadata editing. The design must not make these hard to add later.

## Library facts that shape the design

About 470 files in one flat folder: M4A (AAC) is the majority, then MP3, a few WAV.
Tags are complete (title, artist, album, genre, track/disc numbers, date, embedded
art, often lyrics). Folder structure carries no information; tags are the source of
truth. AAC decoding is mandatory.

## Architecture: two crates, one direction of dependency

Cargo workspace:

```
gpui-tunes/
  Cargo.toml            # workspace
  crates/tunes-core/    # library: scanning, library model, playback. NO gpui dependency.
  crates/gpui-tunes/    # binary: GPUI views only. Depends on tunes-core.
  docs/superpowers/specs/
```

`tunes-core` never imports gpui. `gpui-tunes` never reads tags, touches the audio
device, or filters tracks itself; it calls `tunes-core` and renders what it gets.
This rule is enforced by the crate boundary: `tunes-core/Cargo.toml` lists no gpui
crate, so a violation fails to compile.

### tunes-core

Dependencies: `lofty` 0.25 (tags, art), `rodio` 0.22 with features
`symphonia-aac` and `symphonia-isomp4` added to the defaults (MP3 and WAV are in
the defaults), `walkdir`, `thiserror`, `crossbeam-channel`.

Public modules:

**`track`**
- `TrackId(u32)` newtype, stable for the life of a `Library`.
- `Track { id, path: PathBuf, title, artist, album, genre, track_no: Option<u32>,
  disc_no: Option<u32>, year: Option<u32>, duration: Duration, has_art: bool }`.
  Missing tag fields fall back to: title = file stem, artist/album/genre = "Unknown".

**`scanner`**
- `pub fn scan(root: &Path) -> Result<Vec<Track>, ScanError>`.
- Walks recursively, accepts extensions `m4a mp3 wav flac ogg aac` (Opus is excluded: rodio has no decoder for it), reads tags
  with lofty. A file that fails to parse is skipped and reported through
  `tracing::warn!`; it never aborts the scan. Returns `ScanError::RootMissing` only
  when the root does not exist.
- Pure function, tested against fixture files under `crates/tunes-core/tests/fixtures`
  generated once with ffmpeg (1 second of silence per format with known tags).

**`library`**
- `Library::new(tracks: Vec<Track>) -> Library`. Assigns ids, builds sorted,
  deduplicated lists: `genres()`, `artists()`, `albums()` as `&[String]`.
  Artist and album lists are case-insensitively sorted; the "All" row is a UI
  concept, not a library one.
- `Filter { genre: Option<String>, artist: Option<String>, album: Option<String>,
  search: String }`.
- `Library::query(&self, filter: &Filter, sort: Sort) -> Vec<TrackId>` where
  `Sort { column: SortColumn, ascending: bool }` and `SortColumn` is
  `Title | Artist | Album | Genre | Duration | TrackNo`. Default sort is album,
  then disc, then track number, then title.
- `Library::artists_in(genre: Option<&str>)` and `albums_in(genre, artist)` so the
  column browser can narrow left-to-right.
- `Library::track(id) -> &Track`.
- `Library::cover_art(id) -> Option<Arc<[u8]>>`: reads the embedded picture with
  lofty on first request and memoizes it in a `Mutex<HashMap>`. Returns the raw
  encoded bytes; the UI decodes.
- Search is case-insensitive substring match on title, artist, album.

**`player`**
- `Player::new() -> Result<Player, PlayerError>`. Spawns one audio thread that owns the
  rodio `OutputStream` and `Sink`. The handle is `Clone + Send`.
- Commands, sent over a `crossbeam_channel::Sender<Command>`:
  `Play(PathBuf)`, `Pause`, `Resume`, `Seek(Duration)`, `SetVolume(f32)`, `Stop`.
  Methods on `Player` wrap each command.
- `Player::status() -> PlaybackStatus` clones out of an `Arc<Mutex<PlaybackStatus>>`
  that the audio thread updates:
  `PlaybackStatus { state: Stopped | Playing | Paused, path: Option<PathBuf>,
  position: Duration, duration: Option<Duration>, volume: f32,
  last_error: Option<String> }`.
- The audio thread loops on `recv_timeout(100ms)`: handles any command, then
  refreshes position from `Sink::get_pos()`, and when the sink is empty while
  state was `Playing` it sets `state = Stopped` and pushes `Event::TrackEnded`
  onto an events channel. `Player::events() -> Receiver<Event>` lets the UI
  observe `TrackEnded` and `Error(String)`.
- Decode failure on `Play` produces `Event::Error` and leaves state `Stopped`.
- No gapless. Seek uses `Sink::try_seek`; if it fails the error is reported and
  playback continues.

**`queue`**
- `Queue { order: Vec<TrackId>, current: Option<usize> }` with `set_order`,
  `jump_to(TrackId)`, `next()`, `prev()`, `current()`. Pure data, fully unit
  tested. Next/prev wrap at the ends (iTunes stops at the end; we wrap, simpler
  and no worse for v1). When the order is replaced the current track is kept
  if it is still present, otherwise `current` becomes `None`.

### gpui-tunes (UI)

Dependencies: `gpui-kit` 0.7, `tunes-core`, `image` (decode cover art for
`gpui::ImageSource`), `dirs` (home dir), `tracing-subscriber`.

Startup in `main.rs`:
1. Init tracing, resolve library root, create `Player`.
2. `gpui_kit::application().run`: `gpui_kit::init(cx)`,
   `Theme::change(ThemeMode::Dark, None, cx)`, open one window with `AppView`.
3. `AppView` spawns `scanner::scan` on a background executor; until it returns the
   content area shows a spinner with "Scanning <path>". On `Err(RootMissing)` it
   shows an empty state naming the path.

Views, one file each under `src/ui/`:

- **`app.rs`** `AppView`: owns `Library` (as `Option<Arc<Library>>`), `Player`,
  `Queue`, `Filter`, `Sort`, the `Entity`s of the child views and the
  `TableState`. It is the only view that calls `tunes-core` mutating APIs. It
  runs a 250 ms `cx.spawn` loop that polls `player.status()`, drains
  `player.events()`, and on `TrackEnded` plays `queue.next()`. Child views
  communicate upward through gpui events (`EventEmitter`), never by holding a
  reference to `AppView`.
- **`toolbar.rs`** `Toolbar`: prev, play/pause, next buttons; volume `Slider`;
  now-playing panel with cover art thumbnail, title, artist, elapsed, seek
  `Slider`, total time; search `Input`. Emits `ToolbarEvent::{Prev, PlayPause,
  Next, Seek(Duration), Volume(f32), Search(String)}`. Renders purely from a
  `ToolbarModel` struct passed in by `AppView`.
- **`sidebar.rs`** `LibrarySidebar`: gpui-component `Sidebar` with a Library
  group containing Songs, Artists, Albums, Genres. Emits `SidebarEvent::Select(Section)`.
- **`browser.rs`** `ColumnBrowser`: up to three `List`s (Genres, Artists, Albums)
  in an `h_resizable` group, each with an "All (n)" row at the top. Which lists are
  visible depends on the section: Songs shows all three, Artists shows Artists
  and Albums, Albums shows Albums only, Genres shows Genres, Artists, Albums.
  Emits `BrowserEvent::FilterChanged(Filter)`.
- **`song_table.rs`** `SongTableDelegate: TableDelegate`: columns Name, Time,
  Artist, Album, Genre, #. Holds a `Vec<TrackId>` plus `Arc<Library>` for
  lookup and the current playing id for the play glyph. `perform_sort` emits a
  sort request upward; `AppView` reruns `query` and refreshes. Double-click on a
  row (`TableEvent::DoubleClickedRow`) is handled by `AppView` to jump the queue.
- **`format.rs`**: `fmt_duration(Duration) -> String` ("3:05"), tested.

Layout: `v_flex` root; `Toolbar` top at fixed height; below it `h_resizable` with
`LibrarySidebar` left (fixed 180 px, collapsible) and a `v_resizable` right side
holding `ColumnBrowser` over the song `Table`.

### Data flow

```
scan thread ──Vec<Track>──▶ AppView builds Library, Queue(order = query(default))
Sidebar/Browser/Toolbar ──events──▶ AppView updates Filter/Sort ──▶ library.query
    ──▶ queue.set_order + table delegate rows + table.refresh
Table DoubleClickedRow ──▶ AppView queue.jump_to ──▶ player.play(path)
250 ms poll ──player.status()──▶ ToolbarModel ──▶ Toolbar re-render
player.events() TrackEnded ──▶ AppView plays queue.next()
```

Filter precedence: search applies on top of the browser selections. Changing the
sidebar section clears browser selections but keeps the search text.

### Error handling

- Scan: per-file failures logged and skipped. Missing root: empty state.
- Player creation failure (no audio device): the app still opens; the toolbar shows
  "No audio output" and play buttons are disabled.
- Decode or seek failure: toolbar shows the message for 5 s, and on decode failure
  `AppView` advances to the next track once (not in a loop; if the next also fails
  it stops).

### Coding practices

- `tunes-core` is `#![deny(missing_docs)]` on its public API and has no `unwrap`
  outside tests (`clippy::unwrap_used` denied).
- Workspace lints: `clippy::all`, `clippy::pedantic` as warn, `unsafe_code` forbid.
- Every `tunes-core` module has unit tests next to it; fixtures for scanner tests
  are committed (tiny files).
- UI code renders from plain structs (`ToolbarModel`, `Vec<TrackId>`) so views can
  be reasoned about without the player running.
- `cargo fmt` and `cargo clippy --workspace` clean before any commit.

### Testing

- `tunes-core`: scanner on fixtures (every format reads title/artist/album/duration;
  a corrupt file is skipped), library query (filter combinations, search, sort
  columns, stability of default sort), queue (next/prev wrap, jump, order
  replacement keeps current), player smoke test that constructs a `Player` and,
  if an output device exists, plays a fixture and observes `TrackEnded`
  (skipped with a message when no device).
- `gpui-tunes`: `format` unit tests. The UI is verified manually by running the app
  against the real library and screenshotting each section.

### Theme

Dark mode is set at startup and there is no light toggle in v1. Colors come from
gpui-component's theme tokens; no hard-coded colors in views except the accent
for the play glyph, which uses `cx.theme().primary`.
