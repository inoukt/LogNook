//! Bounded, passive CAN research analysis. All inferred meanings are hypotheses.
use crate::{
    CanDirection, CanFrame, CanSignalDefinition, LogNookError, Sample, SampleQuality,
    decode_can_signal,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanDiagnosticRoute {
    pub request_id: u32,
    pub response_id: u32,
    pub extended: bool,
}

#[derive(Clone, Debug)]
pub struct CanAnalysisConfig {
    pub max_ids: usize,
    pub history_capacity: usize,
    pub max_obd_samples: usize,
    pub max_timing_rows: usize,
    pub correlation_tolerance_ns: i64,
    pub request_timeout_ns: i64,
    pub diagnostic_routes: Vec<CanDiagnosticRoute>,
}
impl Default for CanAnalysisConfig {
    fn default() -> Self {
        Self {
            max_ids: 2048,
            history_capacity: 256,
            max_obd_samples: 1024,
            max_timing_rows: 256,
            correlation_tolerance_ns: 50_000_000,
            request_timeout_ns: 1_000_000_000,
            diagnostic_routes: Vec::new(),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CanByteSummary {
    pub observations: u64,
    pub changes: u64,
    pub min: Option<u8>,
    pub max: Option<u8>,
    pub is_static: bool,
    pub counter_hypothesis: bool,
    pub checksum_hypothesis: bool,
    #[serde(skip)]
    previous: Option<u8>,
    #[serde(skip)]
    increments: u64,
    #[serde(skip)]
    checksum_matches: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanCensusRow {
    pub can_id: u32,
    pub extended: bool,
    pub count: u64,
    pub rx_count: u64,
    pub tx_count: u64,
    pub first_elapsed_ns: i64,
    pub last_elapsed_ns: i64,
    pub dlc_counts: [u64; 9],
    pub rate_hz: Option<f64>,
    pub mean_period_ns: Option<f64>,
    pub bytes: [CanByteSummary; 8],
}
struct BusEntry {
    row: CanCensusRow,
    history: VecDeque<CanFrame>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanDiagnosticOutcome {
    Positive,
    Negative { nrc: u8 },
    ResponsePending { nrc: u8 },
    WrongEcho,
    Malformed,
    NoResponse,
    Superseded,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanRequestTiming {
    pub route: CanDiagnosticRoute,
    pub request_elapsed_ns: i64,
    pub response_elapsed_ns: i64,
    pub latency_ns: i64,
    pub request_payload: Vec<u8>,
    pub response_payload: Vec<u8>,
    pub outcome: CanDiagnosticOutcome,
}
struct Pending {
    timestamp: i64,
    payload: Vec<u8>,
}
#[derive(Default)]
struct Assembly {
    payload: Vec<u8>,
    length: usize,
    next_sequence: u8,
    first_elapsed_ns: i64,
}
struct RouteState {
    route: CanDiagnosticRoute,
    pending: Option<Pending>,
    request: Assembly,
    response: Assembly,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanCorrelationHypothesis {
    pub field_id: String,
    pub obd_source_id: String,
    pub obd_ecu_id: Option<String>,
    pub obd_signal_id: String,
    pub pairs: usize,
    pub pearson_r: f64,
    pub hypothesis: bool,
}

pub struct CanAnalyzer {
    config: CanAnalysisConfig,
    ids: BTreeMap<(u32, bool), BusEntry>,
    obd: VecDeque<Sample>,
    routes: Vec<RouteState>,
    timings: VecDeque<CanRequestTiming>,
    last_time: Option<i64>,
}
impl CanAnalyzer {
    pub fn new(config: CanAnalysisConfig) -> Result<Self, LogNookError> {
        if config.max_ids == 0
            || config.history_capacity == 0
            || config.max_obd_samples == 0
            || config.max_timing_rows == 0
            || config.correlation_tolerance_ns < 0
            || config.request_timeout_ns <= 0
        {
            return Err(LogNookError::new("CAN analysis bounds must be positive"));
        }
        for r in &config.diagnostic_routes {
            for id in [r.request_id, r.response_id] {
                if id > if r.extended { 0x1fff_ffff } else { 0x7ff } {
                    return Err(LogNookError::new("Invalid diagnostic CAN route"));
                }
            }
            if r.request_id == r.response_id {
                return Err(LogNookError::new(
                    "Diagnostic request/reply IDs must differ",
                ));
            }
        }
        let routes = config
            .diagnostic_routes
            .iter()
            .cloned()
            .map(|route| RouteState {
                route,
                pending: None,
                request: Assembly::default(),
                response: Assembly::default(),
            })
            .collect();
        Ok(Self {
            config,
            ids: BTreeMap::new(),
            obd: VecDeque::new(),
            routes,
            timings: VecDeque::new(),
            last_time: None,
        })
    }
    pub fn feed(&mut self, frame: &CanFrame) -> Result<(), LogNookError> {
        frame.validate()?;
        if self.last_time.is_some_and(|t| frame.elapsed_ns < t) {
            return Err(LogNookError::new("CAN frames must be in timestamp order"));
        }
        let key = (frame.can_id, frame.extended);
        if !self.ids.contains_key(&key) && self.ids.len() >= self.config.max_ids {
            return Err(LogNookError::new("CAN census ID limit reached"));
        }
        self.advance_time(frame.elapsed_ns);
        self.last_time = Some(frame.elapsed_ns);
        let entry = self.ids.entry(key).or_insert_with(|| BusEntry {
            row: CanCensusRow {
                can_id: frame.can_id,
                extended: frame.extended,
                count: 0,
                rx_count: 0,
                tx_count: 0,
                first_elapsed_ns: frame.elapsed_ns,
                last_elapsed_ns: frame.elapsed_ns,
                dlc_counts: [0; 9],
                rate_hz: None,
                mean_period_ns: None,
                bytes: Default::default(),
            },
            history: VecDeque::new(),
        });
        let row = &mut entry.row;
        row.count += 1;
        match frame.direction {
            CanDirection::Rx => row.rx_count += 1,
            CanDirection::Tx => row.tx_count += 1,
        }
        row.last_elapsed_ns = frame.elapsed_ns;
        row.dlc_counts[frame.data.len()] += 1;
        let span = row.last_elapsed_ns - row.first_elapsed_ns;
        if span > 0 {
            row.mean_period_ns = Some(span as f64 / (row.count - 1) as f64);
            row.rate_hz = Some((row.count - 1) as f64 * 1e9 / span as f64);
        }
        for (i, &value) in frame.data.iter().enumerate() {
            let byte = &mut row.bytes[i];
            byte.observations += 1;
            if let Some(previous) = byte.previous {
                byte.changes += u64::from(previous != value);
                byte.increments += u64::from(previous.wrapping_add(1) == value);
            }
            byte.min = Some(byte.min.map_or(value, |v| v.min(value)));
            byte.max = Some(byte.max.map_or(value, |v| v.max(value)));
            byte.previous = Some(value);
            byte.is_static = byte.observations >= 2 && byte.changes == 0;
            byte.counter_hypothesis =
                byte.observations >= 4 && byte.increments == byte.observations - 1;
            if i + 1 == frame.data.len() && i > 0 {
                byte.checksum_matches += u64::from(
                    frame.data[..i]
                        .iter()
                        .fold(0u8, |sum, v| sum.wrapping_add(*v))
                        == value,
                );
            }
            byte.checksum_hypothesis = byte.observations >= 4
                && byte.changes > 0
                && byte.checksum_matches == byte.observations;
        }
        push_bounded(
            &mut entry.history,
            frame.clone(),
            self.config.history_capacity,
        );
        let mut rows = Vec::new();
        for state in &mut self.routes {
            if state.route.extended != frame.extended {
                continue;
            }
            let request = frame.can_id == state.route.request_id;
            if !request && frame.can_id != state.route.response_id {
                continue;
            }
            let assembly = if request {
                &mut state.request
            } else {
                &mut state.response
            };
            match assembly.feed(&frame.data, frame.elapsed_ns) {
                Ok(Some((payload, timestamp))) if request => {
                    state.response = Assembly::default();
                    if let Some(p) = state.pending.take() {
                        rows.push(timing(
                            &state.route,
                            p,
                            frame.elapsed_ns,
                            Vec::new(),
                            CanDiagnosticOutcome::Superseded,
                        ));
                    }
                    state.pending = Some(Pending { timestamp, payload });
                }
                Ok(Some((payload, _))) => {
                    if let Some(p) = state.pending.take() {
                        let outcome = reply_outcome(&p.payload, &payload);
                        let pending =
                            matches!(outcome, CanDiagnosticOutcome::ResponsePending { .. });
                        rows.push(timing(
                            &state.route,
                            Pending {
                                timestamp: p.timestamp,
                                payload: p.payload.clone(),
                            },
                            frame.elapsed_ns,
                            payload,
                            outcome,
                        ));
                        if pending {
                            state.pending = Some(p);
                        }
                    }
                }
                Err(()) if !request => {
                    if let Some(p) = state.pending.take() {
                        rows.push(timing(
                            &state.route,
                            p,
                            frame.elapsed_ns,
                            frame.data.clone(),
                            CanDiagnosticOutcome::Malformed,
                        ));
                    }
                }
                _ => {}
            }
        }
        for row in rows {
            push_bounded(&mut self.timings, row, self.config.max_timing_rows);
        }
        Ok(())
    }
    /// Caller-driven timeout evaluation; no timer or transport worker is created.
    pub fn advance_time(&mut self, elapsed_ns: i64) {
        for state in &mut self.routes {
            if state.pending.as_ref().is_some_and(|p| {
                elapsed_ns.saturating_sub(p.timestamp) > self.config.request_timeout_ns
            }) {
                let p = state.pending.take().unwrap();
                push_bounded(
                    &mut self.timings,
                    timing(
                        &state.route,
                        p,
                        elapsed_ns,
                        Vec::new(),
                        CanDiagnosticOutcome::NoResponse,
                    ),
                    self.config.max_timing_rows,
                );
                state.response = Assembly::default();
            }
        }
    }
    pub fn feed_obd_sample(&mut self, sample: &Sample) {
        if sample.quality == SampleQuality::Valid
            && sample.value.is_finite()
            && sample.elapsed_ns >= 0
        {
            push_bounded(&mut self.obd, sample.clone(), self.config.max_obd_samples);
        }
    }
    pub fn census(&self) -> Vec<CanCensusRow> {
        self.ids.values().map(|e| e.row.clone()).collect()
    }
    pub fn history(&self, can_id: u32, extended: bool) -> Vec<CanFrame> {
        self.ids
            .get(&(can_id, extended))
            .map_or_else(Vec::new, |e| e.history.iter().cloned().collect())
    }
    pub fn timings(&self) -> Vec<CanRequestTiming> {
        self.timings.iter().cloned().collect()
    }
    /// Nearest observed RX field, without interpolation; each frame is paired at most once.
    pub fn correlate(
        &self,
        field: &CanSignalDefinition,
        source: &str,
        ecu: Option<&str>,
        signal: &str,
    ) -> Result<Option<CanCorrelationHypothesis>, LogNookError> {
        field.validate()?;
        let frames = self.history(field.can_id, field.extended);
        let mut used = vec![false; frames.len()];
        let mut pairs = Vec::new();
        for sample in self.obd.iter().filter(|s| {
            s.source_id == source && s.ecu_id.as_deref() == ecu && s.signal_id == signal
        }) {
            let candidate = frames
                .iter()
                .enumerate()
                .filter(|(i, f)| !used[*i] && f.direction == CanDirection::Rx)
                .min_by_key(|(_, f)| f.elapsed_ns.abs_diff(sample.elapsed_ns));
            if let Some((i, frame)) = candidate {
                if frame.elapsed_ns.abs_diff(sample.elapsed_ns)
                    > self.config.correlation_tolerance_ns as u64
                {
                    continue;
                }
                if let Ok(Some(value)) = decode_can_signal(frame, field, source) {
                    used[i] = true;
                    pairs.push((value.value, sample.value));
                }
            }
        }
        if pairs.len() < 3 {
            return Ok(None);
        }
        let n = pairs.len() as f64;
        let mx = pairs.iter().map(|p| p.0).sum::<f64>() / n;
        let my = pairs.iter().map(|p| p.1).sum::<f64>() / n;
        let (mut covariance, mut vx, mut vy) = (0.0, 0.0, 0.0);
        for (x, y) in &pairs {
            covariance += (x - mx) * (y - my);
            vx += (x - mx).powi(2);
            vy += (y - my).powi(2);
        }
        let r = covariance / (vx * vy).sqrt();
        if !r.is_finite() {
            return Ok(None);
        }
        Ok(Some(CanCorrelationHypothesis {
            field_id: field.id.clone(),
            obd_source_id: source.into(),
            obd_ecu_id: ecu.map(String::from),
            obd_signal_id: signal.into(),
            pairs: pairs.len(),
            pearson_r: r.clamp(-1.0, 1.0),
            hypothesis: true,
        }))
    }
}
fn push_bounded<T>(queue: &mut VecDeque<T>, value: T, limit: usize) {
    if queue.len() == limit {
        queue.pop_front();
    }
    queue.push_back(value);
}
fn timing(
    route: &CanDiagnosticRoute,
    pending: Pending,
    t: i64,
    reply: Vec<u8>,
    outcome: CanDiagnosticOutcome,
) -> CanRequestTiming {
    CanRequestTiming {
        route: route.clone(),
        request_elapsed_ns: pending.timestamp,
        response_elapsed_ns: t,
        latency_ns: t.saturating_sub(pending.timestamp),
        request_payload: pending.payload,
        response_payload: reply,
        outcome,
    }
}
fn reply_outcome(request: &[u8], response: &[u8]) -> CanDiagnosticOutcome {
    let Some(&service) = request.first() else {
        return CanDiagnosticOutcome::Malformed;
    };
    if response.first() == Some(&0x7f) {
        if response.len() != 3 {
            return CanDiagnosticOutcome::Malformed;
        }
        if response[1] != service {
            return CanDiagnosticOutcome::WrongEcho;
        }
        return if response[2] == 0x78 {
            CanDiagnosticOutcome::ResponsePending { nrc: 0x78 }
        } else {
            CanDiagnosticOutcome::Negative { nrc: response[2] }
        };
    }
    let echo = if service == 0x22 { 2 } else { 1 };
    if request.len() < echo + 1 || response.len() < echo + 1 {
        return CanDiagnosticOutcome::Malformed;
    }
    if response[0] != service.wrapping_add(0x40) || response[1..=echo] != request[1..=echo] {
        CanDiagnosticOutcome::WrongEcho
    } else {
        CanDiagnosticOutcome::Positive
    }
}
impl Assembly {
    fn feed(&mut self, data: &[u8], timestamp: i64) -> Result<Option<(Vec<u8>, i64)>, ()> {
        let Some(&pci) = data.first() else {
            return Err(());
        };
        match pci >> 4 {
            0 => {
                *self = Self::default();
                let n = (pci & 15) as usize;
                if n == 0 || n + 1 > data.len() {
                    return Err(());
                }
                Ok(Some((data[1..=n].to_vec(), timestamp)))
            }
            1 => {
                *self = Self::default();
                if data.len() < 3 {
                    return Err(());
                }
                self.length = ((pci as usize & 15) << 8) | data[1] as usize;
                if self.length <= 7 {
                    return Err(());
                }
                self.payload.extend_from_slice(&data[2..]);
                self.next_sequence = 1;
                self.first_elapsed_ns = timestamp;
                Ok(None)
            }
            2 => {
                if self.length == 0 || pci & 15 != self.next_sequence || data.len() < 2 {
                    *self = Self::default();
                    return Err(());
                }
                self.next_sequence = (self.next_sequence + 1) & 15;
                let needed = self.length - self.payload.len();
                self.payload
                    .extend_from_slice(&data[1..1 + needed.min(data.len() - 1)]);
                if self.payload.len() == self.length {
                    let p = std::mem::take(&mut self.payload);
                    let timestamp = self.first_elapsed_ns;
                    *self = Self::default();
                    Ok(Some((p, timestamp)))
                } else {
                    Ok(None)
                }
            }
            3 => Ok(None),
            _ => {
                *self = Self::default();
                Err(())
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct CanTriggerConfig {
    pub pre_ns: i64,
    pub post_ns: i64,
    pub max_frames: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanTriggerCapture {
    pub trigger_elapsed_ns: i64,
    pub pre_ns: i64,
    pub post_ns: i64,
    pub frames: Vec<CanFrame>,
    pub truncated: bool,
    pub complete: bool,
}
pub struct CanTriggerRecorder {
    config: CanTriggerConfig,
    pre: VecDeque<CanFrame>,
    capture: Option<CanTriggerCapture>,
    last_time: Option<i64>,
    dropped_at: Option<i64>,
}
impl CanTriggerRecorder {
    pub fn new(config: CanTriggerConfig) -> Result<Self, LogNookError> {
        if config.pre_ns < 0 || config.post_ns < 0 || config.max_frames == 0 {
            return Err(LogNookError::new("Invalid CAN trigger bounds"));
        }
        Ok(Self {
            config,
            pre: VecDeque::new(),
            capture: None,
            last_time: None,
            dropped_at: None,
        })
    }
    pub fn feed(&mut self, frame: &CanFrame) -> Result<(), LogNookError> {
        frame.validate()?;
        if self.last_time.is_some_and(|t| frame.elapsed_ns < t) {
            return Err(LogNookError::new(
                "CAN trigger frames must be in timestamp order",
            ));
        }
        self.last_time = Some(frame.elapsed_ns);
        self.advance_time(frame.elapsed_ns);
        if let Some(c) = &mut self.capture {
            if !c.complete && frame.elapsed_ns >= c.trigger_elapsed_ns {
                if c.frames.len() < self.config.max_frames {
                    c.frames.push(frame.clone());
                } else {
                    c.truncated = true;
                }
                if frame.elapsed_ns == c.trigger_elapsed_ns.saturating_add(self.config.post_ns) {
                    c.complete = true;
                }
            }
        }
        let cutoff = frame.elapsed_ns.saturating_sub(self.config.pre_ns);
        while self.pre.front().is_some_and(|f| f.elapsed_ns < cutoff) {
            self.pre.pop_front();
        }
        if self.pre.len() == self.config.max_frames {
            self.dropped_at = self.pre.pop_front().map(|f| f.elapsed_ns);
        }
        self.pre.push_back(frame.clone());
        Ok(())
    }
    /// Trigger after feeding the frame that caused it. A capture must be taken before rearming.
    pub fn trigger(&mut self, elapsed_ns: i64) -> Result<(), LogNookError> {
        if elapsed_ns < 0
            || self.last_time.is_some_and(|t| elapsed_ns < t)
            || self.capture.is_some()
        {
            return Err(LogNookError::new(
                "Trigger must be current and recorder must be armed",
            ));
        }
        let start = elapsed_ns.saturating_sub(self.config.pre_ns);
        self.capture = Some(CanTriggerCapture {
            trigger_elapsed_ns: elapsed_ns,
            pre_ns: self.config.pre_ns,
            post_ns: self.config.post_ns,
            frames: self
                .pre
                .iter()
                .filter(|f| f.elapsed_ns >= start && f.elapsed_ns <= elapsed_ns)
                .cloned()
                .collect(),
            truncated: self.dropped_at.is_some_and(|t| t >= start),
            complete: self.config.post_ns == 0,
        });
        Ok(())
    }
    pub fn advance_time(&mut self, elapsed_ns: i64) {
        if let Some(c) = &mut self.capture {
            if elapsed_ns > c.trigger_elapsed_ns.saturating_add(self.config.post_ns) {
                c.complete = true;
            }
        }
    }
    /// Taking early explicitly returns an incomplete capture; no synthetic end frame is added.
    pub fn take_capture(&mut self) -> Option<CanTriggerCapture> {
        self.capture.take()
    }
}
