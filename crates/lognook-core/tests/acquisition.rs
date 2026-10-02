use lognook_core::{HardwareAdapter, SimulatedObdAdapter};
use std::{thread, time::Duration};

#[test]
fn simulator_emits_monotonic_samples_for_selected_obd_signals() {
    let mut adapter = SimulatedObdAdapter::default();
    let capabilities = adapter.connect().unwrap();
    let selected = capabilities
        .supported_signals
        .into_iter()
        .filter(|signal| signal.id == "engine_rpm" || signal.id == "coolant_temp")
        .collect::<Vec<_>>();

    let first = adapter.read_samples(&selected).unwrap();
    thread::sleep(Duration::from_millis(20));
    let second = adapter.read_samples(&selected).unwrap();

    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 2);
    for before in &first {
        let after = second
            .iter()
            .find(|sample| sample.signal_id == before.signal_id)
            .unwrap();
        assert!(after.elapsed_ns > before.elapsed_ns);
        assert_eq!(after.source_id, "simulated-obd");
    }
    let rpm = second
        .iter()
        .find(|sample| sample.signal_id == "engine_rpm")
        .unwrap();
    let coolant = second
        .iter()
        .find(|sample| sample.signal_id == "coolant_temp")
        .unwrap();
    assert!((650.0..=2_500.0).contains(&rpm.value));
    assert!((15.0..=95.0).contains(&coolant.value));
}
