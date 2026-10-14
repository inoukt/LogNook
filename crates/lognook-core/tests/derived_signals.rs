use lognook_core::{
    ConnectionCapabilities, DerivedSignalDefinition, DeviceDescriptor, EngineEvent,
    HardwareAdapter, LogNookEngine, LogNookError, SignalDefinition, export_recording_csv,
    import_csv, read_recording, standard_obd_signals,
};
use std::{
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn temp_recording() -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("lognook-derived-{id}.lognook"))
}

fn definition(id: &str, expression: &str, tare_offset: f64) -> DerivedSignalDefinition {
    DerivedSignalDefinition {
        id: id.into(),
        name: id.into(),
        expression: expression.into(),
        unit: "derived-unit".into(),
        tare_offset,
    }
}

#[test]
fn engine_acquires_formula_inputs_and_records_derived_rows_without_changing_sources() {
    let (events_tx, events_rx) = mpsc::channel();
    let mut engine = LogNookEngine::new(move |event| {
        let _ = events_tx.send(event);
    });
    let path = temp_recording();
    engine
        .configure_derived_signals(
            &[definition("temp_f", "coolant_temp * 1.8 + 32", 0.0)],
            &["temp_f".into()],
        )
        .unwrap();
    engine.connect_simulated(&["temp_f".into()]).unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut source = None;
    let mut derived = None;
    while std::time::Instant::now() < deadline && derived.is_none() {
        if let Ok(EngineEvent::Sample(sample)) = events_rx.recv_timeout(Duration::from_millis(100))
        {
            if sample.signal_id == "coolant_temp" {
                source = Some(sample.clone());
            }
            if sample.signal_id == "temp_f" {
                derived = Some(sample);
            }
        }
    }
    let source = source.expect("formula input should be acquired");
    let derived = derived.expect("derived Sample event should be emitted");
    assert_eq!(derived.value, source.value * 1.8 + 32.0);
    assert_eq!(derived.elapsed_ns, source.elapsed_ns);
    assert_eq!(derived.origin, source.origin);

    engine.start_recording(&path).unwrap();
    let tare_offset = engine.tare_derived_signal("temp_f").unwrap();
    assert!(tare_offset.is_finite());
    let tared = events_rx
        .try_iter()
        .filter_map(|event| match event {
            EngineEvent::Sample(sample) if sample.signal_id == "temp_f" => Some(sample),
            _ => None,
        })
        .find(|sample| sample.value == 0.0)
        .expect("tare should emit a zero-valued derived sample");
    assert_eq!(tared.value, 0.0);

    thread::sleep(Duration::from_millis(250));
    engine.stop_recording().unwrap();
    engine.disconnect().unwrap();
    let recorded = read_recording(&path).unwrap();
    assert!(
        recorded
            .samples
            .iter()
            .any(|sample| sample.signal_id == "coolant_temp")
    );
    assert!(
        recorded
            .samples
            .iter()
            .any(|sample| sample.signal_id == "temp_f")
    );
    assert!(
        recorded
            .samples
            .iter()
            .any(|sample| sample.signal_id == "temp_f" && sample.value == 0.0)
    );
    let csv_path = path.with_extension("csv");
    export_recording_csv(&path, &csv_path).unwrap();
    let replayed = import_csv(std::fs::File::open(&csv_path).unwrap()).unwrap();
    assert!(replayed.iter().any(|sample| sample.signal_id == "temp_f"));
    std::fs::remove_file(csv_path).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn definitions_reject_unknown_inputs_and_non_finite_tare_offsets() {
    let mut engine = LogNookEngine::new(|_| {});
    assert!(
        engine
            .configure_derived_signals(
                &[definition("bad", "missing_signal + 1", 0.0)],
                &["bad".into()],
            )
            .unwrap_err()
            .to_string()
            .contains("missing_signal")
    );
    assert!(
        engine
            .configure_derived_signals(
                &[definition("bad", "coolant_temp", f64::INFINITY)],
                &["bad".into()],
            )
            .is_err()
    );
}

struct RpmOnlyAdapter;

impl HardwareAdapter for RpmOnlyAdapter {
    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        Ok(ConnectionCapabilities {
            device: DeviceDescriptor {
                stable_id: "rpm-only".into(),
                hardware_family: "test adapter".into(),
                firmware: None,
                path: None,
            },
            protocol: "test".into(),
            read_only: true,
            supported_signals: standard_obd_signals()
                .into_iter()
                .filter(|signal| signal.id == "engine_rpm")
                .collect(),
        })
    }

    fn read_samples(
        &mut self,
        _: &[SignalDefinition],
    ) -> Result<Vec<lognook_core::Sample>, LogNookError> {
        Ok(Vec::new())
    }
}

#[test]
fn connection_rejects_a_derived_formula_when_an_input_is_not_acquired() {
    let mut engine = LogNookEngine::new(|_| {});
    engine
        .configure_derived_signals(
            &[definition("temp_f", "coolant_temp * 1.8 + 32", 0.0)],
            &["temp_f".into()],
        )
        .unwrap();
    let error = engine
        .connect_adapter(Box::new(RpmOnlyAdapter), &["temp_f".into()])
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unsupported input 'coolant_temp'")
    );
    assert!(!engine.is_connected());
}

#[test]
fn runtime_division_by_zero_emits_an_error_and_no_derived_sample() {
    let (events_tx, events_rx) = mpsc::channel();
    let mut engine = LogNookEngine::new(move |event| {
        let _ = events_tx.send(event);
    });
    engine
        .configure_derived_signals(
            &[definition(
                "bad_ratio",
                "coolant_temp / (coolant_temp - coolant_temp)",
                0.0,
            )],
            &["bad_ratio".into()],
        )
        .unwrap();
    engine.connect_simulated(&["bad_ratio".into()]).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut error_seen = false;
    let mut derived_seen = false;
    while std::time::Instant::now() < deadline && !error_seen {
        match events_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(EngineEvent::Error(message)) if message.contains("divide by zero") => {
                error_seen = true
            }
            Ok(EngineEvent::Sample(sample)) if sample.signal_id == "bad_ratio" => {
                derived_seen = true
            }
            _ => {}
        }
    }
    engine.disconnect().unwrap();
    assert!(error_seen, "runtime formula error should be visible");
    assert!(
        !derived_seen,
        "a failed formula must not emit a measurement"
    );
}
