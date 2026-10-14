//! Validated immutable CAN replay sources and bounded saved-capture preparation.
use crate::{CanCaptureReader, CanDirection, CanFrame, LogNookError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SAVED_FRAMES: usize = 10_000;
const MAX_LIVE_FRAMES: usize = 400;
const MAX_GAP_NS: i64 = 60_000_000_000;
const MAX_DURATION_NS: u64 = 600_000_000_000;
const MIN_INTERVAL_NS: u64 = 1_000_000;
const CANCELLATION_BATCH_ROWS: u64 = 256;

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct SavedCanReplaySelection {
    pub can_id: u32,
    pub extended: bool,
    pub start_elapsed_ns: i64,
    pub end_elapsed_ns: i64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct SavedCanReplaySummary {
    pub source_sha256: String,
    pub can_id: u32,
    pub extended: bool,
    pub start_elapsed_ns: i64,
    pub end_elapsed_ns: i64,
    pub first_row: u64,
    pub last_row: u64,
    pub first_elapsed_ns: i64,
    pub last_elapsed_ns: i64,
    /// Eligible unflagged RX frames selected for replay.
    pub matching_rx_count: usize,
    /// Matching TX rows in the selected time range, including flagged TX rows.
    pub excluded_tx_count: usize,
    /// Matching flagged RX rows in the selected time range.
    pub excluded_flagged_count: usize,
    pub source_span_ns: u64,
    pub paced_duration_ns: u64,
    pub source_bitrate: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CanReplaySource {
    LiveSnapshot,
    SavedCapture { summary: SavedCanReplaySummary },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CanReplayFrame {
    /// Physical source row, including the CSV header as row 1; `None` for a live snapshot.
    pub source_row: Option<u64>,
    pub frame: CanFrame,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanReplayPlan {
    source: CanReplaySource,
    frames: Vec<CanReplayFrame>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedSavedCanReplay {
    pub plan: CanReplayPlan,
    pub summary: SavedCanReplaySummary,
}

impl CanReplayPlan {
    /// Compatibility plan for the existing live-ring replay API.
    pub fn live_snapshot(frames: Vec<CanFrame>) -> Result<Self, LogNookError> {
        validate_frames(&frames, MAX_LIVE_FRAMES)?;
        Ok(Self {
            source: CanReplaySource::LiveSnapshot,
            frames: frames
                .into_iter()
                .map(|frame| CanReplayFrame {
                    source_row: None,
                    frame,
                })
                .collect(),
        })
    }

    pub fn source(&self) -> &CanReplaySource {
        &self.source
    }

    pub fn frames(&self) -> &[CanReplayFrame] {
        &self.frames
    }

    pub fn saved_summary(&self) -> Option<&SavedCanReplaySummary> {
        match &self.source {
            CanReplaySource::LiveSnapshot => None,
            CanReplaySource::SavedCapture { summary } => Some(summary),
        }
    }

    /// Attach sidecar metadata parsed by the desktop boundary without changing captured bytes.
    pub fn with_source_bitrate(mut self, bitrate: Option<u32>) -> Self {
        if let CanReplaySource::SavedCapture { summary } = &mut self.source {
            summary.source_bitrate = bitrate;
        }
        self
    }

    /// Recheck all frame invariants at the final owner boundary before a worker can transmit.
    pub(crate) fn validate_for_worker(&self) -> Result<(u32, bool), LogNookError> {
        let frames = self
            .frames
            .iter()
            .map(|item| item.frame.clone())
            .collect::<Vec<_>>();
        let (can_id, extended) = match &self.source {
            CanReplaySource::LiveSnapshot => validate_frames(&frames, MAX_LIVE_FRAMES)?,
            CanReplaySource::SavedCapture { summary } => {
                validate_saved_summary(summary, &self.frames)?
            }
        };
        Ok((can_id, extended))
    }
}

impl PreparedSavedCanReplay {
    /// The parsed journal bitrate is trusted metadata; it does not alter the frozen sequence.
    pub fn with_source_bitrate(mut self, bitrate: Option<u32>) -> Self {
        self.summary.source_bitrate = bitrate;
        self.plan = self.plan.with_source_bitrate(bitrate);
        self
    }
}

pub fn prepare_saved_can_replay(
    path: &Path,
    selection: SavedCanReplaySelection,
    cancelled: &AtomicBool,
) -> Result<PreparedSavedCanReplay, LogNookError> {
    validate_selection(selection)?;
    if cancelled.load(Ordering::Acquire) {
        return Err(LogNookError::new(
            "Saved CAN replay preparation was cancelled",
        ));
    }
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(LogNookError::new("Saved CAN replay source is not a file"));
    }
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(LogNookError::new("Saved CAN replay source exceeds 256 MiB"));
    }

    let file = File::open(path)?;
    let mut reader = CanCaptureReader::from_reader(BoundedHashReader::new(file))?;
    let mut replay_frames = Vec::new();
    let mut eligible_count = 0usize;
    let mut excluded_tx_count = 0usize;
    let mut excluded_flagged_count = 0usize;
    let mut previous_matching_ns = None;
    let mut physical_row = 1u64;
    let mut source_over_limit = false;

    while let Some(row) = reader.next() {
        physical_row = physical_row.saturating_add(1);
        let frame = row.map_err(|error| LogNookError::new(error.to_string()))?;
        if physical_row % CANCELLATION_BATCH_ROWS == 0 && cancelled.load(Ordering::Acquire) {
            return Err(LogNookError::new(
                "Saved CAN replay preparation was cancelled",
            ));
        }
        if frame.can_id == selection.can_id && frame.extended == selection.extended {
            if previous_matching_ns.is_some_and(|previous| frame.elapsed_ns < previous) {
                return Err(LogNookError::new(
                    "Matching CAN capture rows are not in timestamp order",
                ));
            }
            previous_matching_ns = Some(frame.elapsed_ns);
            if !(selection.start_elapsed_ns..=selection.end_elapsed_ns).contains(&frame.elapsed_ns)
            {
                continue;
            }
            if frame.direction == CanDirection::Tx {
                excluded_tx_count = excluded_tx_count
                    .checked_add(1)
                    .ok_or_else(|| LogNookError::new("Saved CAN replay row count overflow"))?;
                continue;
            }
            if frame.flags != 0 {
                excluded_flagged_count = excluded_flagged_count
                    .checked_add(1)
                    .ok_or_else(|| LogNookError::new("Saved CAN replay row count overflow"))?;
                continue;
            }
            eligible_count = eligible_count
                .checked_add(1)
                .ok_or_else(|| LogNookError::new("Saved CAN replay frame count overflow"))?;
            if eligible_count <= MAX_SAVED_FRAMES {
                replay_frames.push(CanReplayFrame {
                    source_row: Some(physical_row),
                    frame,
                });
            } else {
                source_over_limit = true;
            }
        }
    }
    if cancelled.load(Ordering::Acquire) {
        return Err(LogNookError::new(
            "Saved CAN replay preparation was cancelled",
        ));
    }
    if source_over_limit {
        return Err(LogNookError::new(
            "Saved CAN replay must contain 1-10000 selected RX frames; narrow the time range",
        ));
    }
    if replay_frames.is_empty() {
        return Err(LogNookError::new(
            "Saved CAN replay selection contains no matching unflagged RX frames",
        ));
    }

    let source_reader = reader.into_inner();
    let (source_sha256, bytes_read) = source_reader.finish();
    if bytes_read > MAX_SOURCE_BYTES {
        return Err(LogNookError::new("Saved CAN replay source exceeds 256 MiB"));
    }
    let (source_span_ns, paced_duration_ns) = validate_saved_durations(&replay_frames)?;
    let summary = SavedCanReplaySummary {
        source_sha256,
        can_id: selection.can_id,
        extended: selection.extended,
        start_elapsed_ns: selection.start_elapsed_ns,
        end_elapsed_ns: selection.end_elapsed_ns,
        first_row: replay_frames
            .first()
            .and_then(|frame| frame.source_row)
            .unwrap_or(0),
        last_row: replay_frames
            .last()
            .and_then(|frame| frame.source_row)
            .unwrap_or(0),
        first_elapsed_ns: replay_frames[0].frame.elapsed_ns,
        last_elapsed_ns: replay_frames.last().unwrap().frame.elapsed_ns,
        matching_rx_count: eligible_count,
        excluded_tx_count,
        excluded_flagged_count,
        source_span_ns,
        paced_duration_ns,
        source_bitrate: None,
    };
    let plan = CanReplayPlan {
        source: CanReplaySource::SavedCapture {
            summary: summary.clone(),
        },
        frames: replay_frames,
    };
    Ok(PreparedSavedCanReplay { plan, summary })
}

fn validate_selection(selection: SavedCanReplaySelection) -> Result<(), LogNookError> {
    let max_id = if selection.extended {
        0x1fff_ffff
    } else {
        0x7ff
    };
    if selection.can_id > max_id {
        return Err(LogNookError::new(
            "Saved CAN replay identifier exceeds its selected format",
        ));
    }
    if selection.start_elapsed_ns < 0 || selection.end_elapsed_ns < selection.start_elapsed_ns {
        return Err(LogNookError::new(
            "Saved CAN replay start/end range is invalid",
        ));
    }
    Ok(())
}

fn validate_frames(frames: &[CanFrame], max_count: usize) -> Result<(u32, bool), LogNookError> {
    if frames.is_empty() || frames.len() > max_count {
        return Err(LogNookError::new(format!(
            "CAN ID replay must contain 1-{max_count} captured frames"
        )));
    }
    let first = &frames[0];
    first.validate()?;
    let identity = (first.can_id, first.extended);
    let mut previous_elapsed_ns = first.elapsed_ns;
    for frame in frames {
        frame.validate()?;
        if (frame.can_id, frame.extended) != identity {
            return Err(LogNookError::new(
                "CAN ID replay frames must use one exact identifier and format",
            ));
        }
        if frame.direction != CanDirection::Rx || frame.flags != 0 {
            return Err(LogNookError::new(
                "CAN ID replay accepts only unflagged received classic data frames",
            ));
        }
        if frame.elapsed_ns < previous_elapsed_ns {
            return Err(LogNookError::new(
                "CAN ID replay source frames must be in timestamp order",
            ));
        }
        let interval_ns = frame
            .elapsed_ns
            .checked_sub(previous_elapsed_ns)
            .ok_or_else(|| LogNookError::new("CAN ID replay interval overflow"))?;
        if interval_ns > MAX_GAP_NS {
            return Err(LogNookError::new(
                "CAN ID replay source gaps must be at most 60000 ms",
            ));
        }
        previous_elapsed_ns = frame.elapsed_ns;
    }
    Ok(identity)
}

fn validate_saved_summary(
    summary: &SavedCanReplaySummary,
    frames: &[CanReplayFrame],
) -> Result<(u32, bool), LogNookError> {
    let raw_frames = frames
        .iter()
        .map(|item| item.frame.clone())
        .collect::<Vec<_>>();
    let identity = validate_frames(&raw_frames, MAX_SAVED_FRAMES)?;
    if identity != (summary.can_id, summary.extended)
        || frames.len() != summary.matching_rx_count
        || frames.first().and_then(|item| item.source_row) != Some(summary.first_row)
        || frames.last().and_then(|item| item.source_row) != Some(summary.last_row)
        || frames.first().map(|item| item.frame.elapsed_ns) != Some(summary.first_elapsed_ns)
        || frames.last().map(|item| item.frame.elapsed_ns) != Some(summary.last_elapsed_ns)
    {
        return Err(LogNookError::new(
            "Saved CAN replay provenance does not match its frames",
        ));
    }
    let (source_span_ns, paced_duration_ns) = validate_saved_durations(frames)?;
    if source_span_ns != summary.source_span_ns || paced_duration_ns != summary.paced_duration_ns {
        return Err(LogNookError::new(
            "Saved CAN replay duration summary does not match its frames",
        ));
    }
    Ok(identity)
}

fn validate_saved_durations(frames: &[CanReplayFrame]) -> Result<(u64, u64), LogNookError> {
    let first = frames
        .first()
        .ok_or_else(|| LogNookError::new("Saved CAN replay contains no frames"))?;
    let last = frames.last().unwrap();
    let source_span = last
        .frame
        .elapsed_ns
        .checked_sub(first.frame.elapsed_ns)
        .ok_or_else(|| LogNookError::new("Saved CAN replay source span overflow"))?;
    let mut paced_duration_ns = 0u64;
    for pair in frames.windows(2) {
        let gap = pair[1]
            .frame
            .elapsed_ns
            .checked_sub(pair[0].frame.elapsed_ns)
            .ok_or_else(|| LogNookError::new("Saved CAN replay source gap overflow"))?;
        if gap > MAX_GAP_NS {
            return Err(LogNookError::new(
                "Saved CAN replay source gaps must be at most 60000 ms",
            ));
        }
        paced_duration_ns = paced_duration_ns
            .checked_add((gap as u64).max(MIN_INTERVAL_NS))
            .ok_or_else(|| LogNookError::new("Saved CAN replay paced duration overflow"))?;
    }
    if source_span as u64 > MAX_DURATION_NS || paced_duration_ns > MAX_DURATION_NS {
        return Err(LogNookError::new(
            "Saved CAN replay source span and paced duration must each be at most 600 seconds",
        ));
    }
    Ok((source_span as u64, paced_duration_ns))
}

struct BoundedHashReader<R> {
    reader: R,
    hasher: Sha256,
    bytes_read: u64,
}

impl<R> BoundedHashReader<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            hasher: Sha256::new(),
            bytes_read: 0,
        }
    }

    fn finish(self) -> (String, u64) {
        let digest = self.hasher.finalize();
        let hash = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        (hash, self.bytes_read)
    }
}

impl<R: Read> Read for BoundedHashReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let remaining = MAX_SOURCE_BYTES.saturating_sub(self.bytes_read);
        let allowed = buffer
            .len()
            .min(usize::try_from(remaining.saturating_add(1)).unwrap_or(usize::MAX));
        let bytes = self.reader.read(&mut buffer[..allowed])?;
        if bytes as u64 > remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Saved CAN replay source exceeds 256 MiB",
            ));
        }
        self.hasher.update(&buffer[..bytes]);
        self.bytes_read += bytes as u64;
        Ok(bytes)
    }
}
