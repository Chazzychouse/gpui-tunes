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
