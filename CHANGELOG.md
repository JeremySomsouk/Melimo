# Changelog

## 0.2.0 — Unreleased

- Add anonymous Invidious audio search and AAC-LC/M4A streaming.
- Discover HTTPS API instances automatically, with a ten-minute in-memory cache.
- Compare bounded audio samples and continue playback from the fastest successful
  sampled instance; retry failed candidates before playback.
- Preserve optional explicit instances and support anonymous `melimo --invidious`.
- Use Invidious naming throughout the CLI, configuration and terminal interface.
  Existing provider settings must migrate to `[invidious]`.
- Carry provider identity through search results and mixed queues; `P` switches
  the search provider while shared playback controls remain available.
- Cover discovery filtering, search failover, audio throughput selection, exact
  probe continuation, decoder behavior and existing Deezer regressions.

Live public-instance acceptance is pending. Mid-stream errors require retry;
measured throughput is not a guarantee of maximum bandwidth.

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
