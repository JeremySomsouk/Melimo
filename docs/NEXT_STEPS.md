# Mélimo next steps

## COMPLETE — YouTube / Invidious audio provider (2026-09-25)

- [x] Introduce and test provider-aware media models and provider routing.
- [x] Preserve Deezer search, discovery, authentication and playback behavior.
- [x] Add validated, configurable Invidious settings (disabled by default).
- [x] Implement Invidious search and map results into shared media items.
- [x] Implement video metadata lookup and replaceable audio stream resolution.
- [x] Connect resolved audio to the existing player and normal mixed queue.
- [x] Support play/pause, seek, stop and provider identification in the TUI.
- [x] Handle provider, timeout, rate-limit, restriction and playback errors.
- [x] Add unit and focused HTTP/playback integration tests; run existing tests.
- [x] Document configuration, architecture and runtime dependencies.

Initial scope: provider-specific search and audio-only playback. No browser player,
authentication, video renderer or SponsorBlock integration for YouTube.

Implementation: the existing `Track` model now carries provider identity; routing
preserves Deezer capabilities and mixed queue snapshots. Invidious search and
metadata feed a separate stream resolver. Resolution selects audio-only AAC/M4A
and requests instance-proxied URLs (`local=true`); the existing bounded audio
pipeline handles playback and transport. No external runtime extractor is needed.

Validation: 73 automated tests pass, including all Deezer regressions, Invidious
HTTP fixtures, mixed queues, error propagation, nonseekable AAC decoding/seeking
and TUI provider labels at 40/60/100 columns. Formatting, strict Clippy and release
build pass. Both generated MP3 and AAC audio-device smoke tests pass. A temporary
local Invidious fixture exercised the real TUI through anonymous startup, search,
enqueue, queue playback, pause, seek while paused, resume, next, resize, stop and
clean exit. No live public instance or Deezer account was used in this milestone's
validation; live instance availability remains an operational check below.

### Future provider TODO (outside this milestone)

- [ ] Validate a maintainer-chosen live Invidious instance before the next release.

- [ ] Opt-in/configurable SponsorBlock segments (sponsor, intro, outro, self-promotion).
- [ ] Combined Deezer + YouTube search.
- [ ] YouTube playlists.
- [ ] Channel browsing.
- [ ] Subscriptions.
- [ ] Local YouTube history/favorites.
- [ ] Configurable resolver fallback (such as optional external yt-dlp).
- [ ] Native/external video playback.
- [ ] Terminal-rendered video.

## v0.1.0 terminal player complete

- [x] Give the terminal interface a more polished, distinctive Mélimo identity
  while keeping navigation simple and keyboard-first.
- [x] Add a cohesive accent palette with readable light/dark terminal colors and
  a monochrome fallback; use subtle highlights rather than visual clutter.
- [x] Improve spacing, borders, typography emphasis and hierarchy across Discover,
  search results, playlists and the queue.
- [x] Make Now Playing the visual centerpiece: prominent song/artist information,
  a styled progress bar, clear playback status and recognizable shortcut hints.
- [x] Polish karaoke with a prominent active line and subdued surrounding lyrics.
- [x] Add restrained loading/buffering feedback and consistent empty/error states.
- [x] Verify narrow terminal layouts, contrast and rendering performance; preview
  the design with synthetic metadata and lyrics.

First visual pass implemented: plum accent, dark/light/mono themes, rounded panels,
clearer player hierarchy, compact transport hints and empty queue guidance.
Synthetic rendering tests cover 60-column player controls and active lyrics.
Synthetic visual review, PTY resize/exit checks and optimized render measurements
are complete. The maintainer confirmed live testing of main on 2026-09-24.
See [release validation](RELEASE.md) for scope and publication checks.

## Website player / Rust-WASM follow-up

Requested after the terminal playback/discovery/queue/login work is complete.
Target: https://github.com/JeremySomsouk/jeremysomsouk.github.io
Desired route: a homepage shortcut to a dedicated web player (for example /melimo/).
No website changes are part of the current terminal checkpoint.

### Feasibility

A Rust/WebAssembly UI/shared queue core is feasible, similar to Cabane. The website
README confirms `docs/` serves GitHub Pages and `games/` contains existing Rust/WASM
engines. The terminal crate itself cannot simply be compiled unchanged for browsers:
Crossterm input, native threads and native audio need browser-specific adapters.

Suggested next investigation:
1. Extract provider-independent models, queue and playback state into a shared Rust
   library; keep terminal, browser and network backends separate.
2. Build a static web UI with a Web Audio or HTML media adapter and shared WASM core.
3. Verify an authorized browser authentication/playback integration before choosing
   a backend. Browser fetch obeys CORS and cannot freely set the Cookie header;
   compiling HTTP code to WASM does not remove those restrictions.
4. If a server is needed, design it as an authenticated, per-user service hosted
   separately. GitHub Pages serves static assets and cannot run that server. Do not
   bundle an ARL, license token or signed media URL into the public site/WASM bundle.
5. After that decision, add /melimo/ and a homepage navigation shortcut, with keyboard
   controls, mobile layout, autoplay handling and accessibility validation.

Start with a browser demo using synthetic audio/metadata to validate the UI. Decide
between a supported browser playback integration and a properly authenticated
service only after investigating current provider support. No anonymous/shared
account relay and no promise that full Deezer playback works on static Pages alone.

Sources checked 2026-09-23:
- Website: https://github.com/JeremySomsouk/jeremysomsouk.github.io
- Static hosting: https://docs.github.com/en/pages/getting-started-with-github-pages/what-is-github-pages
- Browser CORS: https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/CORS
