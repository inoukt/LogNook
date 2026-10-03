use crate::{
    LogNookError,
    model::{PlaybackState, SampleOrigin},
    pipeline::SampleSender,
    recording::{EventCursor, RecordedEvent, visit_recording_events_from},
};
use std::{
    collections::HashMap,
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
    Duration(i64, Sender<PlaybackState>),
    Stop(Sender<()>),
}

enum Reply {
    State(Sender<PlaybackState>),
    Stop(Sender<()>),
}

struct ReplayClock {
    position_ns: i64,
    started: Instant,
}

impl ReplayClock {
    fn new(position_ns: i64) -> Self {
        Self {
            position_ns,
            started: Instant::now(),
        }
    }

    fn reset(&mut self, position_ns: i64) {
        self.position_ns = position_ns;
        self.started = Instant::now();
    }

    fn deadline(&self, target_ns: i64, speed: f64) -> Instant {
        let delta_ns = (target_ns - self.position_ns).max(0) as f64 / speed;
        self.started + Duration::from_nanos(delta_ns.min(u64::MAX as f64) as u64)
    }

    fn position_at(&self, now: Instant, speed: f64) -> i64 {
        let elapsed_ns = now.saturating_duration_since(self.started).as_nanos() as f64 * speed;
        self.position_ns.saturating_add(elapsed_ns.min(i64::MAX as f64) as i64)
    }
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

    pub(crate) fn set_duration(&self, duration_ns: i64) -> Result<PlaybackState, LogNookError> {
        self.change(|reply| Command::Duration(duration_ns, reply))
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
    let initial_state = snapshot(&state);
    let session_id = initial_state.session_id;
    let mut pending_samples = HashMap::new();
    let mut last_state_emit = Instant::now();
    let mut last_sample_emit = Instant::now();
    let mut clock = ReplayClock::new(initial_state.position_ns);
    output.send_playback_state(snapshot(&state));
    loop {
        if !snapshot(&state).playing {
            match commands.recv() {
                Ok(command) => {
                    let (reply, stop) = apply(command, &state, &mut cursor);
                    clock.reset(snapshot(&state).position_ns);
                    respond(reply, &state);
                    output.send_playback_state(snapshot(&state));
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
                    &mut last_state_emit,
                    &mut clock,
                    &output,
                ) {
                    return Ok(false);
                }
                set_position(&state, elapsed_ns);
                match event {
                    RecordedEvent::Sample(mut sample) => {
                        sample.origin = SampleOrigin::Replay;
                        pending_samples.insert(sample.signal_id.clone(), sample);
                        if !pending_samples.is_empty()
                            && last_sample_emit.elapsed() >= Duration::from_millis(50)
                        {
                            flush_samples(
                                &mut pending_samples,
                                session_id,
                                &output,
                                &mut last_sample_emit,
                            )?;
                        }
                    }
                    RecordedEvent::Marker(marker) => {
                        flush_samples(
                            &mut pending_samples,
                            session_id,
                            &output,
                            &mut last_sample_emit,
                        )?;
                        output.send_playback_marker(session_id, marker)?;
                    }
                }
                cursor = Some(event_cursor);
                Ok(true)
            },
        );

        if let Err(error) = flush_samples(
            &mut pending_samples,
            session_id,
            &output,
            &mut last_sample_emit,
        ) {
            let _ = output.send_error(error.to_string());
        }

        if let Some(reply) = pending_reply {
            let stopping = matches!(&reply, Reply::Stop(_));
            respond(Some(reply), &state);
            output.send_playback_state(snapshot(&state));
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
            }
        }
        if !snapshot(&state).playing {
            continue;
        }
    }
}

fn wait_until(
    target_ns: i64,
    commands: &Receiver<Command>,
    state: &Arc<Mutex<PlaybackState>>,
    cursor: &mut Option<EventCursor>,
    pending_reply: &mut Option<Reply>,
    last_state_emit: &mut Instant,
    clock: &mut ReplayClock,
    output: &SampleSender,
) -> bool {
    loop {
        let current = snapshot(state);
        let deadline = clock.deadline(target_ns, current.speed);
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            if let Ok(command) = commands.try_recv() {
                set_position(state, clock.position_at(Instant::now(), current.speed).min(target_ns));
                let (reply, _) = apply(command, state, cursor);
                clock.reset(snapshot(state).position_ns);
                *pending_reply = reply;
                return false;
            }
            set_position(state, target_ns);
            emit_playback_state_if_due(state, last_state_emit, output);
            return true;
        }
        let timeout = remaining.min(Duration::from_millis(50));
        match commands.recv_timeout(timeout) {
            Ok(command) => {
                set_position(state, clock.position_at(Instant::now(), current.speed).min(target_ns));
                let (reply, _) = apply(command, state, cursor);
                clock.reset(snapshot(state).position_ns);
                *pending_reply = reply;
                return false;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                set_position(state, clock.position_at(Instant::now(), current.speed).min(target_ns));
                emit_playback_state_if_due(state, last_state_emit, output);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return false,
        }
    }
}

fn emit_playback_state_if_due(
    state: &Arc<Mutex<PlaybackState>>,
    last_state_emit: &mut Instant,
    output: &SampleSender,
) {
    if last_state_emit.elapsed() >= Duration::from_millis(50) {
        output.send_playback_state(snapshot(state));
        *last_state_emit = Instant::now();
    }
}

fn flush_samples(
    pending: &mut HashMap<String, crate::Sample>,
    session_id: u64,
    output: &SampleSender,
    last_sample_emit: &mut Instant,
) -> Result<(), LogNookError> {
    for sample in pending.drain().map(|(_, sample)| sample) {
        output.send_playback_sample(session_id, sample)?;
    }
    *last_sample_emit = Instant::now();
    Ok(())
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
        Command::Duration(duration_ns, reply) => {
            update(state, |state| {
                state.duration_ns = duration_ns.max(0);
                state.position_ns = state.position_ns.min(state.duration_ns);
            });
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
