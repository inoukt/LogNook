use lognook_core::{
    ConnectionCapabilities, DeviceDescriptor, DiagnosticOutcome, DiagnosticReport,
    DiagnosticRequest, EngineEvent, HardwareAdapter, LogNookEngine, LogNookError,
    ResearchTestRequest, ResearchTestResult, Sample, SignalDefinition, standard_obd_signals,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

struct ProfileAdapter {
    active: Arc<AtomicBool>,
    overlap: Arc<AtomicBool>,
    samples: Arc<AtomicUsize>,
    identities: Arc<AtomicUsize>,
    query_delay: Duration,
}

impl HardwareAdapter for ProfileAdapter {
    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        Ok(ConnectionCapabilities {
            device: DeviceDescriptor {
                stable_id: "fixture-adapter".into(),
                hardware_family: "OBDLink SX".into(),
                firmware: None,
                path: Some("COM99".into()),
            },
            protocol: "ISO 15765-4 CAN (11 bit, 500 kbit/s)".into(),
            read_only: true,
            supported_signals: standard_obd_signals()
                .into_iter()
                .filter(|signal| matches!(signal.id.as_str(), "engine_rpm" | "coolant_temp"))
                .collect(),
        })
    }

    fn read_samples(&mut self, signals: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
        if self.active.swap(true, Ordering::AcqRel) {
            self.overlap.store(true, Ordering::Release);
        }
        if !signals.is_empty() {
            self.samples.fetch_add(1, Ordering::AcqRel);
        }
        thread::sleep(Duration::from_millis(2));
        self.active.store(false, Ordering::Release);
        Ok(Vec::new())
    }

    fn execute_research_test(
        &mut self,
        request: ResearchTestRequest,
    ) -> Result<ResearchTestResult, LogNookError> {
        let ResearchTestRequest::Diagnostic(query) = request else {
            return Err(LogNookError::new("Unexpected request"));
        };
        if self.active.swap(true, Ordering::AcqRel) {
            self.overlap.store(true, Ordering::Release);
        }
        self.identities.fetch_add(1, Ordering::AcqRel);
        thread::sleep(self.query_delay);
        let pid = query.request_payload()[1];
        let identity = match pid {
            0x02 => b"1FAHP2E89BG123456".to_vec(),
            0x04 => b"CALIBRATION-0001".to_vec(),
            _ => b"PCM".to_vec(),
        };
        let mut payload = vec![0x49, pid, 1];
        payload.extend_from_slice(&identity);
        let read = query.report(
            Some("7E8".into()),
            payload,
            "fixture response".into(),
            "fixture query".into(),
            10,
            2,
            false,
        );
        self.active.store(false, Ordering::Release);
        Ok(ResearchTestResult::Diagnostic(DiagnosticReport {
            report_id: None,
            connection_generation: None,
            vehicle_context: None,
            adapter: self.connect()?.device,
            protocol: "ISO 15765-4 CAN (11 bit, 500 kbit/s)".into(),
            started_at_unix_ms: 10,
            request: DiagnosticRequest::default(),
            reads: vec![read],
            route_restore_error: None,
            evidence_saved: false,
            evidence_save_error: None,
        }))
    }
}

fn adapter_state() -> (
    ProfileAdapter,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
    Arc<AtomicBool>,
) {
    adapter_state_with_delay(Duration::from_millis(2))
}

fn adapter_state_with_delay(
    query_delay: Duration,
) -> (
    ProfileAdapter,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
    Arc<AtomicBool>,
) {
    let active = Arc::new(AtomicBool::new(false));
    let overlap = Arc::new(AtomicBool::new(false));
    let samples = Arc::new(AtomicUsize::new(0));
    let identities = Arc::new(AtomicUsize::new(0));
    (
        ProfileAdapter {
            active,
            overlap: overlap.clone(),
            samples: samples.clone(),
            identities: identities.clone(),
            query_delay,
        },
        samples,
        identities,
        overlap,
    )
}

#[test]
fn identity_queries_are_only_vin_calibration_and_ecu_name_reads() {
    let payloads = DiagnosticRequest::identity_queries()
        .into_iter()
        .map(|query| query.request_payload().to_vec())
        .collect::<Vec<_>>();
    assert_eq!(
        payloads,
        [vec![0x09, 0x02], vec![0x09, 0x04], vec![0x09, 0x0A]]
    );
}

#[test]
fn profile_connection_stays_in_worker_owned_standby_until_acknowledged_selection_replacement() {
    let (adapter, samples, identities, overlap) = adapter_state();
    let (events_tx, events_rx) = std::sync::mpsc::channel();
    let mut engine = LogNookEngine::new(move |event| {
        let _ = events_tx.send(event);
    });
    let caps = engine.connect_for_profile(Box::new(adapter)).unwrap();
    let generation = engine.obd_connection_generation();
    assert!(engine.is_obd_profile_standby());
    assert_eq!(engine.connection_capabilities().unwrap(), &caps);
    thread::sleep(Duration::from_millis(60));
    assert_eq!(
        samples.load(Ordering::Acquire),
        0,
        "standby must not poll, including empty-signal calls"
    );

    let reads = engine
        .queue_profile_identity_reads(generation)
        .unwrap()
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(reads.len(), 3);
    assert!(
        reads
            .iter()
            .all(|read| read.outcome == DiagnosticOutcome::Positive)
    );
    assert_eq!(identities.load(Ordering::Acquire), 3);
    assert_eq!(samples.load(Ordering::Acquire), 0);
    assert!(
        !overlap.load(Ordering::Acquire),
        "identity queries and polling share one serialized adapter owner"
    );

    engine
        .replace_profile_signals(vec!["engine_rpm".into()], generation)
        .unwrap();
    assert!(!engine.is_obd_profile_standby());
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while samples.load(Ordering::Acquire) == 0 && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(samples.load(Ordering::Acquire) > 0);
    assert!(!overlap.load(Ordering::Acquire));
    assert!(matches!(
        events_rx
            .try_iter()
            .find(|event| matches!(event, EngineEvent::Connected(_))),
        Some(EngineEvent::Connected(_))
    ));
    engine.disconnect().unwrap();
}

#[test]
fn stale_profile_generation_and_unsupported_selection_cannot_replace_the_standby_state() {
    let (adapter, samples, _, _) = adapter_state();
    let mut engine = LogNookEngine::new(|_| {});
    engine.connect_for_profile(Box::new(adapter)).unwrap();
    let generation = engine.obd_connection_generation();
    assert!(
        engine
            .replace_profile_signals(vec!["engine_rpm".into()], generation + 1)
            .is_err()
    );
    assert!(
        engine
            .replace_profile_signals(vec!["mass_air_flow".into()], generation)
            .is_err()
    );
    assert!(engine.is_obd_profile_standby());
    assert!(engine.selected_signal_ids().is_empty());
    thread::sleep(Duration::from_millis(50));
    assert_eq!(samples.load(Ordering::Acquire), 0);
    engine.disconnect().unwrap();
}

#[test]
fn ordinary_connection_recognizes_through_same_owner_and_returns_to_normal_polling_between_reads() {
    let (adapter, samples, identities, overlap) =
        adapter_state_with_delay(Duration::from_millis(120));
    let mut engine = LogNookEngine::new(|_| {});
    engine
        .connect_adapter(Box::new(adapter), &["engine_rpm".into()])
        .unwrap();
    let generation = engine.obd_connection_generation();
    thread::sleep(Duration::from_millis(15));
    let before = samples.load(Ordering::Acquire);
    let cancellation = Arc::new(AtomicBool::new(false));
    for query in DiagnosticRequest::identity_queries() {
        let reads = engine
            .queue_profile_identity_query(generation, query, cancellation.clone())
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].outcome, DiagnosticOutcome::Positive);
    }
    assert_eq!(identities.load(Ordering::Acquire), 3);
    assert!(
        samples.load(Ordering::Acquire) > before,
        "the regular poll should resume between recognition queries"
    );
    assert!(!engine.is_obd_profile_standby());
    assert_eq!(engine.selected_signal_ids(), ["engine_rpm"]);
    assert!(!overlap.load(Ordering::Acquire));
    engine.disconnect().unwrap();
}
