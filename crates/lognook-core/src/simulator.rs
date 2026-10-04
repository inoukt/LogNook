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
        (
            "fuel_system_status",
            "Fuel system status (raw code)",
            0x03,
            "code",
            0.0,
            65_535.0,
            "Raw packed fuel-system status bytes",
        ),
        (
            "engine_load",
            "Calculated engine load",
            0x04,
            "%",
            0.0,
            100.0,
            "Calculated engine load percentage",
        ),
        (
            "fuel_trim_short_b1",
            "Short term fuel trim bank 1",
            0x06,
            "%",
            -100.0,
            99.21875,
            "Short term fuel trim for bank 1",
        ),
        (
            "fuel_trim_long_b1",
            "Long term fuel trim bank 1",
            0x07,
            "%",
            -100.0,
            99.21875,
            "Long term fuel trim for bank 1",
        ),
        (
            "fuel_trim_short_b2",
            "Short term fuel trim bank 2",
            0x08,
            "%",
            -100.0,
            99.21875,
            "Short term fuel trim for bank 2",
        ),
        (
            "fuel_trim_long_b2",
            "Long term fuel trim bank 2",
            0x09,
            "%",
            -100.0,
            99.21875,
            "Long term fuel trim for bank 2",
        ),
        (
            "intake_manifold_pressure",
            "Intake manifold absolute pressure",
            0x0B,
            "kPa",
            0.0,
            255.0,
            "Intake manifold absolute pressure",
        ),
        (
            "timing_advance",
            "Timing advance",
            0x0E,
            "°BTDC",
            -64.0,
            63.5,
            "Ignition timing advance",
        ),
        (
            "o2_sensors_present_1_2",
            "O2 sensors present banks 1-2 (bitmask)",
            0x13,
            "bitmap",
            0.0,
            255.0,
            "Raw oxygen-sensor presence bitmap",
        ),
        (
            "obd_standard_supported",
            "OBD standard supported (raw code)",
            0x1C,
            "code",
            0.0,
            255.0,
            "Raw OBD standard identifier",
        ),
        (
            "engine_runtime",
            "Runtime since engine start",
            0x1F,
            "s",
            0.0,
            65_535.0,
            "Seconds since engine start",
        ),
        (
            "distance_with_mil",
            "Distance driven with MIL",
            0x21,
            "km",
            0.0,
            65_535.0,
            "Distance driven with malfunction indicator lamp on",
        ),
        (
            "fuel_rail_pressure",
            "Fuel rail gauge pressure",
            0x23,
            "kPa",
            0.0,
            655_350.0,
            "Fuel rail pressure relative to manifold vacuum",
        ),
        (
            "commanded_evap_purge",
            "Commanded evaporative purge",
            0x2E,
            "%",
            0.0,
            100.0,
            "Commanded evaporative purge percentage",
        ),
        (
            "fuel_tank_level",
            "Fuel tank level",
            0x2F,
            "%",
            0.0,
            100.0,
            "Fuel tank level input",
        ),
        (
            "warmups_since_clear",
            "Warm-ups since DTCs cleared",
            0x30,
            "count",
            0.0,
            255.0,
            "Warm-up cycles since diagnostic trouble codes were cleared",
        ),
        (
            "distance_since_clear",
            "Distance since DTCs cleared",
            0x31,
            "km",
            0.0,
            65_535.0,
            "Distance since diagnostic trouble codes were cleared",
        ),
        (
            "evap_vapor_pressure",
            "Evap vapor pressure",
            0x32,
            "Pa",
            -8_192.0,
            8_191.75,
            "Signed evaporative system vapor pressure",
        ),
        (
            "barometric_pressure",
            "Absolute barometric pressure",
            0x33,
            "kPa",
            0.0,
            255.0,
            "Absolute barometric pressure",
        ),
        (
            "catalyst_temp_b1_s1",
            "Catalyst temperature bank 1 sensor 1",
            0x3C,
            "°C",
            -40.0,
            6_513.5,
            "Catalyst temperature for bank 1 sensor 1",
        ),
        (
            "catalyst_temp_b2_s1",
            "Catalyst temperature bank 2 sensor 1",
            0x3D,
            "°C",
            -40.0,
            6_513.5,
            "Catalyst temperature for bank 2 sensor 1",
        ),
        (
            "control_module_voltage",
            "Control module voltage",
            0x42,
            "V",
            0.0,
            65.535,
            "Control module supply voltage",
        ),
        (
            "absolute_engine_load",
            "Absolute engine load",
            0x43,
            "%",
            0.0,
            25_700.0,
            "Absolute engine load percentage",
        ),
        (
            "commanded_equivalence_ratio",
            "Commanded equivalence ratio lambda",
            0x44,
            "lambda",
            0.0,
            1.999969482421875,
            "Commanded equivalence ratio",
        ),
        (
            "relative_throttle_pos",
            "Relative throttle position",
            0x45,
            "%",
            0.0,
            100.0,
            "Relative throttle position",
        ),
        (
            "ambient_temp",
            "Ambient air temperature",
            0x46,
            "°C",
            -40.0,
            215.0,
            "Ambient air temperature",
        ),
        (
            "throttle_position_b",
            "Absolute throttle position B",
            0x47,
            "%",
            0.0,
            100.0,
            "Absolute throttle position B",
        ),
        (
            "accelerator_pedal_d",
            "Accelerator pedal position D",
            0x49,
            "%",
            0.0,
            100.0,
            "Accelerator pedal position D",
        ),
        (
            "accelerator_pedal_e",
            "Accelerator pedal position E",
            0x4A,
            "%",
            0.0,
            100.0,
            "Accelerator pedal position E",
        ),
        (
            "commanded_throttle_actuator",
            "Commanded throttle actuator",
            0x4C,
            "%",
            0.0,
            100.0,
            "Commanded throttle actuator control",
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
    fn sample_interval(&self) -> Duration {
        Duration::from_millis(17)
    }

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
            let value = value_at(signal, seconds).ok_or_else(|| {
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
            // Keep simulated PID replies distinct without dominating the screen-rate target.
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(samples)
    }

    fn disconnect(&mut self) {
        self.started = None;
    }
}

fn value_at(signal: &SignalDefinition, seconds: f64) -> Option<f64> {
    let idle_wobble = 18.0 * (seconds * 0.9).sin();
    let phase = seconds % 12.0;
    let throttle_blip = if (5.0..6.2).contains(&phase) {
        ((phase - 5.0) * std::f64::consts::PI / 1.2).sin().max(0.0)
    } else {
        0.0
    };
    match signal.id.as_str() {
        "engine_rpm" => Some(780.0 + idle_wobble + throttle_blip * 1_200.0),
        "coolant_temp" => Some(22.0 + 68.0 * (1.0 - (-seconds / 70.0).exp())),
        "intake_temp" => Some(24.0 + 2.0 * (seconds / 23.0).sin()),
        "throttle_pos" => Some(2.5 + throttle_blip * 18.0 + 0.2 * (seconds * 1.7).sin()),
        "vehicle_speed" => Some(0.0),
        "fuel_system_status" => Some(0x0101 as f64),
        "o2_sensors_present_1_2" => Some(0x28 as f64),
        "obd_standard_supported" => Some(0x01 as f64),
        _ if standard_obd_signals().contains(signal) => {
            let fraction = 0.35 + 0.1 * (seconds * 0.3 + signal.pid as f64).sin();
            Some(signal.min + (signal.max - signal.min) * fraction)
        }
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

#[cfg(test)]
mod tests {
    use super::{SimulatedObdAdapter, standard_obd_signals};
    use crate::{HardwareAdapter, SignalDefinition};

    #[test]
    fn simulator_returns_samples_for_new_generic_signals() {
        let signal = SignalDefinition {
            id: "engine_load".into(),
            name: "Calculated engine load".into(),
            service: 0x01,
            pid: 0x04,
            unit: "%".into(),
            min: 0.0,
            max: 100.0,
            description: "Calculated engine load percentage".into(),
        };
        let mut adapter = SimulatedObdAdapter::default();
        adapter.connect().unwrap();

        let samples = adapter.read_samples(&[signal]).unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].signal_id, "engine_load");
        assert!((0.0..=100.0).contains(&samples[0].value));
        assert_eq!(samples[0].unit, "%");
        assert_eq!(standard_obd_signals().len(), 35);
    }
}
