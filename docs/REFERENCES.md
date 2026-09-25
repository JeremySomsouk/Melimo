# Protocol references

Mélimo is an original Rust implementation. These notes identify protocol facts
consulted, not copied source code. No private service responses are stored.

## Milestone 2: authentication and track search

- [yne/dzr](https://github.com/yne/dzr), `dzr` at
  `e2e059468f725795fa96f8d419b73707d5b010ca`: inspected the gateway helper,
  session/authentication sequence, search.music request and track-list mapping.
- [License](https://github.com/yne/dzr/blob/master/LICENSE): Unlicense (public
  domain dedication with fallback terms). Checked before inspecting those portions.
- Gateway: `https://www.deezer.com/ajax/gw-light.php`.
- `deezer.ping` uses GET and yields `results.SESSION`.
- `deezer.getUserData` uses the `sid`/`arl` cookies; a positive `USER.USER_ID`
  establishes authentication, and `checkForm` supplies the API token.
- `search.music` uses POST with query/filter/output/start/nb; track results are
  in `results.data`, using SNG_ID, SNG_TITLE, ART_NAME, ALB_TITLE and DURATION.
- Implementation uses a fixed HTTPS endpoint, no redirects, sensitive cookie
  headers, bounded in-memory responses and fixed safe error messages.

This is an unofficial, changeable protocol. M2 was verified by the user's live
search/selection test. Local HTTP tests use synthetic account data. Test-only
loopback endpoints compile out of production; production endpoints use HTTPS.
OpenDeezer/Rusteer were not consulted.

## M3 playback and initial queue/discovery (2026-09-23)

M2 live search/selection was confirmed by the user. For this checkpoint the
Unlicense was checked again before consulting `dzr`, `dzr-url`, and `dzr-dec` at
`e2e059468f725795fa96f8d419b73707d5b010ca` in yne/dzr. Implementation is original Rust.

- `USER.OPTIONS.license_token`, `USER.USER_ID`, `USER.LOVEDTRACKS_ID` from user data.
- `song.getListData` provides `TRACK_TOKEN`, optional `FALLBACK` and resolved ID.
- `https://media.deezer.com/v1/get_url`: FULL, MP3_128, BF_CBC_STRIPE,
  license_token and track_tokens. No anonymous playback or access bypass.
- 2,048-byte stripes: decrypt every third complete stripe using Blowfish CBC and
  IV 00..07 reset per encrypted stripe. Partial final stripe remains unchanged.
  Track key combines the hexadecimal MD5 halves of the resolved ID and the public
  web-player stripe constant.
- The public explore page linked to
  `https://cdn-files.dzcdn.net/cache/js/app-web.df0fa824b1f95e92bbe9.js`.
  Its two percent-encoded eight-byte arrays matched the reference extraction
  sequence and confirmed the protocol constant. The asset is not bundled and
  no implementation source was copied from it.
- `search.music` output PLAYLIST, `playlist.getSongs`, `deezer.userMenu`
  (PLAYLISTS.data), and `radio.getUserRadio` implement discovery and Flow.
- Genre/mood presets are playlist keyword searches; familiar/discovery choices
  use favorites and a Flow batch with favorite IDs excluded. No claim of matching
  Deezer's full editorial home feed or its native discovery-mode algorithm.
- Rodio 0.21.1 API verified against its installed crate source: nonseekable decoder
  builder, output-stream lifetime, Sink transport, Source sample format. Rodio's
  dependencies use CoreAudio on macOS and require ALSA development headers on Linux.

Normal tests use synthetic HTTP responses and an original generated test tone.
Live account playback/discovery was subsequently smoke-tested successfully.

## Favorites and terminal login

The same public web-player asset was inspected for protocol facts only:
`song.getFavoriteIds` (paged data with SNG_ID), `song.addFavorites` and
`song.removeFavorites` (IDS array). Toggle operations first resolve membership.
Synthetic tests cover both writes; live favorites are not changed by test runs.
The terminal login flow opens Deezer web sign-in and accepts a manually copied ARL
through hidden input. It does not access the browser cookie store. Successfully
authenticated logins may be persisted locally; see SECURITY.md.

## Lyrics follow-up

The previously inspected public Deezer web-player asset calls `song.getLyrics`
with `SNG_ID`. It exposes `LYRICS_SYNC_JSON`, `LYRICS_TEXT`,
`LYRICS_COPYRIGHTS` and `LYRICS_WRITERS`. The implementation parses line text and
millisecond timestamps into original Rust models. Tests use invented demo lines;
no provider lyrics or private responses are stored in this repository.


## Invidious audio (2026-09-25)

The [official Invidious API documentation](https://docs.invidious.io/api/) was
consulted for `GET /api/v1/search` with `q` and `type=video`, and
`GET /api/v1/videos/:id`. The implementation maps videoId, title, author and
lengthSeconds, and resolves audio-only AAC-LC from adaptiveFormats (type, bitrate,
url). No website/player, extraction code, account responses or public instance
configuration is embedded. Tests use a loopback API and original generated audio.
Rodio's existing Symphonia decoder gains the AAC and ISO MP4 feature flags;
nonseekable MP3 and M4A decoding/forward discard are verified by fixtures.

Metadata requests set `local=true` to ask the configured instance for proxied
stream URLs, avoiding direct URLs bound to the instance's upstream IP. This API
parameter was verified in the upstream
[video route](https://github.com/iv-org/invidious/blob/master/src/invidious/routes/api/v1/videos.cr)
and [API change #3567](https://github.com/iv-org/invidious/pull/3567).
