# Mélimo

**Your music, in your terminal.**

Mélimo is a lightweight music player written in Rust. Search with Deezer or
Invidious, build one queue, and control playback from your keyboard. Follow
synchronized lyrics when Deezer provides them.

**v0.2.0 — release preparation · Linux and macOS · MIT**

![Mélimo terminal player with synthetic demo metadata and lyrics](docs/assets/player.png)

## One player, two providers

Both providers share the player, queue, pause/resume, volume, mute, shuffle,
next-track and seek controls. Switch search providers with `P` without
interrupting the current track or clearing your queue.

| Capability | Deezer | Invidious |
| --- | --- | --- |
| Track search and audio playback | Yes | Yes, from recorded videos |
| Access | Your own Deezer account | Anonymous; no login |
| Setup | Sign in once or supply an ARL | Automatic instance discovery |
| Playlists, favorites and Flow | Yes | Not yet |
| Synchronized lyrics | When available | Not yet |
| Audio formats | MP3 | AAC-LC/M4A |

Mélimo streams audio in memory; it does not offer audio downloads or export.
It is an unofficial client, unaffiliated with either provider. Availability
depends on your account access or the selected public instance.

## Install

Install current stable Rust and the audio build prerequisites.

**Debian / Ubuntu**

```sh
sudo apt-get install libasound2-dev pkg-config
```

**macOS**

```sh
xcode-select --install
```

Then build from source:

```sh
git clone https://github.com/JeremySomsouk/Melimo.git
cd Melimo
cargo install --locked --path .
```

The commands install the checked-out source version. v0.2.0 is being prepared
and has not been tagged or published. Playback needs an interactive terminal
and an audio output device. Windows is not a validated target.

## Start listening

### Invidious — no account needed

```sh
melimo --invidious
```

Press `/`, enter a search, and press `Enter`. Select a result and press
`Enter` to play it, or `e` to add it to the queue.

Mélimo discovers HTTPS API instances on first use and caches candidates for ten
minutes. Before playback, it compares small audio samples from up to three
instances at a time and keeps the fastest successful response. Failed candidates
are discarded; another batch is tried if none succeeds. The winning sample flows
straight into playback without downloading it again.

This measures current delivery speed among sampled instances, not their maximum
bandwidth. Public instances can be unavailable, restricted or rate-limited.
If playback fails after starting, retry the track or press `n` to skip it.

### Deezer — your library and lyrics

```sh
melimo --login
```

Sign in on Deezer's own page, then copy the `arl` cookie from browser DevTools
(Application/Storage → Cookies → `https://www.deezer.com`) into the hidden terminal
prompt. Mélimo never asks for your password or reads the browser cookie store.

Treat the ARL like a password: never put it in a command-line argument, issue,
chat, screenshot or committed file. The optional saved login is plaintext,
protected by owner-only filesystem permissions; it is not an encrypted keychain.

For subsequent launches:

```sh
melimo
```

Deezer starts on Discover. Browse playlists or search with `/`; use `Tab`
outside text entry to switch between track and playlist search. With Invidious
enabled, `P` switches between both providers.

| Command | Purpose |
| --- | --- |
| `melimo --invidious` | Start Invidious only, without reading Deezer credentials |
| `melimo` | Start Deezer using `DEEZER_ARL` or the saved login; also enable Invidious |
| `melimo --login` | Sign in interactively; save only after successful authentication |
| `melimo --check-auth` | Validate Deezer credentials without opening the player |
| `melimo --forget` | Remove the saved Deezer login |
| `MELIMO_NO_STORE=1 melimo --login` | Sign in for this process without saved-login access |
| `melimo --mock` | Explore the interface with fictional metadata and no audio or network |
| `melimo --version` | Show the installed version |

`DEEZER_ARL` is never automatically saved. Saved credentials live in
`~/Library/Application Support/melimo/session` on macOS, or
`$XDG_DATA_HOME/melimo/session` on Linux (default
`~/.local/share/melimo/session`). See [security](SECURITY.md) for storage protections.

## Queue, playback and lyrics

Searches and provider switches preserve the current track and queue. Results are
labelled `[DZR]` or `[INV]`. Press `b` to open the queue; playing a queued track
skips earlier entries. Playback errors stop automatic progression so you can retry
or explicitly skip a failed track.

Deezer's **Familiar** view loads favorites; **For you** loads a batch of Flow
recommendations. **Discover** excludes the first 1,000 favorites from that batch.
Genre and mood shortcuts search playlist keywords.

Press `p` to open the player and `l` to show lyrics. Synchronized lyrics highlight
whole lines, with plain-text fallback when available. Missing lyrics do not block
playback. This is a lyrics view, not vocal removal.

| Key | Action |
| --- | --- |
| `j/k`, `↑/↓`, `g/G` | Select / first / last |
| `Enter` | Open a playlist or play the selected track |
| `/`, `Tab`, `P` | Edit search / change Deezer search type / switch provider |
| `a`, `r` | Play all / shuffle and play the displayed set |
| `e`, `Delete` | Enqueue a track / remove a queued track |
| `p`, `b`, `d` | Player / queue / browse or search |
| `Space`, `n`, `s` | Pause/resume / next / stop and clear queue |
| `+/-`, `m` | Volume in 5% steps / mute |
| `←/→` in player | Seek backward/forward 10 seconds |
| `l`, `f` | Toggle lyrics / toggle Deezer favorite |
| `L` | Refresh Deezer login; stops playback and clears the queue |
| `Backspace`, `Ctrl+U` | Delete a search character / clear search |
| `?` | Show help |
| `q`, `Esc` | Back; quit from the initial browse/search screen |
| `Ctrl+C` | Quit |

Provider and navigation shortcuts apply outside text entry.

## Configuration

Invidious works without a configuration file. To disable it or choose a specific
instance, use `$XDG_CONFIG_HOME/melimo/config.toml` (default
`~/.config/melimo/config.toml`, including macOS), or set `MELIMO_CONFIG` to a file.

```toml
[invidious]
enabled = true
# Optional override; omit to discover instances automatically:
# invidious_instance = "https://example.invalid"
```

An explicit instance bypasses discovery and speed comparison. Use HTTPS for
remote instances; HTTP supports locally hosted instances. URLs with credentials,
query strings or fragments are rejected. Set `enabled = false` to disable the
provider. See [the example configuration](config.example.toml).

For earlier development configurations, migrate to the `--invidious` flag and
the `[invidious]` section.

Choose a theme at startup:

```sh
MELIMO_THEME=dark melimo --invidious   # default
MELIMO_THEME=light melimo --invidious
MELIMO_THEME=mono melimo --invidious  # inherit terminal colors
```

A nonempty `NO_COLOR` takes precedence. The compact player supports terminals
from 40 columns wide; larger windows show more metadata and lyrics.

## Current limits

- Invidious plays recorded audio with an AAC-LC/M4A stream. Opus/WebM-only media,
  live streams and video playback are not supported. No external extractor or
  FFmpeg installation is needed at runtime.
- Seeking restarts the stream and decodes forward in bounded memory, which can
  buffer and use additional bandwidth.
- Instance selection happens before playback. Mid-stream failures require retry;
  Mélimo does not join audio from different instances.
- Deezer search returns up to 50 results; playlists and favorites cap at 1,000
  tracks. Flow is a finite batch. Invidious uses the first search results page.
- There is no persistent queue, previous-track control or combined-provider search.
- Audio and lyrics stay in memory; OS swap and core dumps are outside that
  guarantee. The selected instance and media hosts receive network requests.
- Now-playing metadata appears in the terminal title and may appear in screenshots.

Automated tests cover both providers, discovery and playback using synthetic data.
Live public-instance playback for v0.2 remains an acceptance check before release.
See [release validation](docs/RELEASE.md) and [planned work](docs/NEXT_STEPS.md).

## Development

```sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
cargo build --locked --release
```

Tests use synthetic HTTP responses, invented lyrics and generated audio; they
require no account or live public instance. Optional manual checks:

```sh
cargo test --locked --release synthetic_render_review -- --ignored --nocapture
cargo test --locked audio_device -- --ignored --test-threads=1
```

The rendering check exercises synthetic UI data. The device check plays generated
MP3 and AAC tones. Neither replaces live provider acceptance.

The TUI and state live in `src/tui` and `src/app`. Provider identity travels with
tracks through search, queue and playback. Network, decoder and audio workers use
bounded channels; terminal redraws do not block audio. Invidious discovery and
stream resolution remain separate from the shared player.

Contributions should stay focused and exclude credentials, account responses and
personal fixtures. See [security](SECURITY.md) and
[protocol references](docs/REFERENCES.md).

## License

[MIT](LICENSE).
