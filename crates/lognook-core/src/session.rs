use crate::{
    LogNookEngine, LogNookError,
    model::{Marker, PlaybackState, RecordingInfo},
    playback::PlaybackHandle,
    recording_view::{
        add_recording_marker, delete_recording_marker, inspect_recording, update_recording_marker,
    },
};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

impl LogNookEngine {
    pub fn load_recording(&mut self, path: &Path) -> Result<RecordingInfo, LogNookError> {
        if self.pipeline.is_recording() {
            return Err(LogNookError::new("Stop recording before loading a session"));
        }
        let info = inspect_recording(path)?;
        if info.sample_count == 0 {
            return Err(LogNookError::new("Recording contains no samples"));
        }
        self.stop_playback();
        self.session_id = self.session_id.wrapping_add(1).max(1);
        self.session_path = Some(path.to_path_buf());
        self.pipeline.set_last_recording_path(path.to_path_buf());
        self.playback_state = PlaybackState {
            session_id: self.session_id,
            position_ns: 0,
            duration_ns: info.duration_ns,
            speed: 1.0,
            playing: false,
        };
        Ok(info)
    }

    pub fn start_replay(&mut self, speed: f64) -> Result<(), LogNookError> {
        validate_speed(speed)?;
        if self.pipeline.is_recording() {
            return Err(LogNookError::new("Stop recording before replay"));
        }
        let path = self
            .session_path
            .clone()
            .ok_or_else(|| LogNookError::new("Load a recording before replay"))?;
        self.disconnect_if_connected()?;
        if self.playback.is_some() {
            if self.playback_state()?.position_ns >= self.playback_state()?.duration_ns {
                self.seek_replay(0)?;
            }
            self.set_replay_speed(speed)?;
            return self.resume_replay();
        }
        let mut state = self.playback_state.clone();
        if state.position_ns >= state.duration_ns {
            state.position_ns = 0;
        }
        state.speed = speed;
        state.playing = true;
        self.playback_state = state.clone();
        self.pipeline.playback_started();
        self.playback = Some(PlaybackHandle::start(path, state, self.pipeline.sender()));
        Ok(())
    }

    pub fn pause_replay(&mut self) -> Result<(), LogNookError> {
        if let Some(playback) = &self.playback {
            self.playback_state = playback.pause()?;
        } else {
            self.playback_state.playing = false;
        }
        Ok(())
    }

    pub fn resume_replay(&mut self) -> Result<(), LogNookError> {
        if self.playback.is_none() {
            return self.start_replay(self.playback_state.speed);
        }
        self.playback_state = self.playback.as_ref().unwrap().resume()?;
        Ok(())
    }

    pub fn seek_replay(&mut self, position_ns: i64) -> Result<(), LogNookError> {
        if let Some(playback) = &self.playback {
            self.playback_state = playback.seek(position_ns)?;
        } else {
            self.playback_state.position_ns = position_ns.clamp(0, self.playback_state.duration_ns);
        }
        Ok(())
    }

    pub fn set_replay_speed(&mut self, speed: f64) -> Result<(), LogNookError> {
        validate_speed(speed)?;
        if let Some(playback) = &self.playback {
            self.playback_state = playback.set_speed(speed)?;
        } else {
            self.playback_state.speed = speed;
        }
        Ok(())
    }

    pub fn playback_state(&mut self) -> Result<PlaybackState, LogNookError> {
        if let Some(playback) = &self.playback {
            self.playback_state = playback.state()?;
        }
        Ok(self.playback_state.clone())
    }

    pub fn add_playback_marker(&mut self, label: &str, note: &str) -> Result<Marker, LogNookError> {
        let path = self.active_session_path()?;
        let was_playing = self.pause_for_marker()?;
        let position_ns = self.playback_state()?.position_ns;
        let marker = Marker {
            id: None,
            elapsed_ns: position_ns,
            wall_time_unix_ns: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .min(i64::MAX as u128) as i64,
            label: label.into(),
            note: note.into(),
        };
        self.finish_marker_change(add_recording_marker(&path, &marker), was_playing)
    }

    pub fn update_playback_marker(
        &mut self,
        id: i64,
        label: &str,
        note: &str,
    ) -> Result<Marker, LogNookError> {
        let path = self.active_session_path()?;
        let was_playing = self.pause_for_marker()?;
        self.finish_marker_change(update_recording_marker(&path, id, label, note), was_playing)
    }

    pub fn delete_playback_marker(&mut self, id: i64) -> Result<(), LogNookError> {
        let path = self.active_session_path()?;
        let was_playing = self.pause_for_marker()?;
        let result = delete_recording_marker(&path, id);
        if result.is_ok() {
            let duration_ns = inspect_recording(&path)?.duration_ns;
            self.playback_state.duration_ns = duration_ns;
            self.playback_state.position_ns = self.playback_state.position_ns.min(duration_ns);
            if let Some(playback) = &self.playback {
                self.playback_state = playback.set_duration(duration_ns)?;
            }
        }
        self.finish_marker_change(result, was_playing)
    }

    pub fn active_session_path(&self) -> Result<std::path::PathBuf, LogNookError> {
        self.session_path
            .clone()
            .ok_or_else(|| LogNookError::new("No session is loaded"))
    }

    fn pause_for_marker(&mut self) -> Result<bool, LogNookError> {
        let was_playing = self.playback_state()?.playing;
        if was_playing {
            self.pause_replay()?;
        }
        Ok(was_playing)
    }

    fn finish_marker_change<T>(
        &mut self,
        result: Result<T, LogNookError>,
        was_playing: bool,
    ) -> Result<T, LogNookError> {
        if was_playing {
            let resume = self.resume_replay();
            if result.is_ok() {
                resume?;
            }
        }
        result
    }

    pub fn replay_recording(&mut self, path: &Path, speed: f64) -> Result<(), LogNookError> {
        self.load_recording(path)?;
        self.start_replay(speed)
    }

    pub fn replay_last(&mut self, speed: f64) -> Result<(), LogNookError> {
        let path = self
            .last_recording_path()
            .ok_or_else(|| LogNookError::new("No completed recording is available"))?;
        self.replay_recording(&path, speed)
    }

    pub fn stop_replay(&mut self) {
        self.stop_playback();
    }

    pub fn close_session(&mut self) {
        self.stop_playback();
        self.session_path = None;
        self.session_id = self.session_id.wrapping_add(1).max(1);
        self.playback_state = PlaybackState {
            session_id: self.session_id,
            position_ns: 0,
            duration_ns: 0,
            speed: 1.0,
            playing: false,
        };
    }
}

fn validate_speed(speed: f64) -> Result<(), LogNookError> {
    if speed.is_finite() && (0.1..=32.0).contains(&speed) {
        Ok(())
    } else {
        Err(LogNookError::new(
            "Replay speed must be between 0.1x and 32x",
        ))
    }
}
