# v0.2.0 release preparation

Package: **0.2.0**. Source preparation only; no release tag or publication yet.

## Included features

- Anonymous Invidious audio player alongside Deezer, with shared transport and
  provider-aware queues.
- Automatic HTTPS instance discovery, ten-minute candidate cache, bounded audio
  throughput sampling and pre-playback fallback.
- `melimo --invidious` works without manual instance configuration. Optional
  settings use `[invidious]`; older provider flags and sections must be migrated.

## Validation

The Invidious feature snapshot passed formatting, strict Clippy, 77 automated
tests and the release build; 3 manual/device tests were ignored.
Evidence: https://github.com/JeremySomsouk/Melimo/actions/runs/36164352583

The prepared 0.2.0 version and documentation snapshot still needs its own CI run.

Before publication:
1. Run formatting, strict Clippy, locked tests and locked release build.
2. Check `melimo --version` reports 0.2.0.
3. Verify live discovery, search, audible playback, pause, seek, queue behavior and
   instance failures from the listener's network.
4. Repeat the Deezer smoke test with an authorized account.
5. Review the dependency audit and final release notes, then tag `v0.2.0`.

Public-instance availability and live audio remain unverified. AAC-LC/M4A recorded
audio is supported; Opus-only streams, live video and video rendering are outside
this release. Throughput measures sampled delivery, not maximum instance capacity.
Errors after playback begins require retry; incompatible streams are not spliced.

---

# v0.1.0 release validation

Package: **0.1.0**. Release notes finalized on 2026-09-24.
Tag creation and public publication are separate from source preparation.

## Completed review

- Formatting, strict Clippy, 59 automatic tests, optimized build.
- Synthetic dark/light/monochrome rendering; visual inspection of player, queue and
  Discover at compact and normal sizes. The 40x18 player prioritizes the active
  lyric over credits; metadata may truncate when space is limited.
- PTY mock-mode navigation, resize, clean exit and terminal restoration.
- Generated MP3 decoding, seek sample skipping, pause/queue state and stale-event
  rejection. End-to-end generated-tone playback passed with an ALSA **null** sink;
  this validates the pipeline, not audible quality on physical hardware.
- HTTP security tests cover authentication, redirect refusal, credential scoping,
  bounded responses, media-host validation, favorite calls and lyric parsing.
- Login replacement preserves the previous session on failure. Storage is written
  only after successful authentication and refuses unsafe credential paths/files.
- CLI tests disable stored-credential access, so they cannot use a developer's login.

## Performance notes

Measured locally using optimized synthetic render tests, 100 frames per case:

| View | 100x30 median | 100x30 p95 |
| --- | ---: | ---: |
| Discover | 0.268 ms | 0.427 ms |
| Player with lyrics | 0.198 ms | 0.439 ms |
| Queue, 1,000 tracks | 1.118 ms | 1.686 ms |

These are TestBackend render timings, not network latency, audio latency or a
hardware guarantee. Idle PTY sampling and the redraw regression test check that
unchanged playback ticks do not force redundant frames. A 2.01-second idle mock
PTY sample produced zero output bytes and 0.00 CPU seconds at process tick precision. Millisecond playback state
is still updated; visible elapsed seconds, state changes and lyric-line transitions
trigger redraws. Pause/volume synchronization remains on the existing 100 ms worker
loop. Seeking re-downloads and decodes from the start, so later seeks can buffer.
Metadata operations share a session mutex; slow provider responses can delay other
metadata/stream-start requests, while the UI and ongoing audio remain separate.

## Privacy review scope

The review examined 18 commits reachable from main, 114 unique file blobs, current
refs, and 13 available CI job logs at the starting revision. There were no issues,
PRs, releases or additional refs. Binary fixture: generated tone, not service audio.
Pattern checks found no ARL, GitHub token, AWS access-key or private-key material.
The protocol stripe constant is public protocol data, not an account credential.

The public source history is prepared as a single root commit using the maintainer's
GitHub noreply identity. Earlier development history contained private metadata
and is excluded from that commit. Rewriting a branch does not purge hosted caches
or old CI logs; those must be reviewed before changing repository visibility.
Automated scanning is bounded evidence, not proof of absence of every possible secret.

OSV querybatch checked all 279 registry package versions in Cargo.lock during this
review and returned no vulnerability IDs. Re-run an up-to-date dependency audit
before publishing. No known-vulnerability result is a guarantee of secure code.

## Live acceptance

The maintainer confirmed live testing of main on 2026-09-24. This is maintainer
acceptance, not an independently observed run; detailed platform coverage was not
recorded. No credentials, account responses or listening details are retained.

For subsequent releases, repeat these checks:

1. Run `melimo --check-auth`, then `melimo` with your own authorized account.
2. Play a track with synchronized lyrics: verify audible playback, line progression,
   pause/resume, volume/mute and backward/forward seeks; confirm queue preservation.
3. Check a track without synchronized lyrics: plain/unavailable fallback must keep
   playback working. Stop, replace a track and seek rapidly; obsolete lyrics/events
   must not replace the current track's state.
4. Browse/search during playback; disconnect/reconnect the network and verify safe
   errors, explicit next-track recovery and clean terminal exit.
5. Inspect dark/light/mono on Linux/macOS terminals with a real device. Record only
   pass/fail and generic platform details, never account data or private track lists.

## Publication

1. Verify the final single-root-commit tree, noreply author/committer identity and
   all advertised branches/tags. Keep any development-history backup private.
2. Re-run the dependency audit and review hosted CI logs/assets. Rewriting main
   does not guarantee removal of cached or unreachable commits on GitHub; resolve
   retained-history exposure before making the existing repository public.
3. Tag `v0.1.0` on the validated snapshot and create the release from CHANGELOG.
   The initial release can be source-only; do not promise untested platform binaries.
