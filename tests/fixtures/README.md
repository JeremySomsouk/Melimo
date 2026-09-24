# Synthetic decoder fixture

`tone.mp3` is an original, generated 440 Hz sine tone lasting 0.25 seconds,
stereo 44.1 kHz MP3 at 128 kbps. It contains no service audio or account data.
It verifies decoding from small nonseekable stream chunks without an audio device.

Recreate with:

```sh
ffmpeg -f lavfi -i 'sine=frequency=440:duration=0.25' -ar 44100 -ac 2 \
  -codec:a libmp3lame -b:a 128k -map_metadata -1 -write_xing 0 tests/fixtures/tone.mp3
```

The ignored `playback::tests::audio_device_smoke` test plays this brief tone on
the default output device and checks completion. Normal tests never open audio
hardware. The generated fixture is provided under the repository's MIT license.
