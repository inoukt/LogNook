mod derived;
mod live_samples;
mod trigger;

use self::{
    derived::DerivedProcessor,
    live_samples::{
        fail_recording, get_trigger_status, process_live_sample, set_last_recording_path,
        set_trigger_status,
    },
    trigger::TriggerDetector,
};
use crate::{
    model::{AutomaticTriggerConfig, AutomaticTriggerStatus, EngineEvent, Marker},
    pipeline::Message,
    recording::RecordingWriter,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
        mpsc::Receiver,
    },
};

pub(crate) fn process(
    rx: Receiver<Message>,
    latest: Arc<AtomicI64>,
    recording_active: Arc<AtomicBool>,
    automatic_trigger_status: Arc<Mutex<AutomaticTriggerStatus>>,
    last_recording_path: Arc<Mutex<Option<PathBuf>>>,
    sink: Arc<dyn Fn(EngineEvent) + Send + Sync>,
) {
    let mut recorder: Option<RecordingWriter> = None;
    let mut automatic_trigger: Option<(TriggerDetector, AutomaticTriggerConfig, PathBuf)> = None;
    let mut derived = DerivedProcessor::default();
    let mut pre_trigger_samples = VecDeque::new();
    let mut newest_elapsed_ns = 0;
    'messages: while let Ok(message) = rx.recv() {
        match message {
            Message::Sample(sample) => {
                if !sample.value.is_finite() {
                    derived.invalidate(&sample.signal_id);
                    if let Some((detector, _, _)) = automatic_trigger.as_mut() {
                        detector.observe(&sample);
                    }
                    fail_recording(
                        &mut recorder,
                        &recording_active,
                        &automatic_trigger_status,
                        &last_recording_path,
                        &sink,
                        "Dropped non-finite measurement".into(),
                    );
                    continue;
                }
                let (derived_samples, errors) = derived.observe(&sample);
                for error in errors {
                    sink(EngineEvent::Error(error));
                }
                for current in std::iter::once(sample).chain(derived_samples) {
                    if !process_live_sample(
                        current,
                        &latest,
                        &mut recorder,
                        &recording_active,
                        &automatic_trigger_status,
                        &last_recording_path,
                        &sink,
                        &mut automatic_trigger,
                        &mut pre_trigger_samples,
                        &mut newest_elapsed_ns,
                    ) {
                        continue 'messages;
                    }
                }
            }
            Message::PlaybackSample(session_id, sample) => {
                sink(EngineEvent::PlaybackSample { session_id, sample });
            }
            Message::SetDerivedSignals(signals, reply) => {
                derived = DerivedProcessor::new(signals);
                let _ = reply.send(Ok(()));
            }
            Message::TareDerivedSignal(id, reply) => {
                let result = derived.tare(&id);
                match result {
                    Ok((sample, offset)) => {
                        let processed = process_live_sample(
                            sample,
                            &latest,
                            &mut recorder,
                            &recording_active,
                            &automatic_trigger_status,
                            &last_recording_path,
                            &sink,
                            &mut automatic_trigger,
                            &mut pre_trigger_samples,
                            &mut newest_elapsed_ns,
                        );
                        let _ = reply.send(if processed {
                            Ok(offset)
                        } else {
                            Err(
                                "Tare was applied, but the recording stopped after a write error"
                                    .into(),
                            )
                        });
                    }
                    Err(error) => {
                        let _ = reply.send(Err(error));
                    }
                }
            }
            Message::StartRecording(path, reply) => {
                let result = if recorder.is_some() {
                    Err("A recording is already active".to_owned())
                } else {
                    RecordingWriter::create(&path)
                        .map(|writer| {
                            recorder = Some(writer);
                            recording_active.store(true, Ordering::Release);
                        })
                        .map_err(|error| error.to_string())
                };
                if result.is_ok() && automatic_trigger.take().is_some() {
                    set_trigger_status(
                        &automatic_trigger_status,
                        AutomaticTriggerStatus::default(),
                        &sink,
                    );
                }
                let _ = reply.send(result);
            }
            Message::ArmAutomaticTrigger(config, path, reply) => {
                let result = if recorder.is_some() {
                    Err("Stop the active recording before arming an automatic trigger".to_owned())
                } else if automatic_trigger.is_some() {
                    Err("An automatic trigger is already armed".to_owned())
                } else {
                    let status = AutomaticTriggerStatus {
                        armed: true,
                        recording: false,
                        config: Some(config.clone()),
                        recording_path: None,
                    };
                    automatic_trigger = Some((TriggerDetector::new(config.clone()), config, path));
                    set_trigger_status(&automatic_trigger_status, status.clone(), &sink);
                    Ok(status)
                };
                let _ = reply.send(result);
            }
            Message::DisarmAutomaticTrigger(reply) => {
                let status = get_trigger_status(&automatic_trigger_status);
                let result = if status.recording {
                    Err(
                        "Stop the active recording before disarming the automatic trigger"
                            .to_owned(),
                    )
                } else {
                    automatic_trigger = None;
                    pre_trigger_samples.clear();
                    newest_elapsed_ns = 0;
                    if status.armed {
                        set_trigger_status(
                            &automatic_trigger_status,
                            AutomaticTriggerStatus::default(),
                            &sink,
                        );
                    }
                    Ok(get_trigger_status(&automatic_trigger_status))
                };
                let _ = reply.send(result);
            }
            Message::StopRecording(reply) => {
                let result = recorder
                    .take()
                    .ok_or_else(|| "No recording is active".to_owned())
                    .and_then(|writer| {
                        let result = writer.finish().map_err(|error| error.to_string());
                        recording_active.store(false, Ordering::Release);
                        if get_trigger_status(&automatic_trigger_status).recording {
                            set_trigger_status(
                                &automatic_trigger_status,
                                AutomaticTriggerStatus::default(),
                                &sink,
                            );
                        }
                        if let Ok(summary) = &result {
                            set_last_recording_path(&last_recording_path, &summary.path);
                        }
                        result
                    });
                let _ = reply.send(result);
            }
            Message::Marker(marker, reply) => {
                let result = match recorder.as_mut() {
                    Some(writer) => writer
                        .add_marker(&marker)
                        .map(|id| Marker {
                            id: Some(id),
                            ..marker
                        })
                        .map_err(|error| error.to_string()),
                    None => Err("Start recording before inserting a marker".to_owned()),
                };
                if let Ok(marker) = &result {
                    sink(EngineEvent::Marker(marker.clone()));
                }
                let _ = reply.send(result);
            }
            Message::PlaybackMarker(session_id, marker) => {
                sink(EngineEvent::PlaybackMarker { session_id, marker });
            }
            Message::PlaybackStarted => sink(EngineEvent::PlaybackStarted),
            Message::PlaybackFinished => sink(EngineEvent::PlaybackFinished),
            Message::PlaybackState(state) => sink(EngineEvent::PlaybackState(state)),
            Message::SourceDisconnected {
                obd,
                failed,
                last_source,
            } => {
                if last_source {
                    automatic_trigger = None;
                    pre_trigger_samples.clear();
                    newest_elapsed_ns = 0;
                    latest.store(0, Ordering::Release);
                    let trigger_status = get_trigger_status(&automatic_trigger_status);
                    if trigger_status.armed || trigger_status.recording {
                        set_trigger_status(
                            &automatic_trigger_status,
                            AutomaticTriggerStatus::default(),
                            &sink,
                        );
                    }
                    if failed && let Some(writer) = recorder.take() {
                        recording_active.store(false, Ordering::Release);
                        match writer.finish() {
                            Ok(summary) => {
                                set_last_recording_path(&last_recording_path, &summary.path);
                                sink(EngineEvent::RecordingFailed(summary));
                            }
                            Err(error) => sink(EngineEvent::Error(error.to_string())),
                        }
                    }
                }
                if obd && failed {
                    sink(EngineEvent::Disconnected);
                }
            }
            Message::Error(message) => sink(EngineEvent::Error(message)),
            Message::Shutdown => {
                if let Some(writer) = recorder.take() {
                    recording_active.store(false, Ordering::Release);
                    match writer.finish() {
                        Ok(summary) => set_last_recording_path(&last_recording_path, &summary.path),
                        Err(error) => sink(EngineEvent::Error(error.to_string())),
                    }
                }
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests;
