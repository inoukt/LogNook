use lognook_core::{
    DiagnosticPhase, DiagnosticProgress, DiagnosticSink, EngineEvent, SampleOrigin, SampleQuality,
    diagnostic_error_detail,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const QUEUE_SIZE: usize = 1024;
const HISTORY_SIZE: usize = 500;
const MAX_SIGNAL_SUMMARIES: usize = 500;
const ROTATION_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct SignalSummary {
    pub signal_id: String,
    #[serde(default)]
    pub requested: Option<bool>,
    #[serde(default)]
    pub supported: Option<bool>,
    #[serde(default)]
    pub attempt_count: u64,
    pub success_count: u64,
    #[serde(default)]
    pub error_count: u64,
    #[serde(default)]
    pub first_response_delay_ms: Option<u64>,
    pub rate_hz: f64,
    pub latency_ms: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct DiagnosticRecord {
    pub sequence_id: u64,
    pub timestamp_unix_ms: u64,
    pub operation_id: String,
    pub adapter_id: Option<String>,
    pub operation: String,
    pub stage: String,
    pub phase: DiagnosticPhase,
    pub duration_ms: Option<u64>,
    pub signal_ids: Vec<String>,
    pub signal_summaries: Option<Vec<SignalSummary>>,
    pub detail: Option<String>,
    pub transient: Option<bool>,
    #[serde(default)]
    pub consecutive_failures: Option<u64>,
    pub omitted_events: u64,
}
impl DiagnosticRecord {
    fn progress(operation_id: String, progress: DiagnosticProgress) -> Self {
        Self {
            sequence_id: 0,
            timestamp_unix_ms: 0,
            operation_id,
            adapter_id: progress.adapter_id,
            operation: progress.operation,
            stage: progress.stage,
            phase: progress.phase,
            duration_ms: progress.duration_ms,
            signal_ids: progress.signal_ids,
            signal_summaries: None,
            detail: progress.detail.map(|s| diagnostic_error_detail(&s)),
            consecutive_failures: progress.consecutive_failures,
            transient: progress.transient,
            omitted_events: 0,
        }
    }
}

enum Observation {
    Record(DiagnosticRecord),
    Progress(String, DiagnosticProgress, Instant),
    Connected {
        id: String,
        adapter: String,
        supported: Vec<String>,
    },
    Sample(String, Instant),
    Error(String),
    Disconnected,
}
#[derive(Clone)]
pub(crate) struct DiagnosticsService {
    sender: SyncSender<Observation>,
    history: Arc<Mutex<VecDeque<DiagnosticRecord>>>,
    omitted: Arc<AtomicU64>,
    next_operation: Arc<AtomicU64>,
    prefix: Arc<String>,
}
impl DiagnosticsService {
    pub fn start(
        path: PathBuf,
        publish: impl Fn(DiagnosticRecord) + Send + 'static,
    ) -> Result<Self, String> {
        Self::start_with_limit(path, publish, ROTATION_BYTES)
    }
    fn start_with_limit(
        path: PathBuf,
        publish: impl Fn(DiagnosticRecord) + Send + 'static,
        limit: u64,
    ) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_SIZE);
        let history = Arc::new(Mutex::new(VecDeque::with_capacity(HISTORY_SIZE)));
        let omitted = Arc::new(AtomicU64::new(0));
        let sequence = latest_sequence(&path);
        let (writer, startup_error) = match JsonlWriter::open(&path, limit) {
            Ok(writer) => (Some(writer), None),
            Err(error) => (None, Some(format!("Could not open diagnostics: {error}"))),
        };
        let mut worker = Worker {
            writer,
            history: history.clone(),
            omitted: omitted.clone(),
            publish: Box::new(publish),
            sequence,
            reported_omitted: 0,
            session: None,
            selected: Vec::new(),
            disconnect: None,
        };
        std::thread::Builder::new()
            .name("diagnostics-writer".into())
            .spawn(move || {
                if let Some(error) = startup_error {
                    worker.storage_failure(error);
                }
                loop {
                    match receiver.recv_timeout(Duration::from_millis(100)) {
                        Ok(observation) => worker.observe(observation),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                    worker.flush_summary(false);
                    worker.report_omissions();
                }
                worker.flush_summary(true);
            })
            .map_err(|e| format!("Could not start diagnostics writer: {e}"))?;
        Ok(Self {
            sender,
            history,
            omitted,
            next_operation: Arc::new(AtomicU64::new(1)),
            prefix: Arc::new(format!(
                "{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            )),
        })
    }
    fn enqueue(&self, observation: Observation) {
        if self.sender.try_send(observation).is_err() {
            self.omitted.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub fn record(&self, mut record: DiagnosticRecord) {
        record.detail = record.detail.map(|s| diagnostic_error_detail(&s));
        self.enqueue(Observation::Record(record));
    }
    pub fn snapshot(&self) -> Vec<DiagnosticRecord> {
        self.history
            .lock()
            .map(|records| records.iter().cloned().collect())
            .unwrap_or_default()
    }
    fn operation_id(&self) -> String {
        format!(
            "{}-{}",
            self.prefix,
            self.next_operation.fetch_add(1, Ordering::Relaxed)
        )
    }
    pub fn begin_operation(
        &self,
        adapter_id: Option<String>,
        operation: &str,
        signal_ids: Vec<String>,
    ) -> DiagnosticOperation {
        let op = DiagnosticOperation {
            service: self.clone(),
            id: self.operation_id(),
            adapter_id,
            operation: operation.into(),
            signal_ids,
            started: Instant::now(),
        };
        self.record(op.record(DiagnosticPhase::Started, None));
        op
    }
    pub fn observe_engine(&self, event: &EngineEvent) {
        match event {
            EngineEvent::Connected(capabilities) => self.enqueue(Observation::Connected {
                id: self.operation_id(),
                adapter: capabilities.device.stable_id.clone(),
                supported: capabilities
                    .supported_signals
                    .iter()
                    .map(|s| s.id.clone())
                    .collect(),
            }),
            EngineEvent::Sample(sample)
                if sample.origin == SampleOrigin::Live
                    && sample.quality == SampleQuality::Valid
                    && !sample.source_id.starts_with("a30-research:")
                    && !sample.source_id.starts_with("oem:") =>
            {
                self.enqueue(Observation::Sample(
                    sample.signal_id.clone(),
                    Instant::now(),
                ))
            }
            EngineEvent::Error(error) => {
                self.enqueue(Observation::Error(diagnostic_error_detail(error)))
            }
            EngineEvent::Disconnected => self.enqueue(Observation::Disconnected),
            _ => {}
        }
    }
}
pub(crate) struct DiagnosticOperation {
    service: DiagnosticsService,
    id: String,
    adapter_id: Option<String>,
    operation: String,
    signal_ids: Vec<String>,
    started: Instant,
}
impl DiagnosticOperation {
    fn record(&self, phase: DiagnosticPhase, detail: Option<String>) -> DiagnosticRecord {
        DiagnosticRecord::progress(
            self.id.clone(),
            DiagnosticProgress {
                adapter_id: self.adapter_id.clone(),
                operation: self.operation.clone(),
                stage: "command".into(),
                phase,
                duration_ms: if phase == DiagnosticPhase::Started {
                    None
                } else {
                    Some(self.started.elapsed().as_millis().min(u64::MAX as u128) as u64)
                },
                signal_ids: self.signal_ids.clone(),
                detail,
                consecutive_failures: None,
                transient: None,
            },
        )
    }
    pub fn sink(&self) -> DiagnosticSink {
        let service = self.service.clone();
        let id = self.id.clone();
        Arc::new(move |mut progress| {
            // Poll starts/successes are internal observations; samples provide success counts.
            if progress.operation == "acquisition"
                && progress.phase != DiagnosticPhase::Failed
                && !(progress.stage == "pid_read" && progress.phase == DiagnosticPhase::Succeeded)
                && !(progress.stage == "poll"
                    && matches!(
                        progress.phase,
                        DiagnosticPhase::Started | DiagnosticPhase::Succeeded
                    ))
            {
                return;
            }
            progress.detail = progress.detail.map(|s| diagnostic_error_detail(&s));
            service.enqueue(Observation::Progress(id.clone(), progress, Instant::now()));
        })
    }
    pub fn finish(&self, outcome: DiagnosticPhase, detail: Option<String>) {
        self.service.record(self.record(outcome, detail));
    }
    pub fn finish_result<T>(&self, result: &Result<T, String>) {
        self.finish(
            if result.is_ok() {
                DiagnosticPhase::Succeeded
            } else {
                DiagnosticPhase::Failed
            },
            result.as_ref().err().cloned(),
        );
    }
    pub fn research_outcome(
        &self,
        outcome: &str,
        nrc: Option<u8>,
        evidence_saved: bool,
        evidence_error: Option<&str>,
    ) {
        let mut record = self.record(
            if outcome == "positive" {
                DiagnosticPhase::Succeeded
            } else {
                DiagnosticPhase::Failed
            },
            Some(format!(
                "Research outcome: {outcome}; NRC: {nrc:?}; evidence_saved: {evidence_saved}"
            )),
        );
        record.stage = "research_outcome".into();
        self.service.record(record);
        if let Some(error) = evidence_error {
            let mut record = self.record(DiagnosticPhase::Failed, Some(error.to_owned()));
            record.stage = "evidence_storage".into();
            self.service.record(record);
        }
    }
    pub fn supported_signals(&self, signal_ids: Vec<String>) {
        let mut record = self.record(DiagnosticPhase::Progress, None);
        record.stage = "supported_signals".into();
        record.signal_ids = signal_ids;
        self.service.record(record);
    }
}

#[derive(Default)]
struct Counts {
    requested: Option<bool>,
    supported: Option<bool>,
    attempts: u64,
    errors: u64,
    first_response_delay_ms: Option<u64>,
    success: u64,
    interval_success: u64,
    latency_total: u64,
    latency_count: u64,
}
struct Session {
    id: String,
    adapter: String,
    selected: Vec<String>,
    started: Instant,
    last_summary: Instant,
    counts: BTreeMap<String, Counts>,
    supported: BTreeSet<String>,
    first_poll: Option<Instant>,
    consecutive_failures: u64,
    dirty: bool,
    statistics_limited: bool,
    failed: bool,
}
struct Worker {
    writer: Option<JsonlWriter>,
    history: Arc<Mutex<VecDeque<DiagnosticRecord>>>,
    omitted: Arc<AtomicU64>,
    publish: Box<dyn Fn(DiagnosticRecord) + Send>,
    sequence: u64,
    reported_omitted: u64,
    session: Option<Session>,
    selected: Vec<String>,
    disconnect: Option<(String, String, Vec<String>)>,
}
impl Worker {
    fn emit_live(&mut self, mut record: DiagnosticRecord) {
        self.sequence += 1;
        record.sequence_id = self.sequence;
        if record.timestamp_unix_ms == 0 {
            record.timestamp_unix_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64;
        }
        record.omitted_events = self.omitted.load(Ordering::Relaxed);
        if let Ok(mut history) = self.history.lock() {
            if history.len() == HISTORY_SIZE {
                history.pop_front();
            }
            history.push_back(record.clone());
        }
        (self.publish)(record);
    }
    fn persist(&mut self, mut record: DiagnosticRecord) {
        record.sequence_id = self.sequence + 1;
        if record.timestamp_unix_ms == 0 {
            record.timestamp_unix_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64;
        }
        record.omitted_events = self.omitted.load(Ordering::Relaxed);
        let error = match self.writer.as_mut() {
            Some(writer) => writer.append(&record).err(),
            None => Some(std::io::Error::other("Diagnostics storage is unavailable")),
        };
        // Keep the same timestamp on disk and in the live view.
        self.emit_live(record);
        if let Some(error) = error {
            self.storage_failure(format!("Could not write diagnostics: {error}"));
        }
    }
    fn system_record(stage: &str, detail: String) -> DiagnosticRecord {
        DiagnosticRecord::progress(
            "diagnostics".into(),
            DiagnosticProgress {
                adapter_id: None,
                operation: "diagnostics".into(),
                stage: stage.into(),
                phase: DiagnosticPhase::Failed,
                duration_ms: None,
                signal_ids: vec![],
                detail: Some(detail),
                consecutive_failures: None,
                transient: None,
            },
        )
    }
    fn storage_failure(&mut self, detail: String) {
        self.omitted.fetch_add(1, Ordering::Relaxed);
        self.reported_omitted = self.omitted.load(Ordering::Relaxed);
        self.emit_live(Self::system_record("storage", detail));
    }
    fn report_omissions(&mut self) {
        let omitted = self.omitted.load(Ordering::Relaxed);
        if omitted != self.reported_omitted {
            self.reported_omitted = omitted;
            self.persist(Self::system_record(
                "queue",
                format!("Diagnostics omitted {omitted} observations"),
            ));
        }
    }
    fn observe(&mut self, observation: Observation) {
        match observation {
            Observation::Record(mut record) => {
                if record.operation == "connect" && record.phase == DiagnosticPhase::Started {
                    self.selected = record.signal_ids.clone();
                }
                if record.operation == "disconnect" {
                    if record.phase == DiagnosticPhase::Started
                        && let Some(session) = self.session.as_ref()
                    {
                        self.disconnect = Some((
                            record.operation_id.clone(),
                            session.adapter.clone(),
                            session.selected.clone(),
                        ));
                    }
                    if let Some((_, adapter, signals)) = self.disconnect.as_ref() {
                        record.adapter_id = Some(adapter.clone());
                        record.signal_ids = signals.clone();
                    }
                    if record.stage == "command"
                        && matches!(
                            record.phase,
                            DiagnosticPhase::Succeeded | DiagnosticPhase::Failed
                        )
                    {
                        self.disconnect = None;
                    }
                } else if record.adapter_id.is_none()
                    && record.operation == "research"
                    && let Some(session) = self.session.as_ref()
                {
                    record.adapter_id = Some(session.adapter.clone());
                }
                self.persist(record);
            }
            Observation::Connected {
                id,
                adapter,
                supported,
            } => {
                self.session = Some(Session {
                    id: id.clone(),
                    adapter: adapter.clone(),
                    selected: self.selected.clone(),
                    started: Instant::now(),
                    last_summary: Instant::now(),
                    counts: self
                        .selected
                        .iter()
                        .take(MAX_SIGNAL_SUMMARIES)
                        .map(|id| {
                            (
                                id.clone(),
                                Counts {
                                    requested: Some(true),
                                    supported: Some(supported.contains(id)),
                                    ..Counts::default()
                                },
                            )
                        })
                        .collect(),
                    supported: supported
                        .iter()
                        .take(MAX_SIGNAL_SUMMARIES)
                        .cloned()
                        .collect(),
                    first_poll: None,
                    consecutive_failures: 0,
                    dirty: !self.selected.is_empty(),
                    statistics_limited: self.selected.len() > MAX_SIGNAL_SUMMARIES,
                    failed: false,
                });
                let mut record = Self::system_record("session", String::new());
                record.operation_id = id;
                record.adapter_id = Some(adapter);
                record.operation = "acquisition".into();
                record.phase = DiagnosticPhase::Started;
                record.detail = None;
                record.signal_ids = supported;
                self.persist(record);
            }
            Observation::Sample(id, observed) => {
                if let Some(session) = self.session.as_mut()
                    && let Some(count) = session.counts.get_mut(&id)
                {
                    if count.success == 0 {
                        count.first_response_delay_ms = session.first_poll.map(|started| {
                            observed
                                .saturating_duration_since(started)
                                .as_millis()
                                .min(u64::MAX as u128) as u64
                        });
                    }
                    count.success = count.success.saturating_add(1);
                    count.interval_success = count.interval_success.saturating_add(1);
                    session.dirty = true;
                }
            }
            Observation::Progress(id, progress, observed) => {
                if progress.operation == "acquisition" {
                    let Some(session) = self.session.as_mut() else {
                        return;
                    };
                    if progress.stage == "poll" && progress.phase == DiagnosticPhase::Started {
                        session.first_poll.get_or_insert(observed);
                        for id in &progress.signal_ids {
                            if session.counts.len() < MAX_SIGNAL_SUMMARIES
                                && !session.counts.contains_key(id)
                            {
                                session.counts.insert(
                                    id.clone(),
                                    Counts {
                                        requested: Some(true),
                                        supported: Some(session.supported.contains(id)),
                                        ..Counts::default()
                                    },
                                );
                            }
                            if !session.counts.contains_key(id) {
                                session.statistics_limited = true;
                                session.dirty = true;
                            }
                            if let Some(count) = session.counts.get_mut(id) {
                                count.attempts = count.attempts.saturating_add(1);
                                session.dirty = true;
                            }
                        }
                        return;
                    }
                    if progress.stage == "poll" && progress.phase == DiagnosticPhase::Succeeded {
                        session.consecutive_failures = 0;
                        return;
                    }
                    if progress.phase != DiagnosticPhase::Failed {
                        if progress.stage == "pid_read"
                            && progress.phase == DiagnosticPhase::Succeeded
                            && let Some(duration) = progress.duration_ms
                        {
                            for id in &progress.signal_ids {
                                if let Some(count) = session.counts.get_mut(id) {
                                    count.latency_total =
                                        count.latency_total.saturating_add(duration);
                                    count.latency_count = count.latency_count.saturating_add(1);
                                    session.dirty = true;
                                }
                            }
                        }
                        return;
                    }
                    // Only the adapter's singleton read attribution identifies a PID error.
                    // A failed grouped poll remains a batch outcome, never N inferred PID errors.
                    if progress.stage == "pid_read"
                        && progress.signal_ids.len() == 1
                        && let Some(count) = session.counts.get_mut(&progress.signal_ids[0])
                    {
                        count.errors = count.errors.saturating_add(1);
                        session.dirty = true;
                    }
                    let mut record = DiagnosticRecord::progress(session.id.clone(), progress);
                    if record.stage == "poll" {
                        session.consecutive_failures = record
                            .consecutive_failures
                            .unwrap_or_else(|| session.consecutive_failures.saturating_add(1));
                        record.consecutive_failures = Some(session.consecutive_failures);
                    }
                    record.adapter_id = Some(session.adapter.clone());
                    if record.signal_ids.is_empty() {
                        record.signal_ids = session.selected.clone();
                    }
                    self.persist(record);
                } else {
                    let mut record = DiagnosticRecord::progress(id, progress);
                    if record.operation == "disconnect"
                        && let Some((id, adapter, signals)) = self.disconnect.as_ref()
                    {
                        record.operation_id = id.clone();
                        record.adapter_id = Some(adapter.clone());
                        record.signal_ids = signals.clone();
                    }
                    self.persist(record);
                }
            }
            Observation::Error(error) => {
                let mut record = Self::system_record("engine", error);
                record.operation = "acquisition".into();
                if let Some(session) = self.session.as_mut() {
                    session.failed = true;
                    record.operation_id = session.id.clone();
                    record.adapter_id = Some(session.adapter.clone());
                    record.signal_ids = session.selected.clone();
                }
                self.persist(record);
            }
            Observation::Disconnected => {
                self.flush_summary(true);
                if let Some(session) = self.session.take() {
                    let mut record = Self::system_record("session", String::new());
                    record.operation = "acquisition".into();
                    record.operation_id = session.id;
                    record.adapter_id = Some(session.adapter);
                    record.signal_ids = session.selected;
                    record.phase = if session.failed {
                        DiagnosticPhase::Failed
                    } else {
                        DiagnosticPhase::Succeeded
                    };
                    record.detail = None;
                    record.duration_ms =
                        Some(session.started.elapsed().as_millis().min(u64::MAX as u128) as u64);
                    self.persist(record);
                }
            }
        }
    }
    fn flush_summary(&mut self, final_flush: bool) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if !final_flush && session.last_summary.elapsed() < Duration::from_secs(1) {
            return;
        }
        if !session.dirty {
            return;
        }
        let seconds = session.last_summary.elapsed().as_secs_f64().max(0.001);
        let summaries = session
            .counts
            .iter_mut()
            .map(|(id, c)| {
                let summary = SignalSummary {
                    signal_id: id.clone(),
                    requested: c.requested,
                    supported: c.supported,
                    attempt_count: c.attempts,
                    success_count: c.success,
                    error_count: c.errors,
                    first_response_delay_ms: c.first_response_delay_ms,
                    rate_hz: c.interval_success as f64 / seconds,
                    latency_ms: if c.latency_count > 0 {
                        Some(c.latency_total as f64 / c.latency_count as f64)
                    } else {
                        None
                    },
                };
                c.interval_success = 0;
                c.latency_count = 0;
                c.latency_total = 0;
                summary
            })
            .collect();
        session.last_summary = Instant::now();
        session.dirty = false;
        let mut record = Self::system_record("signal_summary", String::new());
        record.operation = "acquisition".into();
        record.operation_id = session.id.clone();
        record.adapter_id = Some(session.adapter.clone());
        record.signal_ids = session.counts.keys().cloned().collect();
        record.phase = DiagnosticPhase::Progress;
        record.detail = if session.statistics_limited {
            Some(format!(
                "PID reliability summaries are limited to {MAX_SIGNAL_SUMMARIES} entries; additional requested signal IDs are not summarized."
            ))
        } else {
            None
        };
        record.signal_summaries = Some(summaries);
        self.persist(record);
    }
}
fn latest_sequence(path: &Path) -> u64 {
    [
        path.to_owned(),
        rotated_path(path, 1),
        rotated_path(path, 2),
    ]
    .iter()
    .filter_map(|path| {
        let mut file = File::open(path).ok()?;
        let size = file.metadata().ok()?.len();
        file.seek(SeekFrom::Start(size.saturating_sub(ROTATION_BYTES)))
            .ok()?;
        let mut tail = String::new();
        file.read_to_string(&mut tail).ok()?;
        tail.lines().rev().find_map(|line| {
            serde_json::from_str::<DiagnosticRecord>(line)
                .ok()
                .map(|record| record.sequence_id)
        })
    })
    .max()
    .unwrap_or_default()
}
struct JsonlWriter {
    path: PathBuf,
    file: Option<File>,
    bytes: u64,
    limit: u64,
}
fn rotated_path(path: &Path, generation: usize) -> PathBuf {
    path.with_file_name(format!("operations.{generation}.jsonl"))
}
impl JsonlWriter {
    fn open(path: &Path, limit: u64) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let bytes = file.metadata()?.len();
        Ok(Self {
            path: path.into(),
            file: Some(file),
            bytes,
            limit,
        })
    }
    fn append(&mut self, record: &DiagnosticRecord) -> std::io::Result<()> {
        let mut bytes = serde_json::to_vec(record)?;
        bytes.push(b'\n');
        if bytes.len() as u64 > self.limit {
            return Err(std::io::Error::other(
                "Diagnostic record exceeds storage size bound",
            ));
        }
        if self.bytes + bytes.len() as u64 > self.limit {
            self.file.take();
            let prior1 = rotated_path(&self.path, 1);
            let prior2 = rotated_path(&self.path, 2);
            if prior2.exists() {
                std::fs::remove_file(&prior2)?;
            }
            if prior1.exists() {
                std::fs::rename(&prior1, &prior2)?;
            }
            std::fs::rename(&self.path, &prior1)?;
            self.bytes = 0;
        }
        if self.file.is_none() {
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)?,
            );
        }
        self.file.as_mut().unwrap().write_all(&bytes)?;
        self.bytes += bytes.len() as u64;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn path() -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "lognook-diagnostics-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
            .join("operations.jsonl")
    }
    fn service(path: &Path) -> (DiagnosticsService, mpsc::Receiver<DiagnosticRecord>) {
        let (tx, rx) = mpsc::channel();
        (
            DiagnosticsService::start(path.to_owned(), move |record| {
                let _ = tx.send(record);
            })
            .unwrap(),
            rx,
        )
    }
    #[test]
    fn jsonl_orders_operation_success_and_failure_with_duration() {
        let path = path();
        let (service, rx) = service(&path);
        for phase in [DiagnosticPhase::Succeeded, DiagnosticPhase::Failed] {
            let op =
                service.begin_operation(Some("adapter".into()), "identify", vec!["rpm".into()]);
            op.finish(phase, Some("provider error 0x09".into()));
        }
        let records: Vec<_> = (0..4)
            .map(|_| rx.recv_timeout(Duration::from_secs(2)).unwrap())
            .collect();
        assert_eq!(
            records.iter().map(|r| r.sequence_id).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
        assert_eq!(records[0].operation_id, records[1].operation_id);
        assert_ne!(records[0].operation_id, records[2].operation_id);
        assert_eq!(records[1].phase, DiagnosticPhase::Succeeded);
        assert_eq!(records[3].phase, DiagnosticPhase::Failed);
        assert!(records[3].duration_ms.is_some());
        assert_eq!(records[3].detail.as_deref(), Some("provider error 0x09"));
        assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 4);
    }
    #[test]
    fn history_retains_only_latest_500_records() {
        let (service, rx) = service(&path());
        for _ in 0..510 {
            service
                .begin_operation(None, "identify", vec![])
                .finish(DiagnosticPhase::Succeeded, None);
            rx.recv().unwrap();
            rx.recv().unwrap();
        }
        let snapshot = service.snapshot();
        assert_eq!(snapshot.len(), 500);
        assert_eq!(snapshot[0].sequence_id, 521);
        assert_eq!(snapshot[499].sequence_id, 1020);
    }
    #[test]
    fn rotation_keeps_two_prior_files_with_size_bound() {
        let path = path();
        let mut writer = JsonlWriter::open(&path, 600).unwrap();
        for _ in 0..20 {
            writer
                .append(&DiagnosticRecord::progress(
                    "op".into(),
                    progress("identify", "command", DiagnosticPhase::Started),
                ))
                .unwrap();
        }
        for path in [&path, &rotated_path(&path, 1), &rotated_path(&path, 2)] {
            assert!(std::fs::metadata(path).unwrap().len() <= 600);
        }
        assert!(!rotated_path(&path, 3).exists());
    }
    #[test]
    fn saturated_queue_returns_promptly_and_surfaces_omissions() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let once = AtomicU64::new(0);
        let service = DiagnosticsService::start(path(), move |_| {
            if once.fetch_add(1, Ordering::Relaxed) == 0 {
                entered_tx.send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
            }
        })
        .unwrap();
        service.begin_operation(None, "identify", vec![]);
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let started = Instant::now();
        for _ in 0..1100 {
            service.record(DiagnosticRecord::progress(
                "op".into(),
                progress("identify", "command", DiagnosticPhase::Started),
            ));
        }
        assert!(started.elapsed() < Duration::from_millis(200));
        assert!(service.omitted.load(Ordering::Relaxed) >= 76);
        release_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !service.snapshot().iter().any(|r| r.omitted_events > 0) {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    #[test]
    fn storage_failure_preserves_live_records_and_counts_omissions() {
        let path = path();
        let (tx, rx) = mpsc::channel();
        let service = DiagnosticsService::start_with_limit(
            path.clone(),
            move |r| {
                let _ = tx.send(r);
            },
            600,
        )
        .unwrap();
        std::fs::create_dir(rotated_path(&path, 2)).unwrap();
        for _ in 0..5 {
            service.begin_operation(None, "identify", vec![]);
            rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        assert!(service.omitted.load(Ordering::Relaxed) > 0);
        assert!(
            service
                .snapshot()
                .iter()
                .any(|r| r.stage == "storage" && r.phase == DiagnosticPhase::Failed)
        );
    }
    #[test]
    fn restarting_preserves_jsonl_sequence_order() {
        let path = path();
        let (first, rx) = service(&path);
        first.begin_operation(None, "identify", vec![]);
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(first);
        let (second, rx) = service(&path);
        second.begin_operation(None, "identify", vec![]);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap().sequence_id,
            2
        );
    }
    #[test]
    fn storage_open_failure_keeps_command_outcomes_live() {
        let path = path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path.parent().unwrap().join("blocker"), b"file").unwrap();
        let (service, rx) = service(&path.parent().unwrap().join("blocker/operations.jsonl"));
        let failure = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(failure.stage, "storage");
        assert!(
            failure
                .detail
                .unwrap()
                .contains("Could not open diagnostics")
        );
        let op = service.begin_operation(None, "identify", vec![]);
        op.finish(DiagnosticPhase::Failed, Some("provider error 0x09".into()));
        loop {
            let record = rx.recv_timeout(Duration::from_secs(2)).unwrap();
            if record.operation == "identify" && record.phase == DiagnosticPhase::Failed {
                assert_eq!(record.detail.as_deref(), Some("provider error 0x09"));
                break;
            }
        }
        assert!(service.omitted.load(Ordering::Relaxed) > 0);
    }
    #[test]
    fn retained_adapter_sink_routes_explicit_disconnect_to_its_command() {
        let (service, rx) = service(&path());
        let connect =
            service.begin_operation(Some("adapter".into()), "connect", vec!["rpm".into()]);
        service.observe_engine(&EngineEvent::Connected(
            lognook_core::ConnectionCapabilities {
                device: lognook_core::DeviceDescriptor {
                    stable_id: "adapter".into(),
                    hardware_family: "test".into(),
                    firmware: None,
                    path: None,
                },
                protocol: "test".into(),
                read_only: true,
                supported_signals: vec![],
            },
        ));
        connect.finish(DiagnosticPhase::Succeeded, None);
        let disconnect = service.begin_operation(None, "disconnect", vec![]);
        connect.sink()(progress(
            "disconnect",
            "channel_close",
            DiagnosticPhase::Progress,
        ));
        service.observe_engine(&EngineEvent::Disconnected);
        disconnect.finish(DiagnosticPhase::Succeeded, None);
        let mut stage = None;
        loop {
            let record = rx.recv_timeout(Duration::from_secs(2)).unwrap();
            if record.stage == "channel_close" {
                stage = Some(record.clone());
            }
            if record.operation == "disconnect"
                && record.stage == "command"
                && record.phase == DiagnosticPhase::Succeeded
            {
                assert_eq!(record.adapter_id.as_deref(), Some("adapter"));
                break;
            }
        }
        let stage = stage.unwrap();
        assert_eq!(stage.operation_id, disconnect.id);
        assert_ne!(stage.operation_id, connect.id);
        assert_eq!(stage.signal_ids, ["rpm"]);
    }
    fn connected(service: &DiagnosticsService, selected: &[&str]) -> DiagnosticOperation {
        let op = service.begin_operation(
            Some("adapter".into()),
            "connect",
            selected.iter().map(|s| (*s).to_owned()).collect(),
        );
        service.observe_engine(&EngineEvent::Connected(
            lognook_core::ConnectionCapabilities {
                device: lognook_core::DeviceDescriptor {
                    stable_id: "adapter".into(),
                    hardware_family: "test".into(),
                    firmware: None,
                    path: None,
                },
                protocol: "test".into(),
                read_only: true,
                supported_signals: lognook_core::standard_obd_signals(),
            },
        ));
        op.finish(DiagnosticPhase::Succeeded, None);
        op
    }
    fn sample(id: &str) -> EngineEvent {
        EngineEvent::Sample(lognook_core::Sample {
            elapsed_ns: 0,
            wall_time_unix_ns: 0,
            source_id: "adapter".into(),
            ecu_id: None,
            signal_id: id.into(),
            value: 123456.789,
            unit: "rpm".into(),
            quality: SampleQuality::Valid,
            origin: SampleOrigin::Live,
            raw_bytes: Some(vec![65, 12, 26, 248]),
        })
    }
    fn poll(phase: DiagnosticPhase, consecutive: Option<u64>) -> DiagnosticProgress {
        let mut event = progress("acquisition", "poll", phase);
        event.signal_ids = vec!["engine_rpm".into(), "vehicle_speed".into()];
        let mut json = serde_json::to_value(event).unwrap();
        json["consecutive_failures"] = serde_json::json!(consecutive);
        serde_json::from_value(json).unwrap()
    }
    fn until_disconnected(rx: &mpsc::Receiver<DiagnosticRecord>) -> Vec<DiagnosticRecord> {
        let mut records = Vec::new();
        loop {
            let record = rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let done = record.operation == "acquisition"
                && record.stage == "session"
                && record.phase != DiagnosticPhase::Started;
            records.push(record);
            if done {
                return records;
            }
        }
    }
    #[test]
    fn pid_reliability_counts_attempts_and_only_attributable_errors_without_values() {
        let (service, rx) = service(&path());
        let connect = connected(&service, &["engine_rpm", "vehicle_speed"]);
        let sink = connect.sink();
        std::thread::sleep(Duration::from_millis(200));
        sink(poll(DiagnosticPhase::Started, None));
        std::thread::sleep(Duration::from_millis(25));
        let mut read = progress("acquisition", "pid_read", DiagnosticPhase::Succeeded);
        read.signal_ids = vec!["engine_rpm".into()];
        sink(read);
        service.observe_engine(&sample("engine_rpm"));
        sink(poll(DiagnosticPhase::Succeeded, None));
        sink(poll(DiagnosticPhase::Started, None));
        let mut specific = progress("acquisition", "pid_read", DiagnosticPhase::Failed);
        specific.signal_ids = vec!["vehicle_speed".into()];
        specific.detail = Some("adapter response timed out".into());
        specific.transient = Some(true);
        sink(specific);
        sink(poll(DiagnosticPhase::Failed, Some(1)));
        sink(poll(DiagnosticPhase::Started, None));
        let mut grouped = poll(DiagnosticPhase::Failed, None);
        grouped.stage = "pid_read".into();
        sink(grouped);
        sink(poll(DiagnosticPhase::Failed, Some(2)));
        service.observe_engine(&EngineEvent::Disconnected);
        let records = until_disconnected(&rx);
        let summary = records
            .iter()
            .find(|r| r.stage == "signal_summary")
            .expect("PID summaries include attempted signals");
        let json = serde_json::to_value(summary).unwrap();
        let rpm = &json["signal_summaries"][0];
        let speed = &json["signal_summaries"][1];
        assert_eq!(rpm["signal_id"], "engine_rpm");
        assert_eq!(rpm["attempt_count"], 3);
        assert_eq!(rpm["success_count"], 1);
        assert_eq!(rpm["error_count"], 0);
        assert_eq!(rpm["requested"], true);
        assert_eq!(rpm["supported"], true);
        assert_eq!(rpm["latency_ms"], 12.0);
        let first_response = rpm["first_response_delay_ms"]
            .as_u64()
            .expect("first-response delay");
        assert!((20..150).contains(&first_response));
        assert_eq!(speed["signal_id"], "vehicle_speed");
        assert_eq!(speed["attempt_count"], 3);
        assert_eq!(speed["success_count"], 0);
        assert_eq!(speed["error_count"], 1);
        assert_eq!(speed["first_response_delay_ms"], serde_json::Value::Null);
        assert_eq!(speed["rate_hz"], 0.0);
        let failures = records
            .iter()
            .filter(|r| r.stage == "poll" && r.phase == DiagnosticPhase::Failed)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 2);
        for (record, count) in failures.iter().zip([1, 2]) {
            assert_eq!(record.signal_ids, ["engine_rpm", "vehicle_speed"]);
            assert_eq!(
                serde_json::to_value(record).unwrap()["consecutive_failures"],
                count
            );
        }
        assert_eq!(
            records
                .iter()
                .filter(|r| r.stage == "signal_summary")
                .count(),
            1
        );
        let json = serde_json::to_string(&records).unwrap();
        assert!(!json.contains("123456.789"));
        assert!(!json.contains("raw_bytes"));
        assert!(!json.contains("65,12,26,248"));
    }
    #[test]
    fn attempted_pids_without_any_samples_still_have_a_final_summary() {
        let (service, rx) = service(&path());
        let op = connected(&service, &["engine_rpm", "vehicle_speed"]);
        op.sink()(poll(DiagnosticPhase::Started, None));
        op.sink()(poll(DiagnosticPhase::Failed, Some(1)));
        service.observe_engine(&EngineEvent::Disconnected);
        let records = until_disconnected(&rx);
        let summary = records
            .iter()
            .find(|r| r.stage == "signal_summary")
            .expect("no-response PIDs must remain visible");
        let json = serde_json::to_value(summary).unwrap();
        assert_eq!(json["signal_summaries"].as_array().unwrap().len(), 2);
        for signal in json["signal_summaries"].as_array().unwrap() {
            assert_eq!(signal["attempt_count"], 1);
            assert_eq!(signal["success_count"], 0);
            assert_eq!(signal["error_count"], 0);
        }
    }
    #[test]
    fn unselected_research_samples_do_not_grow_pid_reliability_history() {
        let (service, rx) = service(&path());
        let op = connected(&service, &["engine_rpm", "vehicle_speed"]);
        op.sink()(poll(DiagnosticPhase::Started, None));
        for index in 0..600 {
            service.observe_engine(&sample(&format!("research-{index}")));
        }
        service.observe_engine(&EngineEvent::Disconnected);
        let records = until_disconnected(&rx);
        let summary = records
            .iter()
            .find(|r| r.stage == "signal_summary")
            .unwrap();
        assert_eq!(summary.signal_ids.len(), 2);
        assert_eq!(summary.signal_ids, ["engine_rpm", "vehicle_speed"]);
    }
    #[test]
    fn research_sample_for_selected_signal_is_not_an_acquisition_response() {
        let (service, rx) = service(&path());
        let op = connected(&service, &["engine_rpm", "vehicle_speed"]);
        op.sink()(poll(DiagnosticPhase::Started, None));
        let EngineEvent::Sample(mut research) = sample("engine_rpm") else {
            unreachable!()
        };
        research.source_id = "a30-research:adapter".into();
        service.observe_engine(&EngineEvent::Sample(research));
        service.observe_engine(&EngineEvent::Disconnected);
        let records = until_disconnected(&rx);
        let summary = records
            .iter()
            .find(|r| r.stage == "signal_summary")
            .unwrap();
        let json = serde_json::to_value(summary).unwrap();
        assert_eq!(json["signal_summaries"][0]["success_count"], 0);
        assert_eq!(
            json["signal_summaries"][0]["first_response_delay_ms"],
            serde_json::Value::Null
        );
    }
    #[test]
    fn selected_pids_are_summarized_even_when_disconnect_precedes_the_first_poll() {
        let (service, rx) = service(&path());
        connected(&service, &["engine_rpm", "vehicle_speed"]);
        service.observe_engine(&EngineEvent::Disconnected);
        let records = until_disconnected(&rx);
        let summary = records
            .iter()
            .find(|r| r.stage == "signal_summary")
            .expect("selected PIDs remain visible before their first attempt");
        let json = serde_json::to_value(summary).unwrap();
        assert_eq!(json["signal_summaries"][0]["attempt_count"], 0);
        assert_eq!(json["signal_summaries"][0]["success_count"], 0);
    }
    #[test]
    fn reliability_entries_are_limited_to_500_with_visible_limit_feedback() {
        let (service, rx) = service(&path());
        let selected = (0..600).map(|i| format!("pid-{i}")).collect::<Vec<_>>();
        let refs = selected.iter().map(String::as_str).collect::<Vec<_>>();
        connected(&service, &refs);
        service.observe_engine(&EngineEvent::Disconnected);
        let records = until_disconnected(&rx);
        let summary = records
            .iter()
            .find(|r| r.stage == "signal_summary")
            .expect("bounded PID summary");
        assert_eq!(summary.signal_summaries.as_ref().unwrap().len(), 500);
        assert!(
            summary
                .detail
                .as_ref()
                .is_some_and(|s| s.contains("limited to 500"))
        );
    }
    fn progress(operation: &str, stage: &str, phase: DiagnosticPhase) -> DiagnosticProgress {
        DiagnosticProgress {
            adapter_id: Some("adapter".into()),
            operation: operation.into(),
            stage: stage.into(),
            phase,
            duration_ms: Some(12),
            signal_ids: vec!["rpm".into(), "speed".into()],
            detail: None,
            consecutive_failures: None,
            transient: None,
        }
    }
    #[test]
    fn acquisition_aggregates_success_and_routes_failures_to_distinct_session() {
        let (service, rx) = service(&path());
        let connect = service.begin_operation(
            Some("adapter".into()),
            "connect",
            vec!["rpm".into(), "speed".into()],
        );
        let connect_id = connect.id.clone();
        service.observe_engine(&EngineEvent::Connected(
            lognook_core::ConnectionCapabilities {
                device: lognook_core::DeviceDescriptor {
                    stable_id: "adapter".into(),
                    hardware_family: "test".into(),
                    firmware: None,
                    path: None,
                },
                protocol: "test".into(),
                read_only: true,
                supported_signals: vec![],
            },
        ));
        connect.finish(DiagnosticPhase::Succeeded, None);
        let sink = connect.sink();
        for _ in 0..100 {
            sink(progress("acquisition", "poll", DiagnosticPhase::Succeeded));
            sink(progress(
                "acquisition",
                "pid_read",
                DiagnosticPhase::Succeeded,
            ));
            service.observe_engine(&EngineEvent::Sample(lognook_core::Sample {
                elapsed_ns: 0,
                wall_time_unix_ns: 0,
                source_id: "adapter".into(),
                ecu_id: None,
                signal_id: "rpm".into(),
                value: 123456.789,
                unit: "rpm".into(),
                quality: lognook_core::SampleQuality::Valid,
                origin: lognook_core::SampleOrigin::Live,
                raw_bytes: Some(vec![65, 12, 26, 248]),
            }));
        }
        let mut failure = progress("acquisition", "poll", DiagnosticPhase::Failed);
        failure.detail = Some("Timed out waiting for the ELM327 > prompt after 010C".into());
        sink(failure);
        service.observe_engine(&EngineEvent::Disconnected);
        let mut records = vec![];
        loop {
            let r = rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let done = r.operation == "acquisition"
                && r.stage == "session"
                && r.phase == DiagnosticPhase::Succeeded;
            records.push(r);
            if done {
                break;
            }
        }
        let failure = records
            .iter()
            .find(|r| r.stage == "poll" && r.phase == DiagnosticPhase::Failed)
            .unwrap();
        assert_ne!(failure.operation_id, connect_id);
        assert_eq!(failure.signal_ids, ["rpm", "speed"]);
        assert_eq!(
            failure.detail.as_deref(),
            Some("Timed out waiting for the ELM327 > prompt after [request redacted]")
        );
        assert_eq!(
            records
                .iter()
                .filter(|r| r.stage == "signal_summary")
                .count(),
            1
        );
        assert_eq!(
            records
                .iter()
                .find_map(|r| r.signal_summaries.as_ref())
                .unwrap()[0]
                .success_count,
            100
        );
        let json = serde_json::to_string(&records).unwrap();
        assert!(!json.contains("123456.789"));
        assert!(!json.contains("raw_bytes"));
        assert!(!json.contains("65,12,26,248"));
    }
}
