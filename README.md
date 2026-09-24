# Mélimo

A streaming-only music player for your terminal, written in Rust.
Search, explore playlists, build a queue and follow synchronized lyrics from the keyboard.

**Version:** `0.1.0`. Linux and macOS are the initial targets.

![Mélimo player with synthetic demo metadata and lyrics](docs/assets/player.png)

## Features

- Deezer track and playlist search, personal playlists, favorites and Flow.
- MP3 playback, pause/resume, volume/mute, queue, shuffle and next track.
- Progress display and 10-second backward/forward seeking.
- Line-synchronized lyrics when available, with plain-text fallback and credits.
- Dark plum, light and monochrome themes; compact player controls.
- Offline metadata demo, hidden login prompt and optional local session storage.

Mélimo is an **unofficial client**, not affiliated with Deezer. It requires your own
account and access to the tracks you play. Provider behavior can change without
notice; streaming-only does not imply official API support.

## Install

Install current stable Rust. On Debian/Ubuntu, install the audio build dependencies:

```sh
sudo apt-get install libasound2-dev pkg-config
```

On macOS, install Xcode Command Line Tools (`xcode-select --install`). Then:

```sh
git clone https://github.com/JeremySomsouk/Melimo.git
cd Melimo
cargo install --locked --path .
melimo --mock
```

Playback needs an interactive terminal and an audio output device. The mock demo
uses fictional metadata, makes no network requests and does not play audio.
Windows is not a validated release target; credential persistence is disabled
outside Unix until equivalent file-access protections are implemented.

## Sign in

```sh
melimo --login
```

Sign in on Deezer's own page, then copy the `arl` cookie from browser DevTools
(Application/Storage → Cookies → `https://www.deezer.com`) into the hidden terminal
prompt. Mélimo never asks for your Deezer password or reads the browser cookie store.

**Treat the ARL like a password.** Never paste it into an issue, chat, screenshot,
command-line argument or committed file. After authentication succeeds, Mélimo can
save it locally. It is a plaintext credential protected by filesystem permissions,
not an encrypted keychain entry.

| Option | Behavior |
| --- | --- |
| `melimo` | Use `DEEZER_ARL`, then the saved login |
| `melimo --login` | Explicit interactive login; save only after successful authentication |
| `melimo --check-auth` | Check login without the TUI or displaying account details |
| `melimo --forget` | Remove the saved login |
| `MELIMO_NO_STORE=1 melimo --login` | Use a login for this process only; do not read/write saved credentials |

`DEEZER_ARL` is never automatically saved. To supply it in Bash without putting its
value in shell history:

```bash
read -rsp 'Deezer ARL: ' DEEZER_ARL
printf '\n'
export DEEZER_ARL
melimo
unset DEEZER_ARL
```

A saved login lives in `~/Library/Application Support/melimo/session` on macOS,
or `$XDG_DATA_HOME/melimo/session` (default `~/.local/share/melimo/session`) on Linux.
The directory must be owner-only (`0700`) and the regular file owner-only (`0600`).
Unsafe permissions, symlinks, hard-linked credentials and oversized files are refused.
A saved login explicitly rejected at startup is removed; transient network failures
leave it intact. A failed interactive replacement preserves the previous login.

## Find and play music

Start on **Discover**. Choose a genre or mood to search playlists, inspect one with
`Enter`, then play a track or press `a` for the displayed set. `d` returns to browsing
while playback continues. `/` edits search; `Tab` changes track/playlist search when
not typing.

**Familiar** loads favorites. **For you** loads a batch of Flow recommendations.
**Discover** excludes the first 1,000 favorites from Flow; it does not guarantee
never-heard songs. Genre/mood shortcuts are playlist keyword searches, not
personalized genre filters.

`p` opens the player; `l` toggles lyrics. Seeking restarts the stream and decodes
forward in bounded memory, so it can buffer and uses extra network traffic.
The queue and pause state are preserved. Lyrics highlight whole lines, not individual
words, and do not remove vocals. Missing lyrics never prevent playback.

## Controls

| Key | Action |
| --- | --- |
| `j/k`, `↑/↓`, `g/G` | Select / first / last |
| `Enter` | Browse, inspect playlist or play selected track |
| `a`, `r` | Play all / shuffle and play |
| `p`, `b` | Player / upcoming queue |
| `e`, `Delete` | Enqueue selected track / remove queued track |
| `Space`, `n`, `s` | Pause/resume / next / stop and clear queue |
| `+/-`, `m` | Volume in 5% steps / mute |
| `←/→` in player | Seek backward/forward 10 seconds |
| `l` | Lyrics/karaoke |
| `f` | Add/remove selected or current song from **your Deezer favorites** |
| `L` | Refresh login; stops playback and clears the queue |
| `d`, `/`, `Tab` | Discover / edit search / change search type |
| `Backspace`, `Ctrl+U` | Delete character / clear search |
| `?` | Help |
| `q`, `Esc` | Back; quit from Discover |
| `Ctrl+C` | Quit |

New searches preserve the queue. Enter on a queued song skips earlier entries.
Playback errors halt automatic progression; `n` explicitly skips the failed song.

## Appearance

```sh
MELIMO_THEME=dark melimo    # default
MELIMO_THEME=light melimo
MELIMO_THEME=mono melimo    # inherit terminal colors
```

A nonempty `NO_COLOR` takes precedence. Themes are read once at startup. A width
of 40 columns is the compact minimum for usable player controls; larger windows
show more metadata and lyrics. Terminals smaller than that remain safe to resize.

## Privacy and limitations

- No audio download/export, telemetry or listening-history log. Audio and lyrics
  stay in memory; OS swap/core dumps are outside that guarantee.
- The only intentional user-data write is the optional saved login and its temporary
  file during atomic replacement. Storage permissions do not protect against other
  processes running as you, administrators, backups or compromised machines.
- ARL cookies go only to the fixed HTTPS Deezer gateway. Media authorization uses
  a fixed HTTPS endpoint; audio URLs are restricted to Deezer/CDN hosts. Redirects
  are disabled and errors omit raw provider responses and credential-bearing URLs.
- Search returns up to 50 results. Playlist/favorite sets cap at 1,000 tracks.
  Flow is a finite batch, not an endlessly replenished radio.
- No previous-track control, persistent queue, alternate quality or browser player.
- Now-playing metadata appears in the terminal title, so it can be visible in
  desktop screenshots or terminal integrations.

See [security](SECURITY.md), [release validation](docs/RELEASE.md) and
[protocol references](docs/REFERENCES.md).

## Development

```sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
cargo build --locked --release
```

Tests use synthetic HTTP responses, invented lyrics and a generated tone; they do
not need a Deezer account. CLI tests disable saved-login access. Optional checks:

```sh
cargo test --locked --release synthetic_render_review -- --ignored --nocapture
cargo test --locked audio_device_smoke -- --ignored
```

The first measures rendering with synthetic metadata and a 1,000-track queue.
The second plays a short generated tone through the default output device.
Neither proves live Deezer playback or lyric timing; the release checklist covers that.

The TUI and state live in `src/tui` and `src/app`; provider operations are cancellable
Tokio tasks. Separate network, decoder and audio workers communicate through bounded
MP3/PCM channels. Display updates follow elapsed seconds and lyric-line changes;
audio never waits for a terminal redraw.

Next: investigate a shared Rust core and web prototype. See [next steps](docs/NEXT_STEPS.md).
Contributions should stay focused and never include account responses, cookies or
personal test fixtures.

## License

[MIT](LICENSE). See the protocol reference notes for third-party attribution.
