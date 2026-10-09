use super::protocol::{
    AgentCanFrame, AgentLogKind, AgentLogPage, AgentLogQueryArgs, AgentLogSource, AgentMeasurement,
};
use crate::AppState;
use lognook_core::{CanCaptureReader, CanDirection, CanFrame, EngineEvent, Sample, SampleOrigin};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

const LIVE_BUFFER_CAPACITY: usize = 20_000;
pub(crate) const MAX_LOG_QUERY_ROWS: usize = 1_000;

#[derive(Default)]
pub(crate) struct AgentLogBuffer {
    samples: VecDeque<Sample>,
    can_frames: VecDeque<CanFrame>,
    obd_connected: bool,
    can_connected: bool,
}

pub(crate) struct LogPage<T> {
    pub items: Vec<T>,
    pub truncated: bool,
}

impl AgentLogBuffer {
    pub(crate) fn record_engine_event(&mut self, event: &EngineEvent) {
        match event {
            EngineEvent::Connected(_) => {
                self.start_session_if_idle();
                self.obd_connected = true;
            }
            EngineEvent::Disconnected => self.obd_connected = false,
            EngineEvent::CanWorkbenchStatus(status) => {
                if status.connected {
                    self.start_session_if_idle();
                }
                self.can_connected = status.connected;
            }
            EngineEvent::Sample(sample) => self.record_sample(sample.clone()),
            EngineEvent::CanFrames(frames) => {
                for frame in frames {
                    self.record_can_frame(frame.clone());
                }
            }
            _ => {}
        }
    }

    fn start_session_if_idle(&mut self) {
        if !self.obd_connected && !self.can_connected {
            self.samples.clear();
            self.can_frames.clear();
        }
    }

    pub(crate) fn record_sample(&mut self, sample: Sample) {
        if sample.origin != SampleOrigin::Live {
            return;
        }
        push_bounded(&mut self.samples, sample);
    }

    pub(crate) fn record_can_frame(&mut self, frame: CanFrame) {
        push_bounded(&mut self.can_frames, frame);
    }

    pub(crate) fn sample_range(&self) -> Option<(i64, i64, usize)> {
        Some((
            self.samples.iter().map(|sample| sample.elapsed_ns).min()?,
            self.samples.iter().map(|sample| sample.elapsed_ns).max()?,
            self.samples.len(),
        ))
    }

    pub(crate) fn can_range(&self) -> Option<(i64, i64, usize)> {
        Some((
            self.can_frames.front()?.elapsed_ns,
            self.can_frames.back()?.elapsed_ns,
            self.can_frames.len(),
        ))
    }

    pub(crate) fn query_samples(
        &self,
        start_ns: i64,
        end_ns: i64,
        signal_ids: &[String],
        limit: usize,
    ) -> LogPage<Sample> {
        query_bounded(&self.samples, start_ns, end_ns, limit, |sample| {
            signal_ids.is_empty() || signal_ids.iter().any(|id| id == &sample.signal_id)
        })
    }

    pub(crate) fn query_can_frames(
        &self,
        start_ns: i64,
        end_ns: i64,
        can_ids: &[u32],
        limit: usize,
    ) -> LogPage<CanFrame> {
        query_bounded(&self.can_frames, start_ns, end_ns, limit, |frame| {
            can_ids.is_empty() || can_ids.contains(&frame.can_id)
        })
    }
}

pub(crate) fn list_log_sources(state: &AppState) -> Result<Vec<AgentLogSource>, String> {
    let mut sources = Vec::new();
    let live = state
        .agent_log_buffer
        .lock()
        .map_err(|_| "Live log buffer is unavailable".to_owned())?;
    if let Some((start, end, count)) = live.sample_range() {
        sources.push(AgentLogSource {
            source_id: "live:pids".into(),
            name: "Live PID samples".into(),
            kind: AgentLogKind::Pids,
            recorded: false,
            start_elapsed_ms: Some(ns_to_ms(start)),
            end_elapsed_ms: Some(ns_to_ms(end)),
            item_count: Some(count),
        });
    }
    if let Some((start, end, count)) = live.can_range() {
        sources.push(AgentLogSource {
            source_id: "live:can".into(),
            name: "Live CAN frames".into(),
            kind: AgentLogKind::Can,
            recorded: false,
            start_elapsed_ms: Some(ns_to_ms(start)),
            end_elapsed_ms: Some(ns_to_ms(end)),
            item_count: Some(count),
        });
    }
    drop(live);

    if let Ok(entries) = std::fs::read_dir(&state.recordings_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !has_extension(&path, "lognook") {
                continue;
            }
            let Ok(info) = lognook_core::inspect_recording(&path) else {
                continue;
            };
            sources.push(AgentLogSource {
                source_id: source_id(&path),
                name: path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                kind: AgentLogKind::Pids,
                recorded: true,
                start_elapsed_ms: Some(0),
                end_elapsed_ms: Some(ns_to_ms(info.duration_ns.max(0))),
                item_count: Some(info.sample_count),
            });
        }
    }

    let can_dir = state
        .recordings_dir
        .parent()
        .map(|directory| directory.join("agent-traces").join("can"));
    if let Some(can_dir) = can_dir
        && let Ok(entries) = std::fs::read_dir(can_dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_can_capture(&path) {
                continue;
            }
            let Ok((start, end, count)) = can_capture_bounds(&path) else {
                continue;
            };
            sources.push(AgentLogSource {
                source_id: source_id(&path),
                name: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                kind: AgentLogKind::Can,
                recorded: true,
                start_elapsed_ms: Some(ns_to_ms(start)),
                end_elapsed_ms: Some(ns_to_ms(end)),
                item_count: Some(count),
            });
        }
    }
    Ok(sources)
}

pub(crate) fn query_log_range(
    state: &AppState,
    query: AgentLogQueryArgs,
) -> Result<AgentLogPage, String> {
    if query.start_elapsed_ms > query.end_elapsed_ms {
        return Err("Log range start must not exceed its end".into());
    }
    if query.end_elapsed_ms > (i64::MAX as u64 / 1_000_000)
        || query.signal_ids.len() > 500
        || query.can_ids.len() > 500
    {
        return Err("Log query exceeds the supported time or filter limit".into());
    }
    match query.kind {
        AgentLogKind::Pids if !query.can_ids.is_empty() => {
            return Err("CAN IDs cannot filter PID samples".into());
        }
        AgentLogKind::Can if !query.signal_ids.is_empty() => {
            return Err("PID IDs cannot filter CAN frames".into());
        }
        _ => {}
    }
    let limit = query.limit.clamp(1, MAX_LOG_QUERY_ROWS);
    let start_ns = ms_to_ns(query.start_elapsed_ms);
    let end_ns = ms_to_ns(query.end_elapsed_ms);
    let (pid_samples, can_frames, truncated) = match (query.kind, query.source_id.as_str()) {
        (AgentLogKind::Pids, "live:pids") => {
            let buffer = state
                .agent_log_buffer
                .lock()
                .map_err(|_| "Live log buffer is unavailable".to_owned())?;
            let page = buffer.query_samples(start_ns, end_ns, &query.signal_ids, limit);
            (
                page.items.into_iter().map(AgentMeasurement::from).collect(),
                Vec::new(),
                page.truncated,
            )
        }
        (AgentLogKind::Can, "live:can") => {
            let buffer = state
                .agent_log_buffer
                .lock()
                .map_err(|_| "Live log buffer is unavailable".to_owned())?;
            let page = buffer.query_can_frames(start_ns, end_ns, &query.can_ids, limit);
            (
                Vec::new(),
                page.items.into_iter().map(AgentCanFrame::from).collect(),
                page.truncated,
            )
        }
        (AgentLogKind::Pids, _) => {
            let path = recorded_path(&query.source_id, AgentLogKind::Pids)?;
            let mut samples = lognook_core::query_recording_samples(
                &path,
                &query.signal_ids,
                start_ns,
                end_ns,
                limit + 1,
            )
            .map_err(|error| error.to_string())?;
            let truncated = samples.len() > limit;
            samples.truncate(limit);
            (
                samples.into_iter().map(AgentMeasurement::from).collect(),
                Vec::new(),
                truncated,
            )
        }
        (AgentLogKind::Can, _) => {
            let path = recorded_path(&query.source_id, AgentLogKind::Can)?;
            let page = query_can_capture(&path, start_ns, end_ns, &query.can_ids, limit)?;
            (
                Vec::new(),
                page.items.into_iter().map(AgentCanFrame::from).collect(),
                page.truncated,
            )
        }
    };
    Ok(AgentLogPage {
        source_id: query.source_id,
        kind: query.kind,
        start_elapsed_ms: query.start_elapsed_ms,
        end_elapsed_ms: query.end_elapsed_ms,
        truncated,
        pid_samples,
        can_frames,
    })
}

fn recorded_path(source_id: &str, kind: AgentLogKind) -> Result<PathBuf, String> {
    let path = Path::new(source_id)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !path.is_file() {
        return Err("Log source is not a file".into());
    }
    let supported = match kind {
        AgentLogKind::Pids => has_extension(&path, "lognook"),
        AgentLogKind::Can => is_can_capture(&path),
    };
    if !supported {
        return Err("Log source type does not match the requested data".into());
    }
    Ok(path)
}

fn is_can_capture(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(".candump.csv")
    })
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .is_some_and(|value| value.to_string_lossy().eq_ignore_ascii_case(extension))
}

fn can_capture_bounds(path: &Path) -> Result<(i64, i64, usize), String> {
    // ponytail: scan on explicit source listing for exact bounds; add a sidecar index if measured latency needs it.
    let mut reader = CanCaptureReader::open(path).map_err(|error| error.to_string())?;
    let mut first = None;
    let mut last = 0;
    let mut count = 0;
    for row in &mut reader {
        let frame = row.map_err(|error| error.to_string())?;
        first.get_or_insert(frame.elapsed_ns);
        last = frame.elapsed_ns;
        count += 1;
    }
    first
        .map(|start| (start, last, count))
        .ok_or_else(|| "CAN capture contains no frames".into())
}

fn query_can_capture(
    path: &Path,
    start_ns: i64,
    end_ns: i64,
    can_ids: &[u32],
    limit: usize,
) -> Result<LogPage<CanFrame>, String> {
    let mut reader = CanCaptureReader::open(path).map_err(|error| error.to_string())?;
    let mut frames = VecDeque::with_capacity(limit);
    let mut truncated = false;
    for row in &mut reader {
        let frame = row.map_err(|error| error.to_string())?;
        if frame.elapsed_ns < start_ns
            || frame.elapsed_ns > end_ns
            || (!can_ids.is_empty() && !can_ids.contains(&frame.can_id))
        {
            continue;
        }
        if frames.len() == limit {
            frames.pop_front();
            truncated = true;
        }
        frames.push_back(frame);
    }
    Ok(LogPage {
        items: frames.into_iter().collect(),
        truncated,
    })
}

fn source_id(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn ms_to_ns(ms: u64) -> i64 {
    (i128::from(ms) * 1_000_000).min(i128::from(i64::MAX)) as i64
}

fn ns_to_ms(ns: i64) -> u64 {
    ns.max(0) as u64 / 1_000_000
}

impl From<Sample> for AgentMeasurement {
    fn from(sample: Sample) -> Self {
        Self {
            elapsed_ms: ns_to_ms(sample.elapsed_ns),
            wall_time_unix_ms: ns_to_ms(sample.wall_time_unix_ns),
            source_id: sample.source_id,
            ecu_id: sample.ecu_id,
            signal_id: sample.signal_id,
            value: sample.value,
            unit: sample.unit,
            quality: format!("{:?}", sample.quality).to_ascii_lowercase(),
        }
    }
}

impl From<CanFrame> for AgentCanFrame {
    fn from(frame: CanFrame) -> Self {
        Self {
            elapsed_ms: ns_to_ms(frame.elapsed_ns),
            wall_time_unix_ms: ns_to_ms(frame.wall_time_unix_ns),
            can_id: frame.can_id,
            extended: frame.extended,
            direction: match frame.direction {
                CanDirection::Rx => "rx",
                CanDirection::Tx => "tx",
            }
            .into(),
            data: frame.data,
            flags: frame.flags,
        }
    }
}

fn push_bounded<T>(items: &mut VecDeque<T>, item: T) {
    if items.len() == LIVE_BUFFER_CAPACITY {
        items.pop_front();
    }
    items.push_back(item);
}

// ponytail: scan at most 20k in-memory rows per query; add indexes only if measured latency needs it.
fn query_bounded<T: Clone>(
    items: &VecDeque<T>,
    start_ns: i64,
    end_ns: i64,
    limit: usize,
    matches: impl Fn(&T) -> bool,
) -> LogPage<T>
where
    T: HasElapsed,
{
    let limit = limit.min(MAX_LOG_QUERY_ROWS);
    let mut result = Vec::with_capacity(limit);
    let mut truncated = false;
    if start_ns <= end_ns && limit > 0 {
        for item in items.iter().rev().filter(|item| {
            let elapsed = item.elapsed_ns();
            elapsed >= start_ns && elapsed <= end_ns && matches(item)
        }) {
            if result.len() == limit {
                truncated = true;
                break;
            }
            result.push(item.clone());
        }
    }
    result.reverse();
    result.sort_by_key(HasElapsed::elapsed_ns);
    LogPage {
        items: result,
        truncated,
    }
}

trait HasElapsed {
    fn elapsed_ns(&self) -> i64;
}

impl HasElapsed for Sample {
    fn elapsed_ns(&self) -> i64 {
        self.elapsed_ns
    }
}

impl HasElapsed for CanFrame {
    fn elapsed_ns(&self) -> i64 {
        self.elapsed_ns
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentLogBuffer, query_can_capture};
    use lognook_core::{
        CanCaptureWriter, CanDirection, CanFrame, Sample, SampleOrigin, SampleQuality,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sample(elapsed_ns: i64, signal_id: &str, value: f64) -> Sample {
        Sample {
            elapsed_ns,
            wall_time_unix_ns: elapsed_ns,
            source_id: "simulator".into(),
            ecu_id: None,
            signal_id: signal_id.into(),
            value,
            unit: "rpm".into(),
            quality: SampleQuality::Valid,
            origin: SampleOrigin::Live,
            raw_bytes: None,
        }
    }

    #[test]
    fn mixed_source_results_have_sorted_time_bounds() {
        let mut buffer = AgentLogBuffer::default();
        for elapsed in [30, 10, 20] {
            buffer.record_sample(sample(elapsed, "engine_rpm", 1.0));
        }
        assert_eq!(buffer.sample_range(), Some((10, 30, 3)));
        assert_eq!(
            buffer
                .query_samples(0, 40, &[], 10)
                .items
                .iter()
                .map(|sample| sample.elapsed_ns)
                .collect::<Vec<_>>(),
            [10, 20, 30]
        );
    }

    #[test]
    fn a_second_source_preserves_logs_and_a_new_session_clears_them() {
        use lognook_core::{CanWorkbenchStatus, EngineEvent, HardwareAdapter, SimulatedObdAdapter};
        let mut buffer = AgentLogBuffer::default();
        let caps = SimulatedObdAdapter::default().connect().unwrap();
        let can = |connected| {
            EngineEvent::CanWorkbenchStatus(CanWorkbenchStatus {
                connected,
                bitrate: 500_000,
                listen_only: true,
                source_id: "can:test".into(),
            })
        };
        buffer.record_engine_event(&EngineEvent::Connected(caps.clone()));
        buffer.record_sample(sample(1, "engine_rpm", 1.0));
        buffer.record_engine_event(&can(true));
        buffer.record_engine_event(&EngineEvent::Disconnected);
        assert_eq!(buffer.sample_range(), Some((1, 1, 1)));
        buffer.record_engine_event(&can(false));
        assert!(buffer.sample_range().is_some());
        buffer.record_engine_event(&EngineEvent::Connected(caps));
        assert!(buffer.sample_range().is_none());
    }

    #[test]
    fn live_pid_query_filters_time_and_signal_and_reports_truncation() {
        let mut buffer = AgentLogBuffer::default();
        buffer.record_sample(sample(10, "engine_rpm", 100.0));
        buffer.record_sample(sample(20, "coolant_temp", 90.0));
        buffer.record_sample(sample(30, "engine_rpm", 300.0));
        buffer.record_sample(sample(40, "engine_rpm", 400.0));

        let page = buffer.query_samples(10, 40, &["engine_rpm".into()], 2);
        assert_eq!(
            page.items
                .iter()
                .map(|sample| (sample.elapsed_ns, sample.value))
                .collect::<Vec<_>>(),
            [(30, 300.0), (40, 400.0)]
        );
        assert!(page.truncated);
    }

    #[test]
    fn live_can_query_filters_time_and_identifier() {
        let mut buffer = AgentLogBuffer::default();
        for (elapsed_ns, can_id) in [(10, 0x100), (20, 0x200), (30, 0x100)] {
            buffer.record_can_frame(CanFrame {
                elapsed_ns,
                wall_time_unix_ns: elapsed_ns,
                can_id,
                extended: false,
                direction: CanDirection::Rx,
                data: vec![1],
                flags: 0,
            });
        }

        let page = buffer.query_can_frames(15, 30, &[0x100], 10);
        assert_eq!(
            page.items
                .iter()
                .map(|frame| (frame.elapsed_ns, frame.can_id))
                .collect::<Vec<_>>(),
            [(30, 0x100)]
        );
        assert!(!page.truncated);
    }

    #[test]
    fn recorded_can_query_returns_the_latest_matching_range() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lognook-range-{nonce}.candump.csv"));
        let mut writer = CanCaptureWriter::create_new(&path).unwrap();
        for (elapsed_ns, can_id) in [(10, 0x100), (20, 0x200), (30, 0x100), (40, 0x100)] {
            writer
                .write_frame(&CanFrame {
                    elapsed_ns,
                    wall_time_unix_ns: elapsed_ns,
                    can_id,
                    extended: false,
                    direction: CanDirection::Rx,
                    data: vec![1],
                    flags: 0,
                })
                .unwrap();
        }
        writer.finish().unwrap();

        let page = query_can_capture(&path, 10, 40, &[0x100], 2).unwrap();
        assert_eq!(
            page.items
                .iter()
                .map(|frame| frame.elapsed_ns)
                .collect::<Vec<_>>(),
            [30, 40]
        );
        assert!(page.truncated);
        let _ = std::fs::remove_file(path);
    }
}
