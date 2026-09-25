# Changelog

## Unreleased

- Configurable anonymous YouTube/Invidious audio search and AAC/M4A playback.
- Provider-aware results and mixed queue; `P` switches search provider.
- Shared transport controls and bounded audio pipeline for both providers.
- Validated TOML settings, explicit provider errors and one expired-URL refresh.
- Synthetic HTTP, decoder, provider-routing and queue regression coverage.

## 0.1.0 — 2026-09-24

- Rust terminal player with Deezer search, discovery, playlists, favorites and Flow.
- Streaming-only MP3 playback, volume, mute, pause, seek, queue and shuffle.
- Line-synchronized lyrics with plain-text fallback and source credits.
- Dark, light and monochrome themes with compact keyboard controls.
- Optional authenticated login persistence and offline metadata demo.
- Bounded audio pipeline, redacted errors, restricted media endpoints and safer
  session-file handling.
- Reduced redundant redraws while preserving lyric-line transitions.

Initial source release for Linux and macOS. Windows is not a validated target.

Known limitations and validation: [release checklist](docs/RELEASE.md).
