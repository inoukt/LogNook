use crate::{
    derived::CompiledDerivedSignal,
    model::{Sample, SampleQuality},
};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct DerivedProcessor {
    signals: Vec<CompiledDerivedSignal>,
    latest: HashMap<String, Sample>,
}

impl DerivedProcessor {
    pub(super) fn new(signals: Vec<CompiledDerivedSignal>) -> Self {
        Self { signals, latest: HashMap::new() }
    }

    pub(super) fn observe(&mut self, input: &Sample) -> (Vec<Sample>, Vec<String>) {
        if input.quality != SampleQuality::Valid {
            self.invalidate(&input.signal_id);
            return (Vec::new(), Vec::new());
        }
        self.latest.insert(input.signal_id.clone(), input.clone());
        let values = self.latest
            .iter()
            .map(|(id, sample)| (id.clone(), sample.value))
            .collect::<HashMap<_, _>>();
        let mut samples = Vec::new();
        let mut errors = Vec::new();
        for signal in &self.signals {
            if !signal.dependencies().contains(&input.signal_id)
                || !signal.dependencies().iter().all(|id| self.latest.contains_key(id))
            {
                continue;
            }
            let raw_value = match signal.formula.evaluate(&values) {
                Ok(value) => value,
                Err(error) => {
                    errors.push(format!("Derived signal '{}': {error}", signal.definition.id));
                    continue;
                }
            };
            let value = raw_value - signal.definition.tare_offset;
            if !value.is_finite() {
                errors.push(format!("Derived signal '{}' result must be finite", signal.definition.id));
                continue;
            }
            let sample = Sample {
                elapsed_ns: input.elapsed_ns,
                wall_time_unix_ns: input.wall_time_unix_ns,
                source_id: input.source_id.clone(),
                ecu_id: input.ecu_id.clone(),
                signal_id: signal.definition.id.clone(),
                value,
                unit: signal.definition.unit.clone(),
                quality: SampleQuality::Valid,
                origin: input.origin.clone(),
                raw_bytes: None,
            };
            self.latest.insert(sample.signal_id.clone(), sample.clone());
            samples.push(sample);
        }
        (samples, errors)
    }

    pub(super) fn invalidate(&mut self, id: &str) {
        self.latest.remove(id);
    }

    pub(super) fn tare(&mut self, id: &str) -> Result<(Sample, f64), String> {
        let index = self.signals
            .iter()
            .position(|signal| signal.definition.id == id)
            .ok_or_else(|| format!("Derived signal '{id}' is not active"))?;
        let signal = &self.signals[index];
        if !signal.dependencies().iter().all(|dependency| self.latest.contains_key(dependency)) {
            return Err(format!("Derived signal '{id}' has no current input samples to tare"));
        }
        let values = self.latest
            .iter()
            .map(|(signal_id, sample)| (signal_id.clone(), sample.value))
            .collect::<HashMap<_, _>>();
        let offset = signal.formula.evaluate(&values).map_err(|error| error.to_string())?;
        let input = signal.dependencies()
            .iter()
            .filter_map(|dependency| self.latest.get(dependency))
            .max_by_key(|sample| sample.elapsed_ns)
            .ok_or_else(|| format!("Derived signal '{id}' has no current input samples to tare"))?;
        let sample = Sample {
            elapsed_ns: input.elapsed_ns,
            wall_time_unix_ns: input.wall_time_unix_ns,
            source_id: input.source_id.clone(),
            ecu_id: input.ecu_id.clone(),
            signal_id: id.to_owned(),
            value: 0.0,
            unit: signal.definition.unit.clone(),
            quality: SampleQuality::Valid,
            origin: input.origin.clone(),
            raw_bytes: None,
        };
        self.signals[index].definition.tare_offset = offset;
        self.latest.insert(id.to_owned(), sample.clone());
        Ok((sample, offset))
    }
}
