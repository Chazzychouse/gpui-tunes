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
use rodio::stream::DeviceSinkBuilder;
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
        self.send(Command::SetVolume(volume.clamp(0.0, 1.0)));
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

fn audio_thread(
    commands: &Receiver<Command>,
    events: &Sender<Event>,
    status: &Mutex<PlaybackStatus>,
    ready: &Sender<Result<(), PlayerError>>,
) {
    let sink = match DeviceSinkBuilder::open_default_sink() {
        Ok(sink) => sink,
        Err(err) => {
            let _ = ready.send(Err(PlayerError::Output(err.to_string())));
            return;
        }
    };
    let player = rodio::Player::connect_new(sink.mixer());
    let _ = ready.send(Ok(()));

    let update = |f: &dyn Fn(&mut PlaybackStatus)| {
        f(&mut status.lock().unwrap_or_else(PoisonError::into_inner));
    };
    let mut state = PlaybackState::Stopped;

    loop {
        match commands.recv_timeout(Duration::from_millis(100)) {
            Ok(Command::Play(path)) => {
                player.clear();
                match open_source(&path) {
                    Ok((source, duration)) => {
                        player.append(source);
                        player.play();
                        state = PlaybackState::Playing;
                        update(&|s| {
                            s.path = Some(path.clone());
                            s.duration = duration;
                            s.position = Duration::ZERO;
                            s.last_error = None;
                        });
                    }
                    Err(message) => {
                        state = PlaybackState::Stopped;
                        tracing::warn!(%message, "could not play");
                        update(&|s| {
                            s.path = None;
                            s.duration = None;
                            s.last_error = Some(message.clone());
                        });
                        let _ = events.send(Event::Error(message));
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
                if let Err(err) = player.try_seek(position) {
                    let _ = events.send(Event::Error(format!("seek failed: {err}")));
                }
            }
            Ok(Command::SetVolume(volume)) => player.set_volume(volume),
            Ok(Command::Stop) => {
                player.clear();
                state = PlaybackState::Stopped;
                update(&|s| {
                    s.path = None;
                    s.duration = None;
                    s.position = Duration::ZERO;
                });
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if state == PlaybackState::Playing && player.empty() {
            state = PlaybackState::Stopped;
            let _ = events.send(Event::TrackEnded);
        }

        let position = player.get_pos();
        let volume = player.volume();
        update(&|s| {
            s.state = state;
            s.position = position;
            s.volume = volume;
        });
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
}
