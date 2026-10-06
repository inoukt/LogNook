use lognook_core::*;

fn frame(t: i64, id: u32, data: &[u8]) -> CanFrame {
    CanFrame {
        elapsed_ns: t,
        wall_time_unix_ns: 1000 + t,
        can_id: id,
        extended: false,
        direction: CanDirection::Rx,
        data: data.to_vec(),
        flags: 0,
    }
}
fn analyzer() -> CanAnalyzer {
    CanAnalyzer::new(CanAnalysisConfig {
        max_ids: 2,
        history_capacity: 4,
        max_obd_samples: 4,
        max_timing_rows: 4,
        correlation_tolerance_ns: 0,
        diagnostic_routes: vec![CanDiagnosticRoute {
            request_id: 0x700,
            response_id: 0x708,
            extended: false,
        }],
        request_timeout_ns: 100,
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn census_keeps_rates_periods_and_tentative_byte_statistics() {
    let mut a = analyzer();
    for i in 0..4 {
        a.feed(&frame(i * 10, 0x123, &[7, i as u8])).unwrap();
    }
    let rows = a.census();
    let r = &rows[0];
    assert_eq!(r.count, 4);
    assert_eq!(r.mean_period_ns, Some(10.0));
    assert_eq!(r.rate_hz, Some(100_000_000.0));
    assert_eq!(r.dlc_counts[2], 4);
    assert!(r.bytes[0].is_static);
    assert!(r.bytes[1].counter_hypothesis);
    assert_eq!(r.bytes[1].changes, 3);
    assert_eq!(r.bytes[1].min, Some(0));
    assert_eq!(r.bytes[1].max, Some(3));
}

#[test]
fn memory_is_bounded_and_invalid_frames_do_not_enter_census() {
    let mut a = analyzer();
    for i in 0..20 {
        a.feed(&frame(i, 0x123, &[i as u8])).unwrap();
    }
    assert_eq!(a.history(0x123, false).len(), 4);
    a.feed(&frame(21, 0x124, &[1])).unwrap();
    assert!(a.feed(&frame(22, 0x125, &[1])).is_err());
    assert!(a.feed(&frame(22, 0x123, &[1; 9])).is_err());
    assert_eq!(a.census().len(), 2);
}

#[test]
fn explicit_routes_pair_positive_pending_and_negative_replies() {
    let mut a = analyzer();
    a.feed(&frame(0, 0x700, &[3, 0x22, 0x12, 0x34])).unwrap();
    a.feed(&frame(10, 0x708, &[3, 0x7f, 0x22, 0x78])).unwrap();
    a.feed(&frame(20, 0x708, &[4, 0x62, 0x12, 0x34, 9]))
        .unwrap();
    assert_eq!(
        a.timings()[0].outcome,
        CanDiagnosticOutcome::ResponsePending { nrc: 0x78 }
    );
    assert_eq!(a.timings()[1].outcome, CanDiagnosticOutcome::Positive);
    assert_eq!(a.timings()[1].latency_ns, 20);
    a.feed(&frame(30, 0x700, &[2, 0x21, 0x05])).unwrap();
    a.feed(&frame(40, 0x708, &[3, 0x7f, 0x21, 0x33])).unwrap();
    assert_eq!(
        a.timings()[2].outcome,
        CanDiagnosticOutcome::Negative { nrc: 0x33 }
    );
    assert_eq!(a.timings()[2].request_payload, vec![0x21, 5]);
}

#[test]
fn wrong_echo_and_timeout_remain_distinct() {
    let mut a = analyzer();
    a.feed(&frame(0, 0x700, &[2, 0x21, 5])).unwrap();
    a.feed(&frame(10, 0x708, &[3, 0x61, 6, 9])).unwrap();
    assert_eq!(a.timings()[0].outcome, CanDiagnosticOutcome::WrongEcho);
    a.feed(&frame(20, 0x700, &[2, 0x21, 5])).unwrap();
    a.advance_time(121);
    assert_eq!(a.timings()[1].outcome, CanDiagnosticOutcome::NoResponse);
}

#[test]
fn correlation_uses_explicit_field_and_actual_sample_timestamps() {
    let mut a = analyzer();
    let d = CanSignalDefinition {
        id: "field".into(),
        name: "field".into(),
        can_id: 0x123,
        extended: false,
        start_bit: 0,
        bit_length: 8,
        byte_order: CanByteOrder::Intel,
        signed: false,
        factor: 1.0,
        offset: 0.0,
        unit: "".into(),
    };
    for i in 0..4 {
        a.feed(&frame(i * 10, 0x123, &[i as u8])).unwrap();
        a.feed_obd_sample(&Sample {
            elapsed_ns: i * 10,
            wall_time_unix_ns: 1000 + i * 10,
            source_id: "obd".into(),
            ecu_id: Some("ECU".into()),
            signal_id: "rpm".into(),
            value: i as f64 * 100.0,
            unit: "rpm".into(),
            quality: SampleQuality::Valid,
            origin: SampleOrigin::Live,
            raw_bytes: None,
        });
    }
    let c = a.correlate(&d, "obd", Some("ECU"), "rpm").unwrap().unwrap();
    assert_eq!(c.pairs, 4);
    assert!((c.pearson_r - 1.0).abs() < 1e-12);
    assert!(c.hypothesis);
}

#[test]
fn trigger_includes_exact_window_and_reports_capacity_truncation() {
    let mut r = CanTriggerRecorder::new(CanTriggerConfig {
        pre_ns: 10,
        post_ns: 10,
        max_frames: 10,
    })
    .unwrap();
    for t in [0, 5, 10, 15, 20] {
        r.feed(&frame(t, 1, &[1])).unwrap();
    }
    r.trigger(20).unwrap();
    for t in [25, 30, 35] {
        r.feed(&frame(t, 1, &[1])).unwrap();
    }
    let c = r.take_capture().unwrap();
    assert_eq!(
        c.frames.iter().map(|f| f.elapsed_ns).collect::<Vec<_>>(),
        vec![10, 15, 20, 25, 30]
    );
    assert!(!c.truncated);
    let mut r = CanTriggerRecorder::new(CanTriggerConfig {
        pre_ns: 100,
        post_ns: 0,
        max_frames: 2,
    })
    .unwrap();
    for t in [0, 1, 2] {
        r.feed(&frame(t, 1, &[1])).unwrap();
    }
    r.trigger(2).unwrap();
    assert!(r.take_capture().unwrap().truncated);
}

#[test]
fn segmented_reply_keeps_payload_and_rejects_wrong_sequence() {
    let mut a = analyzer();
    a.feed(&frame(0, 0x700, &[3, 0x22, 0x12, 0x34])).unwrap();
    a.feed(&frame(10, 0x708, &[0x10, 9, 0x62, 0x12, 0x34, 1, 2, 3]))
        .unwrap();
    assert!(a.timings().is_empty());
    a.feed(&frame(20, 0x708, &[0x21, 4, 5, 6, 0, 0, 0, 0]))
        .unwrap();
    assert_eq!(
        a.timings()[0].response_payload,
        vec![0x62, 0x12, 0x34, 1, 2, 3, 4, 5, 6]
    );
    a.feed(&frame(30, 0x700, &[2, 0x21, 5])).unwrap();
    a.feed(&frame(40, 0x708, &[0x10, 9, 0x61, 5, 1, 2, 3, 4]))
        .unwrap();
    a.feed(&frame(50, 0x708, &[0x22, 5, 6, 7])).unwrap();
    assert_eq!(a.timings()[1].outcome, CanDiagnosticOutcome::Malformed);
}

#[test]
fn sum_checksum_stays_hypothesis_and_out_of_order_frames_are_rejected() {
    let mut a = analyzer();
    for i in 0..4 {
        a.feed(&frame(i, 1, &[i as u8, 10, i as u8 + 10])).unwrap();
    }
    assert!(a.census()[0].bytes[2].checksum_hypothesis);
    assert!(a.feed(&frame(1, 1, &[0])).is_err());
    assert_eq!(a.census()[0].count, 4);
}

#[test]
fn trigger_completes_at_post_boundary_without_needing_an_extra_frame() {
    let mut r = CanTriggerRecorder::new(CanTriggerConfig {
        pre_ns: 0,
        post_ns: 10,
        max_frames: 2,
    })
    .unwrap();
    r.feed(&frame(0, 1, &[0])).unwrap();
    r.trigger(0).unwrap();
    r.feed(&frame(10, 1, &[1])).unwrap();
    let capture = r.take_capture().unwrap();
    assert!(capture.complete);
    assert_eq!(capture.frames.len(), 2);
}

#[test]
fn segmented_request_latency_starts_at_first_observed_frame() {
    let mut a = analyzer();
    a.feed(&frame(0, 0x700, &[0x10, 8, 0x22, 0x12, 0x34, 1, 2, 3]))
        .unwrap();
    a.feed(&frame(10, 0x700, &[0x21, 4, 5])).unwrap();
    a.feed(&frame(20, 0x708, &[4, 0x62, 0x12, 0x34, 9]))
        .unwrap();
    assert_eq!(a.timings()[0].request_elapsed_ns, 0);
    assert_eq!(a.timings()[0].latency_ns, 20);
}

#[test]
fn new_request_cannot_complete_a_previous_partial_reply() {
    let mut a = analyzer();
    a.feed(&frame(0, 0x700, &[2, 0x21, 5])).unwrap();
    a.feed(&frame(10, 0x708, &[0x10, 9, 0x61, 5, 1, 2, 3, 4]))
        .unwrap();
    a.feed(&frame(20, 0x700, &[2, 0x21, 5])).unwrap();
    a.feed(&frame(30, 0x708, &[0x21, 5, 6, 7])).unwrap();
    assert_eq!(a.timings()[0].outcome, CanDiagnosticOutcome::Superseded);
    assert_eq!(a.timings()[1].outcome, CanDiagnosticOutcome::Malformed);
}

#[test]
fn diagnostic_rows_are_bounded_even_when_pending_repeats() {
    let mut a = analyzer();
    a.feed(&frame(0, 0x700, &[2, 0x21, 5])).unwrap();
    for t in 1..10 {
        a.feed(&frame(t, 0x708, &[3, 0x7f, 0x21, 0x78])).unwrap();
    }
    assert_eq!(a.timings().len(), 4);
    assert_eq!(a.timings()[0].response_elapsed_ns, 6);
}

#[test]
fn every_frame_at_the_trigger_post_boundary_is_retained() {
    let mut r = CanTriggerRecorder::new(CanTriggerConfig {
        pre_ns: 0,
        post_ns: 10,
        max_frames: 3,
    })
    .unwrap();
    r.feed(&frame(0, 1, &[0])).unwrap();
    r.trigger(0).unwrap();
    r.feed(&frame(10, 1, &[1])).unwrap();
    r.feed(&frame(10, 2, &[2])).unwrap();
    let capture = r.take_capture().unwrap();
    assert!(capture.complete);
    assert_eq!(
        capture.frames.iter().map(|f| f.can_id).collect::<Vec<_>>(),
        vec![1, 1, 2]
    );
    assert!(!capture.truncated);
}

#[test]
fn short_isotp_first_frame_cannot_become_a_positive_reply() {
    let mut a = analyzer();
    a.feed(&frame(0, 0x700, &[3, 0x22, 0x12, 0x34])).unwrap();
    a.feed(&frame(10, 0x708, &[0x10, 8, 0x62])).unwrap();
    a.feed(&frame(20, 0x708, &[0x21, 0x12, 0x34, 1, 2, 3, 4, 5]))
        .unwrap();
    assert_eq!(a.timings()[0].outcome, CanDiagnosticOutcome::Malformed);
}

#[test]
fn short_nonfinal_isotp_consecutive_frame_is_malformed() {
    let mut a = analyzer();
    a.feed(&frame(0, 0x700, &[3, 0x22, 0x12, 0x34])).unwrap();
    a.feed(&frame(10, 0x708, &[0x10, 15, 0x62, 0x12, 0x34, 1, 2, 3]))
        .unwrap();
    a.feed(&frame(20, 0x708, &[0x21, 4, 5])).unwrap();
    a.feed(&frame(30, 0x708, &[0x22, 6, 7, 8, 9, 10, 11, 12]))
        .unwrap();
    assert_eq!(a.timings()[0].outcome, CanDiagnosticOutcome::Malformed);
}

#[test]
fn timed_out_partial_reply_retains_bytes_and_is_not_silence() {
    let mut a = analyzer();
    a.feed(&frame(0, 0x700, &[3, 0x22, 0x12, 0x34])).unwrap();
    a.feed(&frame(10, 0x708, &[0x10, 9, 0x62, 0x12, 0x34, 1, 2, 3]))
        .unwrap();
    a.advance_time(101);
    assert_eq!(a.timings()[0].outcome, CanDiagnosticOutcome::Malformed);
    assert_eq!(
        a.timings()[0].response_payload,
        vec![0x62, 0x12, 0x34, 1, 2, 3]
    );
}
