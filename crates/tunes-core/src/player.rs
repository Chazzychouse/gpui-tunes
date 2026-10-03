//! Audio playback on a dedicated thread, controlled over a channel.
//!
//! rodio's output stream is created and owned by the audio thread. The
//! [`Player`] handle only sends commands and reads a shared status snapshot.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};
use rodio::stream::{DeviceSinkBuilder, MixerDeviceSink};
use rodio::{Decoder, Source};

/// Whether audio is flowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
    /// Nothing loaded, or the track finished.
    #[default]
    Stopped,
    /// Audio is playing.
    Playing,
    /// A track is loaded but paused.
    Paused,
}

/// Snapshot of the audio thread's state.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackStatus {
    /// Current playback state.
    pub state: PlaybackState,
    /// Path of the loaded track, if any.
    pub path: Option<PathBuf>,
    /// Elapsed time in the current track.
    pub position: Duration,
    /// Total length of the current track, when the decoder knows it.
    pub duration: Option<Duration>,
    /// Output volume, 0.0 to 1.0.
    pub volume: f32,
    /// Most recent error message, cleared on the next successful play.
    pub last_error: Option<String>,
}

impl Default for PlaybackStatus {
    fn default() -> Self {
        Self {
            state: PlaybackState::Stopped,
            path: None,
            position: Duration::ZERO,
            duration: None,
            volume: 1.0,
            last_error: None,
        }
    }
}

/// Notifications from the audio thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The current track played to its end.
    TrackEnded,
    /// Loading, decoding or seeking failed.
    Error(String),
}

/// Why a [`Player`] could not be created.
#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    /// The default output device could not be opened.
    #[error("could not open audio output: {0}")]
    Output(String),
}

#[derive(Debug)]
enum Command {
    Play(PathBuf),
    Pause,
    Resume,
    Toggle,
    Seek(Duration),
    SetVolume(f32),
    Stop,
}

/// Cloneable handle to the audio thread.
///
/// Events are delivered to whichever clone calls `try_recv_event` first; use
/// one consumer for events.
#[derive(Clone)]
pub struct Player {
    commands: Sender<Command>,
    events: Receiver<Event>,
    status: Arc<Mutex<PlaybackStatus>>,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player")
            .field("status", &self.status())
            .finish()
    }
}

impl Player {
    /// Open the default output device on a new audio thread.
    pub fn new() -> Result<Self, PlayerError> {
        let (commands, command_rx) = unbounded();
        let (event_tx, events) = unbounded();
        let (ready_tx, ready_rx) = bounded(1);
        let status = Arc::new(Mutex::new(PlaybackStatus::default()));
        let thread_status = Arc::clone(&status);

        std::thread::Builder::new()
            .name("tunes-audio".into())
            .spawn(move || audio_thread(&command_rx, &event_tx, &thread_status, &ready_tx))
            .map_err(|err| PlayerError::Output(err.to_string()))?;

        ready_rx
            .recv()
            .map_err(|_| PlayerError::Output("audio thread exited early".into()))??;

        Ok(Self {
            commands,
            events,
            status,
        })
    }

    /// Load and start playing `path`, replacing whatever was playing.
    pub fn play(&self, path: &Path) {
        self.send(Command::Play(path.to_path_buf()));
    }

    /// Pause if playing.
    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    /// Resume if paused.
    pub fn resume(&self) {
        self.send(Command::Resume);
    }

    /// Pause if playing, resume if paused.
    pub fn toggle(&self) {
        self.send(Command::Toggle);
    }

    /// Seek within the current track.
    pub fn seek(&self, position: Duration) {
        self.send(Command::Seek(position));
    }

    /// Set the output volume, clamped to 0.0..=1.0.
    pub fn set_volume(&self, volume: f32) {
        let volume = if volume.is_nan() {
            1.0
        } else {
            volume.clamp(0.0, 1.0)
        };
        self.send(Command::SetVolume(volume));
    }

    /// Stop and unload the current track.
    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    /// Latest status snapshot.
    pub fn status(&self) -> PlaybackStatus {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Pop the next pending event without blocking.
    pub fn try_recv_event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            tracing::error!("audio thread is gone");
        }
    }
}

type Loaded = (Decoder<BufReader<File>>, Option<Duration>);

fn open_source(path: &Path) -> Result<Loaded, String> {
    let describe = |err: &dyn std::fmt::Display| format!("{}: {err}", path.display());
    let file = File::open(path).map_err(|e| describe(&e))?;
    let byte_len = file.metadata().ok().map(|m| m.len());
    let mut builder = Decoder::builder()
        .with_data(BufReader::new(file))
        .with_seekable(true);
    if let Some(len) = byte_len {
        builder = builder.with_byte_len(len);
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        builder = builder.with_hint(ext);
    }
    let decoder = builder.build().map_err(|e| describe(&e))?;
    let duration = decoder.total_duration();
    Ok((decoder, duration))
}

fn fresh_player(sink: &MixerDeviceSink, volume: f32) -> rodio::Player {
    let player = rodio::Player::connect_new(sink.mixer());
    player.set_volume(volume);
    player
}

fn audio_thread(
    commands: &Receiver<Command>,
    events: &Sender<Event>,
    status: &Mutex<PlaybackStatus>,
    ready: &Sender<Result<(), PlayerError>>,
) {
    let sink = match DeviceSinkBuilder::open_default_sink() {
        Ok(sink) => sink,
        Err(err) => return drop(ready.send(Err(PlayerError::Output(err.to_string())))),
    };
    let mut player = rodio::Player::connect_new(sink.mixer());
    let _ = ready.send(Ok(()));

    let mut state = PlaybackState::Stopped;
    let mut volume = 1.0_f32;
    let mut path: Option<PathBuf> = None;
    let mut duration: Option<Duration> = None;
    let mut last_error: Option<String> = None;

    loop {
        let mut pending: Vec<Event> = Vec::new();

        match commands.recv_timeout(Duration::from_millis(100)) {
            Ok(Command::Play(new_path)) => {
                // A fresh player avoids `clear()`, which can race with a
                // source ending naturally and skip the new track.
                player = fresh_player(&sink, volume);
                match open_source(&new_path) {
                    Ok((source, total)) => {
                        player.append(source);
                        player.play();
                        state = PlaybackState::Playing;
                        path = Some(new_path);
                        duration = total;
                        last_error = None;
                    }
                    Err(message) => {
                        state = PlaybackState::Stopped;
                        tracing::warn!(%message, "could not play");
                        path = None;
                        duration = None;
                        last_error = Some(message.clone());
                        pending.push(Event::Error(message));
                    }
                }
            }
            Ok(Command::Pause) => {
                if state == PlaybackState::Playing {
                    player.pause();
                    state = PlaybackState::Paused;
                }
            }
            Ok(Command::Resume) => {
                if state == PlaybackState::Paused {
                    player.play();
                    state = PlaybackState::Playing;
                }
            }
            Ok(Command::Toggle) => match state {
                PlaybackState::Playing => {
                    player.pause();
                    state = PlaybackState::Paused;
                }
                PlaybackState::Paused => {
                    player.play();
                    state = PlaybackState::Playing;
                }
                PlaybackState::Stopped => {}
            },
            Ok(Command::Seek(position)) => {
                // rodio queues a seek issued with nothing loaded and applies
                // it to the next source, so only seek while a track is loaded.
                if state != PlaybackState::Stopped
                    && let Err(err) = player.try_seek(position)
                {
                    pending.push(Event::Error(format!("seek failed: {err}")));
                }
            }
            Ok(Command::SetVolume(new_volume)) => {
                volume = new_volume;
                player.set_volume(volume);
            }
            Ok(Command::Stop) => {
                player = fresh_player(&sink, volume);
                state = PlaybackState::Stopped;
                path = None;
                duration = None;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if state == PlaybackState::Playing && player.empty() {
            state = PlaybackState::Stopped;
            pending.push(Event::TrackEnded);
        }

        let position = if state == PlaybackState::Stopped {
            Duration::ZERO
        } else {
            player.get_pos()
        };
        *status.lock().unwrap_or_else(PoisonError::into_inner) = PlaybackStatus {
            state,
            path: path.clone(),
            position,
            duration,
            volume,
            last_error: last_error.clone(),
        };

        for event in pending {
            let _ = events.send(event);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/alpha.m4a")
    }

    #[test]
    fn status_defaults_to_stopped() {
        let status = PlaybackStatus::default();
        assert_eq!(status.state, PlaybackState::Stopped);
        assert!((status.volume - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn plays_fixture_to_the_end_when_a_device_exists() {
        let Ok(player) = Player::new() else {
            eprintln!("no audio device; skipping playback smoke test");
            return;
        };
        player.set_volume(0.0);
        player.play(&fixture());

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut ended = false;
        while Instant::now() < deadline {
            if let Some(Event::TrackEnded) = player.try_recv_event() {
                ended = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(ended, "TrackEnded was not reported within 5s");
        let status = player.status();
        assert_eq!(status.state, PlaybackState::Stopped);
        assert!(
            status
                .duration
                .is_some_and(|d| d >= Duration::from_millis(800))
        );
    }

    #[test]
    fn missing_file_reports_error_event() {
        let Ok(player) = Player::new() else {
            return;
        };
        player.play(Path::new("/nonexistent/file.mp3"));
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut got_error = false;
        while Instant::now() < deadline {
            if let Some(Event::Error(_)) = player.try_recv_event() {
                got_error = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(got_error);
        assert!(player.status().last_error.is_some());
    }

    #[test]
    fn seek_while_stopped_is_ignored() {
        let Ok(player) = Player::new() else {
            return;
        };
        player.set_volume(0.0);
        player.seek(Duration::from_millis(900));
        player.play(&fixture());

        let deadline = Instant::now() + Duration::from_millis(500);
        let mut observed = None;
        while Instant::now() < deadline {
            let status = player.status();
            if status.state == PlaybackState::Playing {
                observed = Some(status.position);
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let position = observed.expect("never observed Playing within 500ms");
        assert!(
            position < Duration::from_millis(300),
            "pending seek was applied: position {position:?}"
        );
    }

    #[test]
    fn stop_resets_status() {
        let Ok(player) = Player::new() else {
            return;
        };
        player.set_volume(0.0);
        player.play(&fixture());
        wait_for(&player, PlaybackState::Playing, Duration::from_secs(2));
        player.stop();
        wait_for(&player, PlaybackState::Stopped, Duration::from_secs(1));
        let status = player.status();
        assert!(status.path.is_none());
        assert_eq!(status.position, Duration::ZERO);
    }

    fn wait_for(player: &Player, want: PlaybackState, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if player.status().state == want {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("state {want:?} not reached within {timeout:?}");
    }
}
