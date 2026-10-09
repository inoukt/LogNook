use crate::{
    model::{AutomaticTriggerConfig, Marker, Sample, SampleOrigin, SampleQuality, TriggerDirection},
    recording::RecordingWriter,
};
use std::{collections::VecDeque, path::Path};

pub(super) fn start_automatic_recording(
    path: &Path,
    config: &AutomaticTriggerConfig,
    trigger_sample: &Sample,
    pre_trigger_samples: &VecDeque<Sample>,
) -> Result<(RecordingWriter, Marker), String> {
    let mut writer = RecordingWriter::create(path).map_err(|error| error.to_string())?;
    let cutoff_ns = trigger_sample
        .elapsed_ns
        .saturating_sub((config.pre_trigger_ms as i64).saturating_mul(1_000_000));
    if let Err(error) = writer.append_batch(pre_trigger_samples.iter().filter(|sample| {
        sample.elapsed_ns >= cutoff_ns && sample.elapsed_ns <= trigger_sample.elapsed_ns
    })) {
        let _ = writer.finish();
        return Err(error.to_string());
    }
    let marker = Marker {
        id: None,
        elapsed_ns: trigger_sample.elapsed_ns,
        wall_time_unix_ns: trigger_sample.wall_time_unix_ns,
        label: "Automatic trigger".into(),
        note: format!(
            "{} {} {} {}",
            config.signal_id,
            config.direction.as_str(),
            config.threshold,
            trigger_sample.unit
        ),
    };
    let marker = match writer.add_marker(&marker) {
        Ok(id) => Marker {
            id: Some(id),
            ..marker
        },
        Err(error) => {
            let _ = writer.finish();
            return Err(error.to_string());
        }
    };
    Ok((writer, marker))
}

pub(super) struct TriggerDetector {
    config: AutomaticTriggerConfig,
    condition_active: bool,
    active_since_ns: Option<i64>,
    last_elapsed_ns: Option<i64>,
}

impl TriggerDetector {
    pub(super) fn new(config: AutomaticTriggerConfig) -> Self {
        Self {
            config,
            condition_active: false,
            active_since_ns: None,
            last_elapsed_ns: None,
        }
    }

    pub(super) fn observe(&mut self, sample: &Sample) -> bool {
        if sample.signal_id != self.config.signal_id {
            return false;
        }
        if sample.origin != SampleOrigin::Live
            || sample.quality != SampleQuality::Valid
            || !sample.value.is_finite()
        {
            self.reset();
            return false;
        }
        if self.last_elapsed_ns.is_some_and(|previous| sample.elapsed_ns < previous) {
            self.reset();
        }
        self.last_elapsed_ns = Some(sample.elapsed_ns);

        let meets_threshold = match self.config.direction {
            TriggerDirection::Above => sample.value >= self.config.threshold,
            TriggerDirection::Below => sample.value <= self.config.threshold,
        };
        let clears_hysteresis = match self.config.direction {
            TriggerDirection::Above => sample.value < self.config.threshold - self.config.hysteresis,
            TriggerDirection::Below => sample.value > self.config.threshold + self.config.hysteresis,
        };
        if self.condition_active && clears_hysteresis {
            self.condition_active = false;
            self.active_since_ns = None;
        } else if !self.condition_active && meets_threshold {
            self.condition_active = true;
            self.active_since_ns = Some(sample.elapsed_ns);
        }

        self.active_since_ns.is_some_and(|since| {
            sample.elapsed_ns.saturating_sub(since)
                >= (self.config.hold_ms as i64).saturating_mul(1_000_000)
        })
    }

    fn reset(&mut self) {
        self.condition_active = false;
        self.active_since_ns = None;
        self.last_elapsed_ns = None;
    }
}

#[cfg(test)]
mod tests {
    use super::TriggerDetector;
    use crate::model::{AutomaticTriggerConfig, Sample, SampleOrigin, SampleQuality, TriggerDirection};

    fn sample(elapsed_ns: i64, value: f64) -> Sample {
        Sample {
            elapsed_ns,
            wall_time_unix_ns: 1_800_000_000_000_000_000 + elapsed_ns,
            source_id: "test-adapter".into(),
            ecu_id: Some("engine".into()),
            signal_id: "engine_rpm".into(),
            value,
            unit: "rpm".into(),
            quality: SampleQuality::Valid,
            origin: SampleOrigin::Live,
            raw_bytes: None,
        }
    }

    fn config() -> AutomaticTriggerConfig {
        AutomaticTriggerConfig {
            signal_id: "engine_rpm".into(),
            direction: TriggerDirection::Above,
            threshold: 100.0,
            hysteresis: 10.0,
            hold_ms: 100,
            pre_trigger_ms: 0,
        }
    }

    #[test]
    fn threshold_hold_uses_sample_time_and_hysteresis_keeps_candidate_alive() {
        let mut detector = TriggerDetector::new(config());
        assert!(!detector.observe(&sample(1_000_000_000, 100.0)));
        assert!(!detector.observe(&sample(1_099_000_000, 101.0)));
        assert!(detector.observe(&sample(1_100_000_000, 95.0)));
    }

    #[test]
    fn leaving_hysteresis_band_resets_the_hold_candidate() {
        let mut detector = TriggerDetector::new(config());
        assert!(!detector.observe(&sample(0, 101.0)));
        assert!(!detector.observe(&sample(50_000_000, 89.0)));
        assert!(!detector.observe(&sample(100_000_000, 101.0)));
        assert!(!detector.observe(&sample(199_000_000, 102.0)));
        assert!(detector.observe(&sample(200_000_000, 95.0)));
    }
}
