use crate::{
    HardwareAdapter,
    model::{
        ConnectionCapabilities, DeviceDescriptor, LogNookError, Sample, SampleOrigin,
        SampleQuality, SignalDefinition,
    },
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub struct SimulatedObdAdapter {
    started: Option<Instant>,
}

pub fn standard_obd_signals() -> Vec<SignalDefinition> {
    [
        (
            "engine_rpm",
            "Engine speed",
            0x0C,
            "rpm",
            0.0,
            8_000.0,
            "Engine crankshaft speed",
        ),
        (
            "coolant_temp",
            "Coolant temperature",
            0x05,
            "°C",
            -40.0,
            215.0,
            "Engine coolant temperature",
        ),
        (
            "intake_temp",
            "Intake air temperature",
            0x0F,
            "°C",
            -40.0,
            215.0,
            "Intake manifold air temperature",
        ),
        (
            "throttle_pos",
            "Throttle position",
            0x11,
            "%",
            0.0,
            100.0,
            "Absolute throttle position",
        ),
        (
            "vehicle_speed",
            "Vehicle speed",
            0x0D,
            "km/h",
            0.0,
            255.0,
            "Road speed reported by the ECU",
        ),
    ]
    .into_iter()
    .map(
        |(id, name, pid, unit, min, max, description)| SignalDefinition {
            id: id.into(),
            name: name.into(),
            service: 0x01,
            pid,
            unit: unit.into(),
            min,
            max,
            description: description.into(),
        },
    )
    .collect()
}

impl HardwareAdapter for SimulatedObdAdapter {
    fn connect(&mut self) -> Result<ConnectionCapabilities, LogNookError> {
        self.started = Some(Instant::now());
        Ok(ConnectionCapabilities {
            device: DeviceDescriptor {
                stable_id: "simulated-obd".into(),
                hardware_family: "simulator".into(),
                firmware: Some("LogNook scripted OBD v1".into()),
                path: None,
            },
            protocol: "SAE J1979 Mode 01 (simulated)".into(),
            read_only: true,
            supported_signals: standard_obd_signals(),
        })
    }

    fn read_samples(&mut self, signals: &[SignalDefinition]) -> Result<Vec<Sample>, LogNookError> {
        let started = self
            .started
            .ok_or_else(|| LogNookError::new("Adapter is not connected"))?;
        let mut samples = Vec::with_capacity(signals.len());
        for signal in signals {
            let elapsed = started.elapsed();
            let seconds = elapsed.as_secs_f64();
            let value = value_at(&signal.id, seconds).ok_or_else(|| {
                LogNookError::new(format!("Unsupported simulated signal: {}", signal.id))
            })?;
            samples.push(Sample {
                elapsed_ns: elapsed.as_nanos().min(i64::MAX as u128) as i64,
                wall_time_unix_ns: now_unix_ns(),
                source_id: "simulated-obd".into(),
                ecu_id: Some("engine".into()),
                signal_id: signal.id.clone(),
                value,
                unit: signal.unit.clone(),
                quality: SampleQuality::Valid,
                origin: SampleOrigin::Live,
                raw_bytes: raw_response(&signal.id, value),
            });
            // Simulate a modest serial ECU response delay; distinct samples keep their receive times.
            std::thread::sleep(Duration::from_millis(8));
        }
        Ok(samples)
    }

    fn disconnect(&mut self) {
        self.started = None;
    }
}

fn value_at(signal_id: &str, seconds: f64) -> Option<f64> {
    let idle_wobble = 18.0 * (seconds * 0.9).sin();
    let phase = seconds % 12.0;
    let throttle_blip = if (5.0..6.2).contains(&phase) {
        ((phase - 5.0) * std::f64::consts::PI / 1.2).sin().max(0.0)
    } else {
        0.0
    };
    match signal_id {
        "engine_rpm" => Some(780.0 + idle_wobble + throttle_blip * 1_200.0),
        "coolant_temp" => Some(22.0 + 68.0 * (1.0 - (-seconds / 70.0).exp())),
        "intake_temp" => Some(24.0 + 2.0 * (seconds / 23.0).sin()),
        "throttle_pos" => Some(2.5 + throttle_blip * 18.0 + 0.2 * (seconds * 1.7).sin()),
        "vehicle_speed" => Some(0.0),
        _ => None,
    }
}

fn raw_response(signal_id: &str, value: f64) -> Option<Vec<u8>> {
    match signal_id {
        "engine_rpm" => Some(((value * 4.0).round() as u16).to_be_bytes().to_vec()),
        "coolant_temp" | "intake_temp" => Some([(value + 40.0).round() as u8].to_vec()),
        "throttle_pos" => Some([(value * 2.55).round() as u8].to_vec()),
        "vehicle_speed" => Some([value.round() as u8].to_vec()),
        _ => None,
    }
}

fn now_unix_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(i64::MAX as u128) as i64
}
