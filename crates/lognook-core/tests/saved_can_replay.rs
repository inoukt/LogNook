use lognook_core::{
    CanCaptureWriter, CanDirection, CanFrame, CanReplayPlan, CanReplaySource,
    SavedCanReplaySelection, prepare_saved_can_replay,
};
use std::{
    fs::{self, OpenOptions},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_FILE: AtomicU64 = AtomicU64::new(1);

struct TempFile(PathBuf);

impl TempFile {
    fn new() -> Self {
        let id = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!("lognook-saved-replay-{nanos}-{id}.candump.csv")))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn frame(
    elapsed_ns: i64,
    can_id: u32,
    extended: bool,
    direction: CanDirection,
    flags: u32,
) -> CanFrame {
    CanFrame {
        elapsed_ns,
        wall_time_unix_ns: 1_700_000_000_000_000_000,
        can_id,
        extended,
        direction,
        data: vec![elapsed_ns as u8],
        flags,
    }
}

fn write_capture(path: &std::path::Path, frames: &[CanFrame]) {
    let _ = fs::remove_file(path);
    let mut writer = CanCaptureWriter::create_new(path).unwrap();
    for frame in frames {
        writer.write_frame(frame).unwrap();
    }
    writer.finish().unwrap();
}

fn selection(start_elapsed_ns: i64, end_elapsed_ns: i64) -> SavedCanReplaySelection {
    SavedCanReplaySelection {
        can_id: 0x123,
        extended: false,
        start_elapsed_ns,
        end_elapsed_ns,
    }
}

fn prepare(
    path: &std::path::Path,
    selection: SavedCanReplaySelection,
) -> Result<CanReplayPlan, String> {
    prepare_saved_can_replay(path, selection, &AtomicBool::new(false))
        .map(|prepared| prepared.plan)
        .map_err(|error| error.to_string())
}

#[test]
fn selects_exact_format_inclusive_range_and_excludes_tx_and_flagged_rows() {
    let path = TempFile::new();
    write_capture(
        path.path(),
        &[
            frame(0, 0x123, false, CanDirection::Rx, 0),
            frame(500_000, 0x123, true, CanDirection::Rx, 0),
            frame(1_000_000, 0x123, false, CanDirection::Tx, 0),
            frame(1_500_000, 0x123, false, CanDirection::Rx, 1),
            frame(2_000_000, 0x123, false, CanDirection::Rx, 0),
            frame(3_000_000, 0x123, false, CanDirection::Rx, 0),
        ],
    );

    let plan = prepare(path.path(), selection(0, 2_000_000)).unwrap();

    assert!(matches!(
        plan.source(),
        CanReplaySource::SavedCapture { .. }
    ));
    assert_eq!(
        plan.frames()
            .iter()
            .map(|item| item.source_row.unwrap())
            .collect::<Vec<_>>(),
        [2, 6]
    );
    assert_eq!(plan.frames()[0].frame.elapsed_ns, 0);
    assert_eq!(plan.frames()[1].frame.elapsed_ns, 2_000_000);
    let summary = plan.saved_summary().unwrap();
    assert_eq!(summary.matching_rx_count, 2);
    assert_eq!(summary.excluded_tx_count, 1);
    assert_eq!(summary.excluded_flagged_count, 1);
    assert_eq!((summary.first_row, summary.last_row), (2, 6));
    assert_eq!(summary.source_span_ns, 2_000_000);
    assert_eq!(summary.paced_duration_ns, 2_000_000);
}

#[test]
fn duplicate_timestamps_keep_row_order_and_use_one_millisecond_floor() {
    let path = TempFile::new();
    write_capture(
        path.path(),
        &[
            frame(8_000_000, 0x123, false, CanDirection::Rx, 0),
            frame(8_000_000, 0x123, false, CanDirection::Rx, 0),
            frame(8_000_000, 0x123, false, CanDirection::Rx, 0),
        ],
    );

    let plan = prepare(path.path(), selection(8_000_000, 8_000_000)).unwrap();

    assert_eq!(
        plan.frames()
            .iter()
            .map(|item| item.source_row.unwrap())
            .collect::<Vec<_>>(),
        [2, 3, 4]
    );
    assert_eq!(plan.saved_summary().unwrap().source_span_ns, 0);
    assert_eq!(plan.saved_summary().unwrap().paced_duration_ns, 2_000_000);
}

#[test]
fn prepared_plan_and_digest_do_not_change_when_source_file_is_replaced() {
    let path = TempFile::new();
    write_capture(path.path(), &[frame(0, 0x123, false, CanDirection::Rx, 0)]);
    let prepared =
        prepare_saved_can_replay(path.path(), selection(0, 0), &AtomicBool::new(false)).unwrap();
    let original_digest = prepared.summary.source_sha256.clone();

    write_capture(
        path.path(),
        &[CanFrame {
            data: vec![0xFE],
            ..frame(0, 0x123, false, CanDirection::Rx, 0)
        }],
    );

    assert_eq!(prepared.plan.frames()[0].frame.data, [0]);
    assert_eq!(prepared.summary.source_sha256, original_digest);
    let changed =
        prepare_saved_can_replay(path.path(), selection(0, 0), &AtomicBool::new(false)).unwrap();
    assert_ne!(changed.summary.source_sha256, original_digest);
    assert_eq!(changed.plan.frames()[0].frame.data, [0xFE]);
}

#[test]
fn malformed_row_outside_selection_rejects_the_entire_capture() {
    let path = TempFile::new();
    fs::write(
        path.path(),
        "wall_time_unix_ns,elapsed_ns,direction,can_id,format,dlc,data_hex,flags\n\
         1700000000000000000,0,rx,123,standard,1,AA,00000000\n\
         malformed,row,outside,the,selection\n",
    )
    .unwrap();

    let error = prepare(path.path(), selection(0, 0)).unwrap_err();

    assert!(error.contains("row 3"), "{error}");
}

#[test]
fn decreasing_matching_timestamps_reject_instead_of_sorting() {
    let path = TempFile::new();
    write_capture(
        path.path(),
        &[
            frame(2_000_000, 0x123, false, CanDirection::Rx, 0),
            frame(1_000_000, 0x123, false, CanDirection::Rx, 0),
        ],
    );

    let error = prepare(path.path(), selection(0, 3_000_000)).unwrap_err();

    assert!(error.contains("timestamp order"), "{error}");
}

#[test]
fn empty_selection_and_invalid_ranges_are_rejected() {
    let path = TempFile::new();
    write_capture(path.path(), &[frame(0, 0x124, false, CanDirection::Rx, 0)]);
    assert!(
        prepare(path.path(), selection(0, 0))
            .unwrap_err()
            .contains("no matching")
    );

    let invalid = SavedCanReplaySelection {
        start_elapsed_ns: 2,
        end_elapsed_ns: 1,
        ..selection(0, 1)
    };
    assert!(
        prepare(path.path(), invalid)
            .unwrap_err()
            .contains("start/end")
    );

    let overflow_id = SavedCanReplaySelection {
        can_id: u32::MAX,
        ..selection(0, 1)
    };
    assert!(
        prepare(path.path(), overflow_id)
            .unwrap_err()
            .contains("identifier")
    );

    let near_limit = TempFile::new();
    write_capture(
        near_limit.path(),
        &[
            frame(0, 0x123, false, CanDirection::Rx, 0),
            frame(i64::MAX, 0x123, false, CanDirection::Rx, 0),
        ],
    );
    assert!(
        prepare(near_limit.path(), selection(0, i64::MAX))
            .unwrap_err()
            .contains("60000 ms")
    );
}

#[test]
fn exact_10000_frame_limit_is_accepted_and_10001_is_rejected() {
    let accepted = TempFile::new();
    let frames = (0..10_000)
        .map(|i| frame(i * 1_000_000, 0x123, false, CanDirection::Rx, 0))
        .collect::<Vec<_>>();
    write_capture(accepted.path(), &frames);
    let plan = prepare(accepted.path(), selection(0, 10_000_000_000)).unwrap();
    assert_eq!(plan.frames().len(), 10_000);

    let rejected = TempFile::new();
    let frames = (0..10_001)
        .map(|i| frame(i * 1_000_000, 0x123, false, CanDirection::Rx, 0))
        .collect::<Vec<_>>();
    write_capture(rejected.path(), &frames);
    assert!(
        prepare(rejected.path(), selection(0, 11_000_000_000))
            .unwrap_err()
            .contains("1-10000")
    );
}

#[test]
fn source_and_paced_duration_and_per_frame_gaps_are_bounded() {
    let long_source = TempFile::new();
    let frames = (0..=13)
        .map(|i| frame(i * 50_000_000_000, 0x123, false, CanDirection::Rx, 0))
        .collect::<Vec<_>>();
    write_capture(long_source.path(), &frames);
    assert!(
        prepare(long_source.path(), selection(0, 700_000_000_000))
            .unwrap_err()
            .contains("600 seconds")
    );

    let over_gap = TempFile::new();
    write_capture(
        over_gap.path(),
        &[
            frame(0, 0x123, false, CanDirection::Rx, 0),
            frame(60_000_000_001, 0x123, false, CanDirection::Rx, 0),
        ],
    );
    assert!(
        prepare(over_gap.path(), selection(0, 61_000_000_000))
            .unwrap_err()
            .contains("60000 ms")
    );

    let effective_overflow = TempFile::new();
    let mut frames = (0..9_990)
        .map(|_| frame(0, 0x123, false, CanDirection::Rx, 0))
        .collect::<Vec<_>>();
    for i in 1..=10 {
        frames.push(frame(i * 60_000_000_000, 0x123, false, CanDirection::Rx, 0));
    }
    write_capture(effective_overflow.path(), &frames);
    assert!(
        prepare(effective_overflow.path(), selection(0, 600_000_000_000))
            .unwrap_err()
            .contains("600 seconds")
    );
}

#[test]
fn live_snapshot_plan_retains_the_existing_400_frame_cap() {
    let frames = (0..400)
        .map(|i| frame(i * 1_000_000, 0x123, false, CanDirection::Rx, 0))
        .collect::<Vec<_>>();
    let plan = CanReplayPlan::live_snapshot(frames.clone()).unwrap();
    assert!(matches!(plan.source(), CanReplaySource::LiveSnapshot));
    assert_eq!(plan.frames().len(), 400);

    let too_many = (0..401)
        .map(|i| frame(i * 1_000_000, 0x123, false, CanDirection::Rx, 0))
        .collect::<Vec<_>>();
    assert!(
        CanReplayPlan::live_snapshot(too_many)
            .unwrap_err()
            .to_string()
            .contains("1-400")
    );
}

#[test]
fn oversized_file_and_cancelled_preparation_are_rejected() {
    let oversized = TempFile::new();
    fs::write(
        oversized.path(),
        "wall_time_unix_ns,elapsed_ns,direction,can_id,format,dlc,data_hex,flags\n",
    )
    .unwrap();
    OpenOptions::new()
        .write(true)
        .open(oversized.path())
        .unwrap()
        .set_len(256 * 1024 * 1024 + 1)
        .unwrap();
    assert!(
        prepare(oversized.path(), selection(0, 0))
            .unwrap_err()
            .contains("256 MiB")
    );

    let cancelled = TempFile::new();
    write_capture(
        cancelled.path(),
        &[frame(0, 0x123, false, CanDirection::Rx, 0)],
    );
    let token = Arc::new(AtomicBool::new(true));
    let error = prepare_saved_can_replay(cancelled.path(), selection(0, 0), &token).unwrap_err();
    assert!(error.to_string().contains("cancelled"));
}
