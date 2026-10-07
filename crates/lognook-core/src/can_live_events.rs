//! Incremental, bounded before/after analysis for live manually marked CAN events.
use crate::{
    CanDirection, CanEventCandidate, CanEventCorrelator, CanFrame, CanManualEventMark, LogNookError,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_PRE_EVENT_NS: i64 = 60_000_000_000;
const MAX_HISTORY_FRAMES: usize = 50_000;
const MAX_FRAMES_PER_EVENT: usize = 10_000;
const MAX_PENDING_EVENTS: usize = 32;
const MAX_TOTAL_EVENTS: usize = 256;
const MAX_LABELS: usize = 64;
const MAX_CANDIDATES_PER_LABEL: usize = 1_024;
const MAX_VISIBLE_CANDIDATES: usize = 100;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct CanLiveEventSnapshot {
    pub capture_id: u64,
    pub source_id: String,
    pub active: bool,
    pub groups: Vec<CanLiveEventGroup>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct CanLiveEventGroup {
    pub label: String,
    pub completed_events: usize,
    pub pending_events: usize,
    pub incomplete_events: usize,
    pub omitted_candidates: usize,
    pub candidates: Vec<CanLiveEventCandidate>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CanLiveEventCandidate {
    pub correlation: CanEventCandidate,
    pub sample_frame: Option<CanFrame>,
    pub counter_like_bytes: Vec<u8>,
    pub sum_checksum_like_bytes: Vec<u8>,
}

#[derive(Default)]
struct CandidateTotals {
    observed_events: usize,
    changed_events: usize,
    count_change_events: usize,
    before_frames: u64,
    after_frames: u64,
    before_window_ns: u64,
    after_window_ns: u64,
    sample_frame: Option<CanFrame>,
    counter_like_bytes: BTreeSet<u8>,
    sum_checksum_like_bytes: BTreeSet<u8>,
    transitions: BTreeMap<(u8, Option<u8>, Option<u8>), usize>,
}

#[derive(Default)]
struct GroupTotals {
    completed_events: usize,
    incomplete_events: usize,
    omitted_candidates: usize,
    candidates: BTreeMap<(u32, bool), CandidateTotals>,
}

struct PendingEvent {
    mark: CanManualEventMark,
    frames: Vec<CanFrame>,
    truncated: bool,
}

/// Live analyzer for explicit before/after marks. Raw capture remains the full evidence source.
pub struct LiveCanEventAnalyzer {
    capture_id: u64,
    source_id: String,
    active: bool,
    history: VecDeque<CanFrame>,
    dropped_through_ns: Option<i64>,
    last_elapsed_ns: Option<i64>,
    pending: Vec<PendingEvent>,
    totals: BTreeMap<String, GroupTotals>,
    total_events: usize,
}

impl LiveCanEventAnalyzer {
    pub fn new(capture_id: u64, source_id: impl Into<String>) -> Self {
        Self {
            capture_id,
            source_id: source_id.into(),
            active: true,
            history: VecDeque::with_capacity(MAX_HISTORY_FRAMES),
            dropped_through_ns: None,
            last_elapsed_ns: None,
            pending: Vec::new(),
            totals: BTreeMap::new(),
            total_events: 0,
        }
    }

    pub fn mark(&mut self, mark: CanManualEventMark) -> Result<(), LogNookError> {
        self.validate_mark(&mark)?;

        let start_ns = mark.elapsed_ns.saturating_sub(mark.pre_ns);
        let mut truncated = self
            .dropped_through_ns
            .is_some_and(|dropped| dropped >= start_ns);
        let mut frames = self
            .history
            .iter()
            .filter(|frame| frame.elapsed_ns >= start_ns && frame.elapsed_ns < mark.elapsed_ns)
            .cloned()
            .collect::<Vec<_>>();
        if frames.len() > MAX_FRAMES_PER_EVENT {
            frames.drain(..frames.len() - MAX_FRAMES_PER_EVENT);
            truncated = true;
        }
        self.total_events += 1;
        self.totals.entry(mark.label.clone()).or_default();
        self.pending.push(PendingEvent {
            mark,
            frames,
            truncated,
        });
        Ok(())
    }

    pub(crate) fn validate_mark(&self, mark: &CanManualEventMark) -> Result<(), LogNookError> {
        CanEventCorrelator::new(std::slice::from_ref(mark))?;
        if self
            .last_elapsed_ns
            .is_some_and(|last| mark.elapsed_ns < last)
        {
            return Err(LogNookError::new(
                "CAN event mark precedes the latest received frame",
            ));
        }
        if self.total_events >= MAX_TOTAL_EVENTS {
            return Err(LogNookError::new(
                "This capture has reached its 256 event analysis limit",
            ));
        }
        if self.pending.len() >= MAX_PENDING_EVENTS {
            return Err(LogNookError::new(
                "Too many CAN event windows are still pending",
            ));
        }
        if !self.totals.contains_key(&mark.label) && self.totals.len() >= MAX_LABELS {
            return Err(LogNookError::new(
                "This capture has reached its 64 event label limit",
            ));
        }
        Ok(())
    }

    pub fn feed(&mut self, frame: &CanFrame) -> Result<(), LogNookError> {
        frame.validate()?;
        if frame.direction != CanDirection::Rx {
            return Ok(());
        }
        if self
            .last_elapsed_ns
            .is_some_and(|last| frame.elapsed_ns < last)
        {
            return Err(LogNookError::new(
                "Live CAN event frames must be in timestamp order",
            ));
        }
        self.last_elapsed_ns = Some(frame.elapsed_ns);
        for event in &mut self.pending {
            let end_ns = event.mark.elapsed_ns.saturating_add(event.mark.post_ns);
            if frame.elapsed_ns >= event.mark.elapsed_ns && frame.elapsed_ns <= end_ns {
                if event.frames.len() < MAX_FRAMES_PER_EVENT {
                    event.frames.push(frame.clone());
                } else {
                    event.truncated = true;
                }
            }
        }

        let cutoff = frame.elapsed_ns.saturating_sub(MAX_PRE_EVENT_NS);
        while self
            .history
            .front()
            .is_some_and(|old| old.elapsed_ns < cutoff)
        {
            self.history.pop_front();
        }
        if self.history.len() == MAX_HISTORY_FRAMES {
            if let Some(dropped) = self.history.pop_front() {
                self.dropped_through_ns = Some(dropped.elapsed_ns);
            }
        }
        self.history.push_back(frame.clone());
        Ok(())
    }

    /// Completes windows by monotonic session time, even when the bus goes silent.
    pub fn advance(&mut self, elapsed_ns: i64) -> Result<bool, LogNookError> {
        let mut completed = false;
        let mut still_pending = Vec::with_capacity(self.pending.len());
        for event in self.pending.drain(..) {
            if elapsed_ns < event.mark.elapsed_ns.saturating_add(event.mark.post_ns) {
                still_pending.push(event);
                continue;
            }
            completed = true;
            if event.truncated {
                self.totals
                    .entry(event.mark.label)
                    .or_default()
                    .incomplete_events += 1;
                continue;
            }
            let mut correlator = CanEventCorrelator::new(std::slice::from_ref(&event.mark))?;
            let mut frames_by_id = BTreeMap::<(u32, bool), Vec<&CanFrame>>::new();
            for frame in &event.frames {
                correlator.feed(frame)?;
                frames_by_id
                    .entry((frame.can_id, frame.extended))
                    .or_default()
                    .push(frame);
            }
            if let Some(report) = correlator.finish_with_unchanged().into_iter().next() {
                let group = self.totals.entry(report.label).or_default();
                group.completed_events += 1;
                for candidate in report.candidates {
                    let key = (candidate.can_id, candidate.extended);
                    let totals = if group.candidates.contains_key(&key) {
                        group.candidates.get_mut(&key).unwrap()
                    } else if group.candidates.len() < MAX_CANDIDATES_PER_LABEL {
                        group.candidates.entry(key).or_default()
                    } else {
                        group.omitted_candidates += 1;
                        continue;
                    };
                    totals.observed_events += candidate.observed_events;
                    totals.changed_events += candidate.changed_events;
                    totals.count_change_events += candidate.count_change_events;
                    totals.before_frames += candidate.before_frames;
                    totals.after_frames += candidate.after_frames;
                    totals.before_window_ns += candidate.before_window_ns;
                    totals.after_window_ns += candidate.after_window_ns;
                    let candidate_frames = frames_by_id
                        .get(&(candidate.can_id, candidate.extended))
                        .map(Vec::as_slice)
                        .unwrap_or_default();
                    if let Some(frame) = candidate_frames.last() {
                        totals.sample_frame = Some((**frame).clone());
                    }
                    let (counter_bytes, checksum_bytes) = event_noise_hints(candidate_frames);
                    totals.counter_like_bytes.extend(counter_bytes);
                    totals.sum_checksum_like_bytes.extend(checksum_bytes);
                    for byte in candidate.changed_bytes {
                        for transition in byte.transitions {
                            *totals
                                .transitions
                                .entry((byte.byte_index, transition.before, transition.after))
                                .or_default() += transition.occurrences;
                        }
                    }
                }
            }
        }
        self.pending = still_pending;
        Ok(completed)
    }

    /// Marks outstanding windows incomplete when their capture closes.
    pub fn finish(&mut self) -> CanLiveEventSnapshot {
        for event in self.pending.drain(..) {
            self.totals
                .entry(event.mark.label)
                .or_default()
                .incomplete_events += 1;
        }
        self.active = false;
        self.snapshot()
    }

    /// Clears visible live trials while retaining receive history for future pre-windows.
    pub fn clear_results(&mut self) {
        self.pending.clear();
        self.totals.clear();
        self.total_events = 0;
    }

    pub fn snapshot(&self) -> CanLiveEventSnapshot {
        let groups = self
            .totals
            .iter()
            .map(|(label, totals)| {
                let mut candidates = totals
                    .candidates
                    .iter()
                    .filter(|(_, value)| value.changed_events > 0)
                    .map(|(&(can_id, extended), value)| {
                        let mut bytes = BTreeMap::<u8, Vec<crate::CanEventByteTransition>>::new();
                        for (&(index, before, after), &occurrences) in &value.transitions {
                            bytes
                                .entry(index)
                                .or_default()
                                .push(crate::CanEventByteTransition {
                                    before,
                                    after,
                                    occurrences,
                                });
                        }
                        CanLiveEventCandidate {
                            correlation: CanEventCandidate {
                                can_id,
                                extended,
                                observed_events: value.observed_events,
                                changed_events: value.changed_events,
                                count_change_events: value.count_change_events,
                                before_frames: value.before_frames,
                                after_frames: value.after_frames,
                                before_window_ns: value.before_window_ns,
                                after_window_ns: value.after_window_ns,
                                changed_bytes: bytes
                                    .into_iter()
                                    .map(|(byte_index, transitions)| crate::CanEventByteChange {
                                        byte_index,
                                        changed_events: transitions
                                            .iter()
                                            .map(|transition| transition.occurrences)
                                            .sum(),
                                        transitions,
                                    })
                                    .collect(),
                            },
                            sample_frame: value.sample_frame.clone(),
                            counter_like_bytes: value.counter_like_bytes.iter().copied().collect(),
                            sum_checksum_like_bytes: value
                                .sum_checksum_like_bytes
                                .iter()
                                .copied()
                                .collect(),
                        }
                    })
                    .collect::<Vec<_>>();
                candidates.sort_by(|left, right| {
                    (right.correlation.changed_events as u128
                        * left.correlation.observed_events as u128)
                        .cmp(
                            &(left.correlation.changed_events as u128
                                * right.correlation.observed_events as u128),
                        )
                        .then_with(|| {
                            right
                                .correlation
                                .changed_events
                                .cmp(&left.correlation.changed_events)
                        })
                        .then_with(|| {
                            right
                                .correlation
                                .changed_bytes
                                .len()
                                .cmp(&left.correlation.changed_bytes.len())
                        })
                        .then_with(|| {
                            right
                                .correlation
                                .count_change_events
                                .cmp(&left.correlation.count_change_events)
                        })
                });
                let omitted_candidates = totals.omitted_candidates
                    + candidates.len().saturating_sub(MAX_VISIBLE_CANDIDATES);
                candidates.truncate(MAX_VISIBLE_CANDIDATES);
                CanLiveEventGroup {
                    label: label.clone(),
                    completed_events: totals.completed_events,
                    pending_events: self
                        .pending
                        .iter()
                        .filter(|event| event.mark.label == *label)
                        .count(),
                    incomplete_events: totals.incomplete_events,
                    omitted_candidates,
                    candidates,
                }
            })
            .collect();
        CanLiveEventSnapshot {
            capture_id: self.capture_id,
            source_id: self.source_id.clone(),
            active: self.active,
            groups,
        }
    }
}

fn event_noise_hints(frames: &[&CanFrame]) -> (BTreeSet<u8>, BTreeSet<u8>) {
    let mut counters = BTreeSet::new();
    let mut checksum_bytes = BTreeSet::new();
    for byte_index in 0..8 {
        let values = frames
            .iter()
            .filter_map(|frame| frame.data.get(byte_index).copied())
            .collect::<Vec<_>>();
        if values.len() >= 4
            && values
                .windows(2)
                .all(|pair| pair[1] == pair[0].wrapping_add(1))
        {
            counters.insert(byte_index as u8);
        }
    }
    let mut by_length = BTreeMap::<usize, Vec<&CanFrame>>::new();
    for frame in frames.iter().filter(|frame| frame.data.len() >= 2) {
        by_length.entry(frame.data.len()).or_default().push(frame);
    }
    for (length, frames) in by_length {
        if frames.len() >= 4
            && frames.iter().all(|frame| {
                frame.data[..length - 1]
                    .iter()
                    .fold(0u8, |sum, byte| sum.wrapping_add(*byte))
                    == frame.data[length - 1]
            })
        {
            checksum_bytes.insert((length - 1) as u8);
        }
    }
    (counters, checksum_bytes)
}
