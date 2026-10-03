use crate::{
    LogNookError,
    model::{PlaybackState, SampleOrigin},
    pipeline::SampleSender,
    recording::{EventCursor, RecordedEvent, visit_recording_events_from},
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

enum Command {
    Pause(Sender<PlaybackState>),
    Resume(Sender<PlaybackState>),
    Seek(i64, Sender<PlaybackState>),
    Speed(f64, Sender<PlaybackState>),
    Stop(Sender<()>),
}

enum Reply {
    State(Sender<PlaybackState>),
    Stop(Sender<()>),
}

pub(crate) struct PlaybackHandle {
    commands: Sender<Command>,
    state: Arc<Mutex<PlaybackState>>,
    worker: Option<JoinHandle<()>>,
}

impl PlaybackHandle {
    pub(crate) fn start(path: PathBuf, state: PlaybackState, output: SampleSender) -> Self {
        let shared_state = Arc::new(Mutex::new(state));
        let (commands, receiver) = mpsc::channel();
        let worker_state = shared_state.clone();
        let worker = thread::spawn(move || run(path, receiver, worker_state, output));
        Self {
            commands,
            state: shared_state,
            worker: Some(worker),
        }
    }

    pub(crate) fn state(&self) -> Result<PlaybackState, LogNookError> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| LogNookError::new("Playback state is unavailable"))
    }

    fn change(
        &self,
        command: impl FnOnce(Sender<PlaybackState>) -> Command,
    ) -> Result<PlaybackState, LogNookError> {
        let (reply, response) = mpsc::channel();
        self.commands
            .send(command(reply))
            .map_err(|_| LogNookError::new("Playback has stopped"))?;
        response
            .recv()
            .map_err(|_| LogNookError::new("Playback did not acknowledge the command"))
    }

    pub(crate) fn pause(&self) -> Result<PlaybackState, LogNookError> {
        self.change(Command::Pause)
    }

    pub(crate) fn resume(&self) -> Result<PlaybackState, LogNookError> {
        self.change(Command::Resume)
    }

    pub(crate) fn seek(&self, position_ns: i64) -> Result<PlaybackState, LogNookError> {
        self.change(|reply| Command::Seek(position_ns, reply))
    }

    pub(crate) fn set_speed(&self, speed: f64) -> Result<PlaybackState, LogNookError> {
        self.change(|reply| Command::Speed(speed, reply))
    }

    pub(crate) fn stop(&mut self) {
        let (reply, response) = mpsc::channel();
        let _ = self.commands.send(Command::Stop(reply));
        let _ = response.recv();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for PlaybackHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(
    path: PathBuf,
    commands: Receiver<Command>,
    state: Arc<Mutex<PlaybackState>>,
    output: SampleSender,
) {
    let mut cursor: Option<EventCursor> = None;
    output.send_playback_state(snapshot(&state));
    loop {
        if !snapshot(&state).playing {
            match commands.recv() {
                Ok(command) => {
                    let (reply, stop) = apply(command, &state, &mut cursor);
                    respond(reply, &state);
                    if stop {
                        break;
                    }
                    continue;
                }
                Err(_) => break,
            }
        }

        let current = snapshot(&state);
        let mut pending_reply = None;
        let result = visit_recording_events_from(
            &path,
            current.position_ns,
            cursor,
            |event, event_cursor| {
                let elapsed_ns = match &event {
                    RecordedEvent::Sample(sample) => sample.elapsed_ns,
                    RecordedEvent::Marker(marker) => marker.elapsed_ns,
                };
                if !wait_until(
                    elapsed_ns,
                    &commands,
                    &state,
                    &mut cursor,
                    &mut pending_reply,
                    &output,
                ) {
                    return Ok(false);
                }
                set_position(&state, elapsed_ns);
                output.send_playback_state(snapshot(&state));
                match event {
                    RecordedEvent::Sample(mut sample) => {
                        sample.origin = SampleOrigin::Replay;
                        output.send_sample(sample)?;
                    }
                    RecordedEvent::Marker(marker) => output.send_marker(marker)?,
                }
                cursor = Some(event_cursor);
                Ok(true)
            },
        );

        if let Some(reply) = pending_reply {
            let stopping = matches!(&reply, Reply::Stop(_));
            respond(Some(reply), &state);
            if stopping {
                break;
            }
        }
        match result {
            Ok(true) => {
                let mut current = snapshot(&state);
                current.position_ns = current.duration_ns;
                current.playing = false;
                store(&state, current.clone());
                output.send_playback_state(current);
                output.playback_finished();
            }
            Ok(false) => {}
            Err(error) => {
                let _ = output.send_error(error.to_string());
                let mut current = snapshot(&state);
                current.playing = false;
                store(&state, current.clone());
                output.send_playback_state(current);
                output.playback_finished();
            }
        }
        if !snapshot(&state).playing {
            continue;
        }
    }
    output.playback_finished();
}

fn wait_until(
    target_ns: i64,
    commands: &Receiver<Command>,
    state: &Arc<Mutex<PlaybackState>>,
    cursor: &mut Option<EventCursor>,
    pending_reply: &mut Option<Reply>,
    output: &SampleSender,
) -> bool {
    let mut last_tick = Instant::now();
    let mut last_emit = last_tick;
    loop {
        let current = snapshot(state);
        if current.position_ns >= target_ns {
            return true;
        }
        let remaining = (target_ns - current.position_ns) as f64 / current.speed;
        let timeout = Duration::from_nanos(remaining.max(1.0).min(50_000_000.0) as u64);
        match commands.recv_timeout(timeout) {
            Ok(command) => {
                advance(state, target_ns, last_tick.elapsed());
                let (reply, _) = apply(command, state, cursor);
                *pending_reply = reply;
                return false;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let elapsed = last_tick.elapsed();
                advance(state, target_ns, elapsed);
                last_tick = Instant::now();
                if last_emit.elapsed() >= Duration::from_millis(50) {
                    output.send_playback_state(snapshot(state));
                    last_emit = Instant::now();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return false,
        }
    }
}

fn advance(state: &Arc<Mutex<PlaybackState>>, target_ns: i64, elapsed: Duration) {
    if let Ok(mut state) = state.lock() {
        let virtual_ns = elapsed.as_nanos() as f64 * state.speed;
        state.position_ns = state
            .position_ns
            .saturating_add(virtual_ns.min(i64::MAX as f64) as i64)
            .min(target_ns);
    }
}

fn apply(
    command: Command,
    state: &Arc<Mutex<PlaybackState>>,
    cursor: &mut Option<EventCursor>,
) -> (Option<Reply>, bool) {
    match command {
        Command::Pause(reply) => {
            update(state, |state| state.playing = false);
            (Some(Reply::State(reply)), false)
        }
        Command::Resume(reply) => {
            update(state, |state| state.playing = true);
            (Some(Reply::State(reply)), false)
        }
        Command::Seek(position_ns, reply) => {
            update(state, |state| {
                state.position_ns = position_ns.clamp(0, state.duration_ns)
            });
            *cursor = None;
            (Some(Reply::State(reply)), false)
        }
        Command::Speed(speed, reply) => {
            update(state, |state| state.speed = speed);
            (Some(Reply::State(reply)), false)
        }
        Command::Stop(reply) => {
            update(state, |state| state.playing = false);
            (Some(Reply::Stop(reply)), true)
        }
    }
}

fn update(state: &Arc<Mutex<PlaybackState>>, change: impl FnOnce(&mut PlaybackState)) {
    if let Ok(mut state) = state.lock() {
        change(&mut state);
    }
}

fn set_position(state: &Arc<Mutex<PlaybackState>>, position_ns: i64) {
    update(state, |state| state.position_ns = position_ns);
}

fn snapshot(state: &Arc<Mutex<PlaybackState>>) -> PlaybackState {
    state.lock().expect("playback state lock poisoned").clone()
}

fn store(state: &Arc<Mutex<PlaybackState>>, value: PlaybackState) {
    if let Ok(mut state) = state.lock() {
        *state = value;
    }
}

fn respond(reply: Option<Reply>, state: &Arc<Mutex<PlaybackState>>) {
    match reply {
        Some(Reply::State(reply)) => {
            let _ = reply.send(snapshot(state));
        }
        Some(Reply::Stop(reply)) => {
            let _ = reply.send(());
        }
        None => {}
    }
}
