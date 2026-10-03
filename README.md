# gpui-tunes

A classic-iTunes-style player for a local audio library, built with
[GPUI Kit](https://github.com/longbridge/gpui-kit).

## Prerequisites (Linux)

- A Rust toolchain, 1.96 or newer (edition 2024).
- ALSA development headers for cpal: `alsa-lib` on Arch, `libasound2-dev` on Debian/Ubuntu.
- The X11/Wayland/xkbcommon/fontconfig development libraries gpui needs:
  `libxkbcommon`, `wayland`, `libx11`, `libxcb`, `fontconfig`, `vulkan-icd-loader` on Arch;
  the `-dev` equivalents on Debian/Ubuntu.

## Run

    cargo run --release

Scans `~/music/library` at startup. Override with:

    GPUI_TUNES_LIBRARY=/path/to/music cargo run --release

Supported formats: M4A (AAC), MP3, WAV, FLAC, OGG Vorbis. Opus and ALAC are not decodable by rodio and are not scanned.

## Layout

- `crates/tunes-core` — scanning (lofty), library queries, queue, playback (rodio). No UI dependency.
- `crates/gpui-tunes` — the GPUI views.

## Develop

    cargo test --workspace
    cargo clippy --workspace --all-targets

Player tests open the default audio output device and skip themselves with a message when none is available.
