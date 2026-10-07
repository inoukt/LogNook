use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const TRACE_PAGE_MAX: usize = 500;
const DEFAULT_RING_CAPACITY: usize = 10_000;
const DEFAULT_QUEUE_CAPACITY: usize = 8_192;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommunicationDirection {
    Tx,
    Rx,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct CommunicationTraceEvent {
    pub sequence: u64,
    pub timestamp_unix_ns: u64,
    pub elapsed_ns: Option<u64>,
    pub adapter_id: String,
    pub operation: String,
    pub direction: CommunicationDirection,
    pub route: Option<String>,
    pub bytes_hex: Option<String>,
    pub outcome: String,
    pub raw_bytes_unavailable: Option<String>,
}

impl CommunicationTraceEvent {
    pub fn new(
        adapter_id: impl Into<String>,
        operation: impl Into<String>,
        direction: CommunicationDirection,
    ) -> Self {
        let timestamp_unix_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .min(u64::MAX as u128) as u64;
        Self {
            sequence: 0,
            timestamp_unix_ns,
            elapsed_ns: None,
            adapter_id: adapter_id.into(),
            operation: operation.into(),
            direction,
            route: None,
            bytes_hex: None,
            outcome: "unknown".into(),
            raw_bytes_unavailable: None,
        }
    }
}

pub fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 0x0F) as usize] as char);
    }
    text
}

#[derive(Clone)]
pub struct CommunicationSink {
    sender: SyncSender<CommunicationTraceEvent>,
    next_sequence: Arc<AtomicU64>,
    dropped_events: Arc<AtomicU64>,
}

impl CommunicationSink {
    pub fn try_record(&self, mut event: CommunicationTraceEvent) -> bool {
        event.sequence = self
            .next_sequence
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        match self.sender.try_send(event) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.dropped_events.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    pub fn dropped_events(&self) -> u64 {
        self.dropped_events.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct TraceCaptureStatus {
    pub capturing: bool,
    pub path: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct TracePage {
    pub events: Vec<CommunicationTraceEvent>,
    pub next_sequence: u64,
    pub gaps: u64,
    pub dropped_events: u64,
    pub evicted_events: u64,
    pub capture: TraceCaptureStatus,
}

struct TraceState {
    events: VecDeque<CommunicationTraceEvent>,
    ring_capacity: usize,
    evicted_events: u64,
    capture: TraceCaptureStatus,
}

enum Control {
    Start(PathBuf, mpsc::Sender<Result<(), String>>),
    Stop(mpsc::Sender<TraceCaptureStatus>),
    Flush(mpsc::Sender<()>),
    Shutdown,
}

pub struct CommunicationTraceStore {
    sink: CommunicationSink,
    state: Arc<Mutex<TraceState>>,
    controls: mpsc::Sender<Control>,
    worker: Option<JoinHandle<()>>,
}

impl Default for CommunicationTraceStore {
    fn default() -> Self {
        Self::with_limits(DEFAULT_RING_CAPACITY, DEFAULT_QUEUE_CAPACITY)
    }
}

impl CommunicationTraceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sink(&self) -> CommunicationSink {
        self.sink.clone()
    }

    pub fn with_limits(ring_capacity: usize, queue_capacity: usize) -> Self {
        let (events, receiver) = mpsc::sync_channel(queue_capacity.max(1));
        let (controls, control_receiver) = mpsc::channel();
        let sequence = Arc::new(AtomicU64::new(0));
        let dropped_events = Arc::new(AtomicU64::new(0));
        let state = Arc::new(Mutex::new(TraceState {
            events: VecDeque::new(),
            ring_capacity: ring_capacity.max(1),
            evicted_events: 0,
            capture: TraceCaptureStatus::default(),
        }));
        let worker_state = Arc::clone(&state);
        let worker_sequence = Arc::clone(&sequence);
        let worker = thread::spawn(move || {
            run_writer(receiver, control_receiver, worker_state, worker_sequence)
        });
        Self {
            sink: CommunicationSink {
                sender: events,
                next_sequence: sequence,
                dropped_events,
            },
            state,
            controls,
            worker: Some(worker),
        }
    }

    pub fn get_trace(&self, after_sequence: u64, limit: usize) -> Result<TracePage, String> {
        self.flush()?;
        let state = self
            .state
            .lock()
            .map_err(|_| "Communication trace is unavailable".to_owned())?;
        let limit = limit.clamp(1, TRACE_PAGE_MAX);
        let events = state
            .events
            .iter()
            .filter(|event| event.sequence > after_sequence)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let mut gaps = 0u64;
        let mut expected = after_sequence.saturating_add(1);
        for event in &events {
            if event.sequence > expected {
                gaps = gaps.saturating_add(event.sequence - expected);
            }
            expected = event.sequence.saturating_add(1);
        }
        let current_sequence = self.sink.next_sequence.load(Ordering::Relaxed);
        let next_sequence = events
            .last()
            .map_or(current_sequence.max(after_sequence), |event| event.sequence);
        if events.is_empty() && current_sequence > after_sequence {
            gaps = current_sequence - after_sequence;
        }
        Ok(TracePage {
            events,
            next_sequence,
            gaps,
            dropped_events: self.sink.dropped_events(),
            evicted_events: state.evicted_events,
            capture: state.capture.clone(),
        })
    }

    pub fn start_capture(&self, path: &std::path::Path) -> Result<(), String> {
        let (reply, result) = mpsc::channel();
        self.controls
            .send(Control::Start(path.to_owned(), reply))
            .map_err(|error| error.to_string())?;
        result.recv().map_err(|error| error.to_string())?
    }

    pub fn stop_capture(&self) -> Result<TraceCaptureStatus, String> {
        let (reply, result) = mpsc::channel();
        self.controls
            .send(Control::Stop(reply))
            .map_err(|error| error.to_string())?;
        result.recv().map_err(|error| error.to_string())
    }

    pub fn capture_status(&self) -> TraceCaptureStatus {
        self.state
            .lock()
            .map(|state| state.capture.clone())
            .unwrap_or_else(|_| TraceCaptureStatus {
                capturing: false,
                path: None,
                error: Some("Communication trace state is unavailable".into()),
            })
    }

    pub fn gap_count(&self) -> u64 {
        let evicted = self
            .state
            .lock()
            .map(|state| state.evicted_events)
            .unwrap_or_default();
        self.sink.dropped_events().saturating_add(evicted)
    }

    fn flush(&self) -> Result<(), String> {
        let (reply, result) = mpsc::channel();
        self.controls
            .send(Control::Flush(reply))
            .map_err(|error| error.to_string())?;
        result.recv().map_err(|error| error.to_string())
    }
}

impl Drop for CommunicationTraceStore {
    fn drop(&mut self) {
        let _ = self.controls.send(Control::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_writer(
    receiver: Receiver<CommunicationTraceEvent>,
    controls: Receiver<Control>,
    state: Arc<Mutex<TraceState>>,
    sequence: Arc<AtomicU64>,
) {
    let mut file: Option<File> = None;
    let mut capture_start_sequence = u64::MAX;
    loop {
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(event) => write_event(event, &mut file, capture_start_sequence, &state),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        while let Ok(control) = controls.try_recv() {
            match control {
                Control::Start(path, reply) => {
                    if file.is_some() {
                        let _ = reply.send(Err(
                            "A communication trace capture is already running".into()
                        ));
                        continue;
                    }
                    let result = start_file(&path);
                    match result {
                        Ok(opened) => {
                            capture_start_sequence =
                                sequence.load(Ordering::Relaxed).saturating_add(1);
                            file = Some(opened);
                            if let Ok(mut state) = state.lock() {
                                state.capture = TraceCaptureStatus {
                                    capturing: true,
                                    path: Some(path.to_string_lossy().into_owned()),
                                    error: None,
                                };
                            }
                            let _ = reply.send(Ok(()));
                        }
                        Err(error) => {
                            if let Ok(mut state) = state.lock() {
                                state.capture.error = Some(error.clone());
                            }
                            let _ = reply.send(Err(error));
                        }
                    }
                }
                Control::Stop(reply) => {
                    drain_events(&receiver, &mut file, capture_start_sequence, &state);
                    if let Some(mut file) = file.take() {
                        if let Err(error) = file.flush() {
                            if let Ok(mut state) = state.lock() {
                                state.capture.error = Some(error.to_string());
                            }
                        }
                    }
                    if let Ok(mut state) = state.lock() {
                        state.capture.capturing = false;
                    }
                    let status = state
                        .lock()
                        .map(|state| state.capture.clone())
                        .unwrap_or_default();
                    let _ = reply.send(status);
                    capture_start_sequence = u64::MAX;
                }
                Control::Flush(reply) => {
                    drain_events(&receiver, &mut file, capture_start_sequence, &state);
                    if let Some(file) = &mut file {
                        let _ = file.flush();
                    }
                    let _ = reply.send(());
                }
                Control::Shutdown => {
                    drain_events(&receiver, &mut file, capture_start_sequence, &state);
                    if let Some(file) = &mut file {
                        let _ = file.flush();
                    }
                    return;
                }
            }
        }
    }
}

fn start_file(path: &std::path::Path) -> Result<File, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(|error| error.to_string())
}

fn drain_events(
    receiver: &Receiver<CommunicationTraceEvent>,
    file: &mut Option<File>,
    start_sequence: u64,
    state: &Arc<Mutex<TraceState>>,
) {
    while let Ok(event) = receiver.try_recv() {
        write_event(event, file, start_sequence, state);
    }
}

fn write_event(
    event: CommunicationTraceEvent,
    file: &mut Option<File>,
    start_sequence: u64,
    state: &Arc<Mutex<TraceState>>,
) {
    if let Ok(mut state) = state.lock() {
        state.events.push_back(event.clone());
        while state.events.len() > state.ring_capacity {
            state.events.pop_front();
            state.evicted_events = state.evicted_events.saturating_add(1);
        }
    }
    if event.sequence < start_sequence {
        return;
    }
    let Some(file_ref) = file.as_mut() else {
        return;
    };
    let result = serde_json::to_vec(&event)
        .map_err(io::Error::other)
        .and_then(|bytes| file_ref.write_all(&bytes))
        .and_then(|()| file_ref.write_all(b"\n"));
    if let Err(error) = result {
        *file = None;
        if let Ok(mut state) = state.lock() {
            state.capture.capturing = false;
            state.capture.error = Some(error.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::{Arc, atomic::AtomicU64, mpsc::sync_channel},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn event() -> CommunicationTraceEvent {
        CommunicationTraceEvent {
            sequence: 0,
            timestamp_unix_ns: 1,
            elapsed_ns: Some(2),
            adapter_id: "sx".into(),
            operation: "request".into(),
            direction: CommunicationDirection::Tx,
            route: Some("7E0".into()),
            bytes_hex: Some("010C".into()),
            outcome: "sent".into(),
            raw_bytes_unavailable: None,
        }
    }

    fn temp_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("lognook-trace-{label}-{nonce}.jsonl"))
    }

    #[test]
    fn trace_cursor_returns_ordered_events() {
        let store = CommunicationTraceStore::with_limits(8, 8);
        let sink = store.sink();
        for _ in 0..3 {
            sink.try_record(event());
        }
        let page = store.get_trace(0, 10).unwrap();
        assert_eq!(
            page.events
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
    }

    #[test]
    fn trace_ring_reports_eviction() {
        let store = CommunicationTraceStore::with_limits(2, 8);
        let sink = store.sink();
        for _ in 0..3 {
            sink.try_record(event());
        }
        let page = store.get_trace(0, 10).unwrap();
        assert_eq!(page.events.len(), 2);
        assert_eq!(page.gaps, 1);
        assert_eq!(page.evicted_events, 1);
        assert_eq!(store.gap_count(), 1);
    }

    #[test]
    fn trace_slow_subscriber_reports_gap_without_blocking() {
        let (sender, _receiver) = sync_channel(1);
        let sink = CommunicationSink {
            sender,
            next_sequence: Arc::new(AtomicU64::new(0)),
            dropped_events: Arc::new(AtomicU64::new(0)),
        };
        assert!(sink.try_record(event()));
        assert!(!sink.try_record(event()));
        assert_eq!(sink.dropped_events(), 1);
    }

    #[test]
    fn trace_export_round_trips_raw_bytes() {
        let store = CommunicationTraceStore::with_limits(8, 8);
        let path = temp_path("roundtrip");
        store.start_capture(&path).unwrap();
        let sink = store.sink();
        sink.try_record(event());
        store.stop_capture().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let recorded: CommunicationTraceEvent =
            serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(recorded.bytes_hex.as_deref(), Some("010C"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn trace_storage_error_is_reported() {
        let store = CommunicationTraceStore::with_limits(8, 8);
        let parent = temp_path("not-directory");
        fs::write(&parent, b"occupied").unwrap();
        let path = parent.join("capture.jsonl");
        assert!(store.start_capture(&path).is_err());
        assert!(store.capture_status().error.is_some());
        let _ = fs::remove_file(parent);
    }
}
