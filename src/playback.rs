//! Bounded in-memory pipeline: HTTP -> audio decoder -> PCM -> audio device.
//! The device callback never waits on network I/O.
use crate::{
    app::{action::Action, state::PlaybackState},
    provider::{Track, router::Providers},
};
use rodio::{Decoder, OutputStreamBuilder, Sink, Source};
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc as sync,
    },
    thread,
    time::Duration,
};
use tokio::{sync::mpsc, task::JoinHandle};

pub struct Playback {
    cancelled: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    volume: Arc<AtomicU32>,
    network: JoinHandle<()>,
}
impl Drop for Playback {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.network.abort();
    }
}
impl Playback {
    pub fn toggle_pause(&self) {
        self.paused.fetch_xor(true, Ordering::Relaxed);
    }
    /// Set the output gain (0.0..=1.0). Takes effect immediately on the live sink.
    pub fn set_volume(&self, level: f32) {
        self.volume
            .store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
    pub fn start(
        provider: Arc<Providers>,
        track: Track,
        id: u64,
        tx: mpsc::Sender<Action>,
        offset: u64,
        initially_paused: bool,
        volume: f32,
    ) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(initially_paused));
        let volume = Arc::new(AtomicU32::new(volume.clamp(0.0, 1.0).to_bits()));
        let (audio_tx, audio_rx) = mpsc::channel(32); // Bounded chunks of encoded audio.
        let notify = tx.clone();
        let network_cancel = cancelled.clone();
        let network = tokio::spawn(async move {
            if let Err(error) = provider.stream(track, audio_tx.clone()).await {
                // Preserve the useful provider failure; a closed byte channel must
                // not race it with a generic decoder error or queue advancement.
                network_cancel.store(true, Ordering::Relaxed);
                let _ = notify
                    .send(Action::PlaybackUpdate {
                        id,
                        state: PlaybackState::Error(error),
                        elapsed_ms: 0,
                        buffering: false,
                    })
                    .await;
            }
        });
        let cancel = cancelled.clone();
        let pause = paused.clone();
        let gain = volume.clone();
        let spawn_tx = tx.clone();
        if thread::Builder::new()
            .name("melimo-audio".into())
            .spawn(move || {
                let result = play(audio_rx, cancel.clone(), pause, id, &tx, offset, gain);
                if !cancel.load(Ordering::Relaxed)
                    && let Err(error) = result
                {
                    let _ = tx.blocking_send(Action::PlaybackUpdate {
                        id,
                        state: PlaybackState::Error(error.into()),
                        elapsed_ms: 0,
                        buffering: false,
                    });
                }
                cancel.store(true, Ordering::Relaxed);
            })
            .is_err()
        {
            cancelled.store(true, Ordering::Relaxed);
            network.abort();
            let _ = spawn_tx.try_send(Action::PlaybackUpdate {
                id,
                state: PlaybackState::Error("Cannot start audio worker.".into()),
                elapsed_ms: 0,
                buffering: false,
            });
        }
        Self {
            cancelled,
            paused,
            volume,
            network,
        }
    }
}

struct AudioReader {
    rx: mpsc::Receiver<Vec<u8>>,
    current: io::Cursor<Vec<u8>>,
    cancelled: Arc<AtomicBool>,
}
impl Read for AudioReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            if self.cancelled.load(Ordering::Relaxed) {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            let n = self.current.read(out)?;
            if n > 0 {
                return Ok(n);
            }
            match self.rx.try_recv() {
                Ok(bytes) => self.current = io::Cursor::new(bytes),
                Err(mpsc::error::TryRecvError::Disconnected) => return Ok(0),
                Err(mpsc::error::TryRecvError::Empty) => thread::sleep(Duration::from_millis(10)),
            }
        }
    }
}
impl Seek for AudioReader {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

struct Pcm {
    rx: sync::Receiver<Vec<f32>>,
    current: std::vec::IntoIter<f32>,
    channels: u16,
    rate: u32,
    samples: Arc<AtomicU64>,
    buffering: Arc<AtomicBool>,
    silence_remaining: u16,
}
impl Iterator for Pcm {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.silence_remaining > 0 {
            self.silence_remaining -= 1;
            return Some(0.0);
        }
        if let Some(sample) = self.current.next() {
            self.samples.fetch_add(1, Ordering::Relaxed);
            return Some(sample);
        }
        match self.rx.try_recv() {
            Ok(chunk) => {
                self.current = chunk.into_iter();
                self.buffering.store(false, Ordering::Relaxed);
                self.next()
            }
            Err(sync::TryRecvError::Disconnected) => None,
            Err(sync::TryRecvError::Empty) => {
                self.buffering.store(true, Ordering::Relaxed);
                self.silence_remaining = self.channels.saturating_sub(1);
                Some(0.0)
            }
        }
    }
}
impl Source for Pcm {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

fn discard_samples(
    decoder: &mut impl Iterator<Item = f32>,
    count: u64,
    cancel: &AtomicBool,
) -> bool {
    for _ in 0..count {
        if cancel.load(Ordering::Relaxed) || decoder.next().is_none() {
            return false;
        }
    }
    true
}

fn play(
    rx: mpsc::Receiver<Vec<u8>>,
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    id: u64,
    tx: &mpsc::Sender<Action>,
    offset: u64,
    volume: Arc<AtomicU32>,
) -> Result<(), &'static str> {
    let reader = AudioReader {
        rx,
        current: io::Cursor::new(Vec::new()),
        cancelled: cancel.clone(),
    };
    let mut decoder = Decoder::builder()
        .with_data(reader)
        .with_seekable(false)
        .build()
        .map_err(|_| "Cannot decode this audio stream. Try another track.")?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    let channels = decoder.channels();
    let rate = decoder.sample_rate();
    let device_failed = Arc::new(AtomicBool::new(false));
    let device_error = device_failed.clone();
    let mut output = OutputStreamBuilder::from_default_device()
        .map_err(|_| "No audio output device available.")?
        .with_error_callback(move |_| {
            device_error.store(true, Ordering::Relaxed);
        })
        .open_stream()
        .map_err(|_| "Cannot open the audio output device.")?;
    output.log_on_drop(false);
    let sink = Sink::connect_new(output.mixer());
    let (pcm_tx, pcm_rx) = sync::sync_channel(8); // At most 128 KiB of decoded PCM.
    let decode_cancel = cancel.clone();
    thread::Builder::new()
        .name("melimo-decode".into())
        .spawn(move || {
            // Reopen and decode forward for seeks: bounded memory, no disk cache.
            let skip = offset
                .saturating_mul(u64::from(rate))
                .saturating_mul(u64::from(channels));
            if !discard_samples(&mut decoder, skip, &decode_cancel) {
                return;
            }
            while !decode_cancel.load(Ordering::Relaxed) {
                let mut chunk: Vec<f32> = decoder.by_ref().take(4096).collect();
                if chunk.is_empty() {
                    break;
                }
                loop {
                    if decode_cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    match pcm_tx.try_send(chunk) {
                        Ok(()) => break,
                        Err(sync::TrySendError::Disconnected(_)) => return,
                        Err(sync::TrySendError::Full(value)) => {
                            chunk = value;
                            thread::sleep(Duration::from_millis(10));
                        }
                    }
                }
            }
        })
        .map_err(|_| "Cannot start audio decoder.")?;
    let samples = Arc::new(AtomicU64::new(0));
    let buffering = Arc::new(AtomicBool::new(true));
    if pause.load(Ordering::Relaxed) {
        sink.pause();
    }
    sink.append(Pcm {
        rx: pcm_rx,
        current: Vec::new().into_iter(),
        channels,
        rate,
        samples: samples.clone(),
        buffering: buffering.clone(),
        silence_remaining: 0,
    });
    let mut applied_volume = f32::NAN;
    loop {
        if cancel.load(Ordering::Relaxed) {
            sink.stop();
            return Ok(());
        }
        if device_failed.load(Ordering::Relaxed) {
            return Err("Audio output device disconnected or failed.");
        }
        // Apply gain changes from the UI thread without touching the sink from afar.
        let level = f32::from_bits(volume.load(Ordering::Relaxed));
        if level != applied_volume {
            sink.set_volume(level);
            applied_volume = level;
        }
        let elapsed_ms = offset.saturating_mul(1000)
            + samples.load(Ordering::Relaxed).saturating_mul(1000)
                / u64::from(channels)
                / u64::from(rate);
        let state = if sink.empty() {
            PlaybackState::Finished
        } else if pause.load(Ordering::Relaxed) {
            sink.pause();
            PlaybackState::Paused
        } else {
            sink.play();
            PlaybackState::Playing
        };
        let finished = state == PlaybackState::Finished;
        let action = Action::PlaybackUpdate {
            id,
            state,
            elapsed_ms,
            buffering: buffering.load(Ordering::Relaxed),
        };
        if finished {
            let _ = tx.blocking_send(action);
            break;
        }
        let _ = tx.try_send(action);
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_and_seeks_aac_m4a_from_nonseekable_chunks() {
        let bytes = include_bytes!("../tests/fixtures/tone.m4a");
        let (tx, rx) = mpsc::channel(2);
        let feed = thread::spawn(move || {
            for chunk in bytes.chunks(127) {
                if tx.blocking_send(chunk.to_vec()).is_err() {
                    break;
                }
            }
        });
        let cancel = Arc::new(AtomicBool::new(false));
        let mut decoder = Decoder::builder()
            .with_data(AudioReader {
                rx,
                current: io::Cursor::new(Vec::new()),
                cancelled: cancel.clone(),
            })
            .with_seekable(false)
            .build()
            .unwrap();
        assert_eq!(decoder.channels(), 2);
        assert_eq!(decoder.sample_rate(), 44100);
        assert!(discard_samples(&mut decoder, 8820, &cancel));
        let remainder: Vec<_> = decoder.collect();
        assert!(remainder.len() >= 13230);
        assert!(remainder.iter().any(|s| s.abs() > 0.01));
        feed.join().unwrap();
    }

    #[tokio::test]
    async fn provider_failure_reaches_ui_without_decoder_error_race() {
        let providers = Arc::new(Providers::default());
        let (tx, mut rx) = mpsc::channel(8);
        let player = Playback::start(
            providers,
            Track {
                provider: crate::provider::ProviderId::YouTube,
                id: "abcdefghijk".into(),
                title: "Synthetic".into(),
                artist: "Channel".into(),
                album: String::new(),
                duration_secs: 120,
            },
            7,
            tx,
            0,
            false,
            1.0,
        );
        let action = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(action, Action::PlaybackUpdate { id: 7, state: PlaybackState::Error(e), .. } if e.contains("YouTube is disabled"))
        );
        drop(player);
    }
    #[test]
    #[ignore = "requires an audio device; plays a generated quarter-second tone"]
    fn audio_device_smoke() {
        audio_device_fixture(include_bytes!("../tests/fixtures/tone.mp3"));
    }
    #[test]
    #[ignore = "requires an audio device; plays a generated quarter-second AAC tone"]
    fn audio_device_m4a_smoke() {
        audio_device_fixture(include_bytes!("../tests/fixtures/tone.m4a"));
    }
    fn audio_device_fixture(bytes: &'static [u8]) {
        let (tx, rx) = mpsc::channel(32);
        for chunk in bytes.chunks(2048) {
            tx.try_send(chunk.to_vec()).unwrap();
        }
        drop(tx);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let (notify, mut events) = mpsc::channel(64);
        let worker = thread::spawn(move || {
            play(
                rx,
                stop,
                Arc::new(AtomicBool::new(false)),
                1,
                &notify,
                0,
                Arc::new(AtomicU32::new(1.0f32.to_bits())),
            )
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut finished = false;
        while std::time::Instant::now() < deadline {
            if matches!(
                events.try_recv(),
                Ok(Action::PlaybackUpdate {
                    state: PlaybackState::Finished,
                    ..
                })
            ) {
                finished = true;
                break;
            }
            if worker.is_finished() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        while let Ok(Action::PlaybackUpdate { state, .. }) = events.try_recv() {
            finished |= state == PlaybackState::Finished;
        }
        cancel.store(true, Ordering::Relaxed);
        worker.join().unwrap().unwrap();
        assert!(finished, "audio did not reach completion");
    }
    #[test]
    fn decodes_generated_mp3_from_small_stream_chunks_without_seeking() {
        let (tx, rx) = mpsc::channel(2);
        let bytes = include_bytes!("../tests/fixtures/tone.mp3");
        let feed = thread::spawn(move || {
            for chunk in bytes.chunks(127) {
                tx.blocking_send(chunk.to_vec()).unwrap();
            }
        });
        let reader = AudioReader {
            rx,
            current: io::Cursor::new(Vec::new()),
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let decoder = Decoder::builder()
            .with_data(reader)
            .with_seekable(false)
            .build()
            .unwrap();
        assert_eq!(decoder.channels(), 2);
        assert_eq!(decoder.sample_rate(), 44100);
        let samples: Vec<f32> = decoder.collect();
        assert!(samples.len() >= 22050);
        assert!(samples.iter().any(|sample| sample.abs() > 0.01));
        feed.join().unwrap();
        let mut seek_decoder = Decoder::builder()
            .with_data(io::Cursor::new(bytes.to_vec()))
            .with_seekable(false)
            .build()
            .unwrap();
        let cancel = AtomicBool::new(false);
        assert!(discard_samples(&mut seek_decoder, 8820, &cancel));
        let remainder: Vec<_> = seek_decoder.by_ref().collect();
        assert_eq!(remainder, samples[8820..]);
        assert!(!discard_samples(&mut seek_decoder, 1, &cancel));
        cancel.store(true, Ordering::Relaxed);
        assert!(!discard_samples(&mut std::iter::repeat(0.0), 100, &cancel));
    }
    #[test]
    fn reader_handles_boundaries_eof_and_cancellation() {
        let (tx, rx) = mpsc::channel(2);
        tx.try_send(vec![1, 2]).unwrap();
        tx.try_send(vec![3]).unwrap();
        drop(tx);
        let cancel = Arc::new(AtomicBool::new(false));
        let mut reader = AudioReader {
            rx,
            current: io::Cursor::new(Vec::new()),
            cancelled: cancel.clone(),
        };
        let mut data = Vec::new();
        reader.read_to_end(&mut data).unwrap();
        assert_eq!(data, [1, 2, 3]);
        cancel.store(true, Ordering::Relaxed);
        assert!(reader.read(&mut [0]).is_err());
    }
    #[test]
    fn pcm_starvation_is_silent_and_does_not_advance_track_time() {
        let (tx, rx) = sync::sync_channel(1);
        let samples = Arc::new(AtomicU64::new(0));
        let mut pcm = Pcm {
            rx,
            current: Vec::new().into_iter(),
            channels: 2,
            rate: 44100,
            samples: samples.clone(),
            buffering: Arc::new(AtomicBool::new(false)),
            silence_remaining: 0,
        };
        assert_eq!(pcm.next(), Some(0.0));
        assert_eq!(pcm.next(), Some(0.0)); // Preserve stereo frame alignment.
        assert_eq!(samples.load(Ordering::Relaxed), 0);
        tx.send(vec![0.5, -0.5]).unwrap();
        drop(tx);
        assert_eq!(pcm.collect::<Vec<_>>(), [0.5, -0.5]);
        assert_eq!(samples.load(Ordering::Relaxed), 2);
    }
}
