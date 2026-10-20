use lognook_core::{
    CanDirection, CanFrame, CanLiveEventWindowState, CanManualEventMark, LiveCanEventAnalyzer,
    correlate_can_events,
};

fn frame(elapsed_ns: i64, value: u8) -> CanFrame {
    identified_frame(elapsed_ns, 0x120, false, value)
}

fn identified_frame(elapsed_ns: i64, can_id: u32, extended: bool, value: u8) -> CanFrame {
    CanFrame {
        elapsed_ns,
        wall_time_unix_ns: elapsed_ns,
        can_id,
        extended,
        direction: CanDirection::Rx,
        data: vec![value],
        flags: 0,
    }
}

fn mark(elapsed_ns: i64) -> CanManualEventMark {
    CanManualEventMark {
        label: "traction toggle".into(),
        elapsed_ns,
        pre_ns: 100,
        post_ns: 100,
    }
}

#[test]
fn live_correlation_counts_unchanged_trials_in_the_recurrence_denominator() {
    let mut analyzer = LiveCanEventAnalyzer::new(1, "test-can");
    analyzer.feed(&frame(0, 0)).unwrap();
    analyzer.mark(mark(100)).unwrap();
    assert_eq!(analyzer.snapshot().groups[0].pending_events, 1);
    analyzer.feed(&frame(150, 1)).unwrap();
    assert!(analyzer.advance(200).unwrap());

    analyzer.feed(&frame(250, 1)).unwrap();
    analyzer.mark(mark(300)).unwrap();
    analyzer.feed(&frame(350, 1)).unwrap();
    assert!(analyzer.advance(400).unwrap());

    let group = analyzer.snapshot().groups.remove(0);
    assert_eq!(group.completed_events, 2);
    let candidate = &group.candidates[0].correlation;
    assert_eq!(candidate.observed_events, 2);
    assert_eq!(candidate.changed_events, 1);
    assert_eq!(candidate.before_frames, 2);
    assert_eq!(candidate.after_frames, 2);
    assert_eq!(candidate.before_window_ns, 200);
    assert_eq!(candidate.after_window_ns, 200);
    assert_eq!(candidate.changed_bytes[0].transitions[0].before, Some(0));
    assert_eq!(candidate.changed_bytes[0].transitions[0].after, Some(1));
}

#[test]
fn overlapping_live_windows_keep_standard_and_extended_ids_separate_and_match_replay() {
    let marks = [
        CanManualEventMark {
            label: "traction toggle".into(),
            elapsed_ns: 100,
            pre_ns: 100,
            post_ns: 100,
        },
        CanManualEventMark {
            label: "traction toggle".into(),
            elapsed_ns: 130,
            pre_ns: 50,
            post_ns: 100,
        },
    ];
    let frames = [
        identified_frame(0, 0x123, false, 0),
        identified_frame(0, 0x123, true, 9),
        identified_frame(120, 0x123, false, 1),
        identified_frame(120, 0x123, true, 9),
        identified_frame(150, 0x123, false, 0),
        identified_frame(150, 0x123, true, 8),
    ];
    let mut analyzer = LiveCanEventAnalyzer::new(1, "test-can");
    analyzer.feed(&frames[0]).unwrap();
    analyzer.feed(&frames[1]).unwrap();
    analyzer.mark(marks[0].clone()).unwrap();
    analyzer.feed(&frames[2]).unwrap();
    analyzer.feed(&frames[3]).unwrap();
    analyzer.mark(marks[1].clone()).unwrap();
    analyzer.feed(&frames[4]).unwrap();
    analyzer.feed(&frames[5]).unwrap();
    analyzer.advance(230).unwrap();

    let live = analyzer.snapshot();
    let group = &live.groups[0];
    assert_eq!(group.completed_events, 2);
    let standard = group
        .candidates
        .iter()
        .find(|candidate| !candidate.correlation.extended)
        .unwrap();
    let extended = group
        .candidates
        .iter()
        .find(|candidate| candidate.correlation.extended)
        .unwrap();
    assert_eq!(standard.correlation.observed_events, 2);
    assert_eq!(standard.correlation.changed_events, 2);
    assert_eq!(standard.correlation.before_window_ns, 150);
    assert_eq!(extended.correlation.observed_events, 2);
    assert_eq!(extended.correlation.changed_events, 1);

    let replay = correlate_can_events(&marks, frames.iter());
    let replay_candidates = &replay[0].candidates;
    assert_eq!(
        standard.correlation.changed_events,
        replay_candidates
            .iter()
            .find(|candidate| !candidate.extended)
            .unwrap()
            .changed_events,
    );
    assert_eq!(
        extended.correlation.observed_events,
        replay_candidates
            .iter()
            .find(|candidate| candidate.extended)
            .unwrap()
            .observed_events,
    );
}

#[test]
fn live_candidates_keep_an_exact_rx_frame_for_the_visual_editor() {
    let mut analyzer = LiveCanEventAnalyzer::new(1, "test-can");
    analyzer.feed(&frame(0, 0)).unwrap();
    analyzer.mark(mark(100)).unwrap();
    analyzer.feed(&frame(150, 1)).unwrap();
    analyzer.advance(200).unwrap();

    let candidate = &analyzer.snapshot().groups[0].candidates[0];
    assert_eq!(candidate.sample_frame.as_ref().unwrap().elapsed_ns, 150);
    assert_eq!(candidate.sample_frame.as_ref().unwrap().data, vec![1]);
}

#[test]
fn live_candidate_marks_incrementing_and_sum_checksum_bytes_as_noise_hints() {
    let mut analyzer = LiveCanEventAnalyzer::new(1, "test-can");
    for (elapsed_ns, sequence) in [(100, 0), (200, 1), (300, 2), (400, 3)] {
        let mut frame = frame(elapsed_ns, sequence);
        frame.data = vec![sequence, 0x55, sequence.wrapping_add(0x55)];
        analyzer.feed(&frame).unwrap();
    }
    analyzer
        .mark(CanManualEventMark {
            label: "toggle".into(),
            elapsed_ns: 1_000,
            pre_ns: 1_000,
            post_ns: 1_000,
        })
        .unwrap();
    let mut after = frame(1_100, 4);
    after.data = vec![4, 0x55, 4u8.wrapping_add(0x55)];
    analyzer.feed(&after).unwrap();
    analyzer.advance(2_000).unwrap();

    let candidate = &analyzer.snapshot().groups[0].candidates[0];
    assert!(candidate.counter_like_bytes.contains(&0));
    assert!(candidate.sum_checksum_like_bytes.contains(&2));
}

#[test]
fn clearing_results_keeps_the_pre_event_receive_history() {
    let mut analyzer = LiveCanEventAnalyzer::new(1, "test-can");
    analyzer.feed(&frame(0, 0)).unwrap();
    analyzer.mark(mark(100)).unwrap();
    analyzer.clear_results();
    assert!(analyzer.snapshot().active);
    assert!(analyzer.snapshot().groups.is_empty());

    analyzer.mark(mark(100)).unwrap();
    analyzer.feed(&frame(150, 1)).unwrap();
    analyzer.advance(200).unwrap();
    let candidate = &analyzer.snapshot().groups[0].candidates[0].correlation;
    assert_eq!(candidate.before_frames, 1);
}

#[test]
fn live_window_finishes_on_time_without_a_later_frame_and_close_marks_pending_incomplete() {
    let mut analyzer = LiveCanEventAnalyzer::new(1, "test-can");
    analyzer.feed(&frame(0, 0)).unwrap();
    analyzer.mark(mark(100)).unwrap();
    assert!(analyzer.advance(200).unwrap());
    let completed = analyzer.snapshot();
    assert_eq!(completed.groups[0].completed_events, 1);
    assert_eq!(
        completed.groups[0].candidates[0].correlation.after_frames,
        0
    );

    analyzer.mark(mark(300)).unwrap();
    let stopped = analyzer.finish();
    assert!(!stopped.active);
    assert_eq!(stopped.groups[0].pending_events, 0);
    assert_eq!(stopped.groups[0].incomplete_events, 1);
    assert_eq!(stopped.groups[0].completed_events, 1);
}

#[test]
fn live_mark_publishes_pending_then_completed_window_with_latest_rx_summaries() {
    let mut analyzer = LiveCanEventAnalyzer::new(1, "test-can");
    analyzer.mark(mark(100)).unwrap();
    let pending = &analyzer.snapshot().groups[0].windows[0];
    assert_eq!(pending.sequence, 1);
    assert_eq!(pending.elapsed_ns, 100);
    assert_eq!(pending.pre_ns, 100);
    assert_eq!(pending.post_ns, 100);
    assert_eq!(pending.state, CanLiveEventWindowState::Pending);

    analyzer
        .feed(&identified_frame(110, 0x123, false, 1))
        .unwrap();
    analyzer
        .feed(&identified_frame(120, 0x123, false, 2))
        .unwrap();
    analyzer
        .feed(&identified_frame(130, 0x123, true, 3))
        .unwrap();
    analyzer
        .feed(&identified_frame(140, 0x100, false, 4))
        .unwrap();
    analyzer.advance(200).unwrap();

    let completed = &analyzer.snapshot().groups[0].windows[0];
    assert_eq!(completed.state, CanLiveEventWindowState::Complete);
    assert_eq!(completed.omitted_ids, 0);
    assert_eq!(completed.messages.len(), 3);
    let standard = completed
        .messages
        .iter()
        .find(|message| message.can_id == 0x123 && !message.extended)
        .unwrap();
    assert_eq!(standard.frame_count, 2);
    assert_eq!(standard.latest_frame.elapsed_ns, 120);
    assert_eq!(standard.latest_frame.data, vec![2]);
    assert!(
        completed
            .messages
            .iter()
            .any(|message| message.can_id == 0x123 && message.extended)
    );
}

#[test]
fn completed_window_with_no_frames_is_empty() {
    let mut analyzer = LiveCanEventAnalyzer::new(1, "test-can");
    analyzer.mark(mark(100)).unwrap();
    analyzer.advance(200).unwrap();
    let window = &analyzer.snapshot().groups[0].windows[0];
    assert_eq!(window.state, CanLiveEventWindowState::Complete);
    assert!(window.messages.is_empty());
    assert_eq!(window.omitted_ids, 0);
    assert_eq!(
        serde_json::to_string(&window.state).unwrap(),
        "\"complete\""
    );
}

#[test]
fn completed_window_caps_ids_and_orders_by_count_then_width_and_id() {
    let mut analyzer = LiveCanEventAnalyzer::new(1, "test-can");
    analyzer.mark(mark(100)).unwrap();
    analyzer.feed(&identified_frame(101, 10, false, 1)).unwrap();
    analyzer.feed(&identified_frame(102, 10, false, 2)).unwrap();
    for can_id in 0..=101 {
        analyzer
            .feed(&identified_frame(103, can_id, false, 0))
            .unwrap();
    }
    analyzer.advance(200).unwrap();

    let window = &analyzer.snapshot().groups[0].windows[0];
    assert_eq!(window.messages.len(), 100);
    assert_eq!(window.omitted_ids, 2);
    assert_eq!(window.messages[0].can_id, 10);
    assert_eq!(window.messages[0].frame_count, 3);
    assert_eq!(window.messages[1].can_id, 0);
    assert_eq!(window.messages[2].can_id, 1);
}

#[test]
fn completed_window_orders_standard_ids_before_extended_ids() {
    let mut analyzer = LiveCanEventAnalyzer::new(1, "test-can");
    analyzer.mark(mark(100)).unwrap();
    analyzer.feed(&identified_frame(101, 0, true, 0)).unwrap();
    analyzer
        .feed(&identified_frame(102, 100, false, 0))
        .unwrap();
    analyzer.feed(&identified_frame(103, 0, false, 0)).unwrap();
    analyzer.advance(200).unwrap();

    let messages = &analyzer.snapshot().groups[0].windows[0].messages;
    assert_eq!((messages[0].can_id, messages[0].extended), (0, false));
    assert_eq!((messages[1].can_id, messages[1].extended), (100, false));
    assert_eq!((messages[2].can_id, messages[2].extended), (0, true));
}

#[test]
fn truncated_and_stopped_windows_are_incomplete() {
    let mut analyzer = LiveCanEventAnalyzer::new(1, "test-can");
    analyzer
        .mark(CanManualEventMark {
            label: "truncated".into(),
            elapsed_ns: 0,
            pre_ns: 0,
            post_ns: 20_000,
        })
        .unwrap();
    for elapsed_ns in 1..=10_001 {
        analyzer.feed(&frame(elapsed_ns, elapsed_ns as u8)).unwrap();
    }
    analyzer.advance(20_000).unwrap();
    assert_eq!(
        analyzer.snapshot().groups[0].windows[0].state,
        CanLiveEventWindowState::Incomplete
    );

    analyzer
        .mark(CanManualEventMark {
            label: "stopped".into(),
            elapsed_ns: 20_001,
            pre_ns: 0,
            post_ns: 100,
        })
        .unwrap();
    let stopped = analyzer.finish();
    assert_eq!(
        stopped.groups[1].windows[0].state,
        CanLiveEventWindowState::Incomplete
    );
}

#[test]
fn mark_with_lost_receive_history_is_incomplete() {
    let mut analyzer = LiveCanEventAnalyzer::new(1, "test-can");
    for _ in 0..=50_000 {
        analyzer.feed(&frame(50_001, 0)).unwrap();
    }
    analyzer
        .mark(CanManualEventMark {
            label: "lost history".into(),
            elapsed_ns: 50_001,
            pre_ns: 0,
            post_ns: 1,
        })
        .unwrap();
    analyzer.feed(&frame(50_002, 0)).unwrap();
    analyzer.advance(50_002).unwrap();
    let window = &analyzer.snapshot().groups[0].windows[0];
    assert_eq!(window.state, CanLiveEventWindowState::Incomplete);
    assert_eq!(window.messages[0].frame_count, 1);
}

#[test]
fn sequence_continues_after_clearing_results() {
    let mut analyzer = LiveCanEventAnalyzer::new(1, "test-can");
    analyzer.mark(mark(100)).unwrap();
    analyzer.clear_results();
    analyzer.mark(mark(100)).unwrap();
    let window = &analyzer.snapshot().groups[0].windows[0];
    assert_eq!(window.sequence, 2);
    assert_eq!(window.state, CanLiveEventWindowState::Pending);
}
