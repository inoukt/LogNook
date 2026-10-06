use super::trigger::{TriggerDetector, start_automatic_recording};
use crate::{
    model::{AutomaticTriggerConfig, AutomaticTriggerStatus, EngineEvent, Sample, SampleOrigin},
    recording::RecordingWriter,
};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
};

const MAX_PRE_TRIGGER_NS: i64 = 30_000_000_000;
const MAX_BUFFERED_SAMPLES: usize = 100_000;

pub(super) fn process_live_sample(
    sample: Sample,
    latest: &AtomicI64,
    recorder: &mut Option<RecordingWriter>,
    recording_active: &AtomicBool,
    automatic_trigger_status: &Mutex<AutomaticTriggerStatus>,
    last_recording_path: &Mutex<Option<PathBuf>>,
    sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    automatic_trigger: &mut Option<(TriggerDetector, AutomaticTriggerConfig, PathBuf)>,
    pre_trigger_samples: &mut VecDeque<Sample>,
    newest_elapsed_ns: &mut i64,
) -> bool {
    if sample.origin == SampleOrigin::Live {
        latest.fetch_max(sample.elapsed_ns, Ordering::AcqRel);
        let write_error = recorder
            .as_mut()
            .and_then(|writer| writer.append(&sample).err());
        if let Some(error) = write_error {
            fail_recording(
                recorder,
                recording_active,
                automatic_trigger_status,
                last_recording_path,
                sink,
                format!("Recording stopped: {error}"),
            );
            return false;
        }

        *newest_elapsed_ns = (*newest_elapsed_ns).max(sample.elapsed_ns);
        pre_trigger_samples.push_back(sample.clone());
        while pre_trigger_samples.len() > MAX_BUFFERED_SAMPLES
            || pre_trigger_samples.front().is_some_and(|buffered| {
                buffered.elapsed_ns < newest_elapsed_ns.saturating_sub(MAX_PRE_TRIGGER_NS)
            })
        {
            pre_trigger_samples.pop_front();
        }

        let fired = automatic_trigger
            .as_mut()
            .is_some_and(|(detector, _, _)| detector.observe(&sample));
        if fired {
            let (_, config, path) = automatic_trigger.take().unwrap();
            match start_automatic_recording(&path, &config, &sample, pre_trigger_samples) {
                Ok((writer, marker)) => {
                    *recorder = Some(writer);
                    recording_active.store(true, Ordering::Release);
                    set_trigger_status(
                        automatic_trigger_status,
                        AutomaticTriggerStatus {
                            armed: false,
                            recording: true,
                            config: Some(config),
                            recording_path: Some(path.to_string_lossy().into_owned()),
                        },
                        sink,
                    );
                    sink(EngineEvent::Marker(marker));
                }
                Err(error) => {
                    set_trigger_status(
                        automatic_trigger_status,
                        AutomaticTriggerStatus::default(),
                        sink,
                    );
                    sink(EngineEvent::Error(format!(
                        "Automatic trigger recording could not start: {error}"
                    )));
                }
            }
        }
    }
    sink(EngineEvent::Sample(sample));
    true
}

pub(super) fn get_trigger_status(status: &Mutex<AutomaticTriggerStatus>) -> AutomaticTriggerStatus {
    status
        .lock()
        .map(|status| status.clone())
        .unwrap_or_default()
}

pub(super) fn set_trigger_status(
    current: &Mutex<AutomaticTriggerStatus>,
    status: AutomaticTriggerStatus,
    sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
) {
    if let Ok(mut current) = current.lock() {
        *current = status.clone();
    }
    sink(EngineEvent::TriggerStatus(status));
}

pub(super) fn set_last_recording_path(last: &Mutex<Option<PathBuf>>, path: &Path) {
    if let Ok(mut last) = last.lock() {
        *last = Some(path.to_path_buf());
    }
}

pub(super) fn fail_recording(
    recorder: &mut Option<RecordingWriter>,
    active: &AtomicBool,
    automatic_trigger_status: &Mutex<AutomaticTriggerStatus>,
    last_path: &Mutex<Option<PathBuf>>,
    sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    message: String,
) {
    let was_recording = recorder.is_some();
    if let Some(writer) = recorder.take() {
        match writer.finish() {
            Ok(summary) => {
                set_last_recording_path(last_path, &summary.path);
                sink(EngineEvent::RecordingFailed(summary));
            }
            Err(error) => sink(EngineEvent::Error(format!(
                "Could not finalize recording: {error}"
            ))),
        }
    }
    active.store(false, Ordering::Release);
    if was_recording && get_trigger_status(automatic_trigger_status).recording {
        set_trigger_status(
            automatic_trigger_status,
            AutomaticTriggerStatus::default(),
            sink,
        );
    }
    sink(EngineEvent::Error(message));
}
