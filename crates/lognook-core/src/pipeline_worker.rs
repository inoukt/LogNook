use crate::{
    model::{EngineEvent, Marker, SampleOrigin},
    pipeline::Message,
    recording::RecordingWriter,
};
use std::{
    path::{Path, PathBuf},
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
    last_recording_path: Arc<Mutex<Option<PathBuf>>>,
    sink: Arc<dyn Fn(EngineEvent) + Send + Sync>,
) {
    let mut recorder: Option<RecordingWriter> = None;
    while let Ok(message) = rx.recv() {
        match message {
            Message::Sample(sample) => {
                if !sample.value.is_finite() {
                    fail_recording(
                        &mut recorder,
                        &recording_active,
                        &last_recording_path,
                        &sink,
                        "Dropped non-finite measurement".into(),
                    );
                    continue;
                }
                if sample.origin == SampleOrigin::Live {
                    latest.store(sample.elapsed_ns, Ordering::Relaxed);
                    let write_error = recorder
                        .as_mut()
                        .and_then(|writer| writer.append(&sample).err());
                    if let Some(error) = write_error {
                        fail_recording(
                            &mut recorder,
                            &recording_active,
                            &last_recording_path,
                            &sink,
                            format!("Recording stopped: {error}"),
                        );
                        continue;
                    }
                }
                sink(EngineEvent::Sample(sample));
            }
            Message::PlaybackSample(session_id, sample) => {
                sink(EngineEvent::PlaybackSample { session_id, sample });
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
                let _ = reply.send(result);
            }
            Message::StopRecording(reply) => {
                let result = recorder
                    .take()
                    .ok_or_else(|| "No recording is active".to_owned())
                    .and_then(|writer| {
                        let result = writer.finish().map_err(|error| error.to_string());
                        recording_active.store(false, Ordering::Release);
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
            Message::Disconnected => sink(EngineEvent::Disconnected),
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

fn set_last_recording_path(last: &Mutex<Option<PathBuf>>, path: &Path) {
    if let Ok(mut last) = last.lock() {
        *last = Some(path.to_path_buf());
    }
}

fn fail_recording(
    recorder: &mut Option<RecordingWriter>,
    active: &AtomicBool,
    last_path: &Mutex<Option<PathBuf>>,
    sink: &Arc<dyn Fn(EngineEvent) + Send + Sync>,
    message: String,
) {
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
    sink(EngineEvent::Error(message));
}

#[cfg(test)]
mod tests {
    use crate::{
        model::{EngineEvent, Sample, SampleOrigin, SampleQuality},
        pipeline::SamplePipeline,
        read_recording,
    };
    use std::{
        sync::{Arc, mpsc},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn invalid_sample_stops_recording_and_keeps_the_partial_session_readable() {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lognook-write-error-{id}.lognook"));
        let (tx, rx) = mpsc::channel();
        let pipeline = SamplePipeline::new(Arc::new(move |event| {
            let _ = tx.send(event);
        }));
        pipeline.start_recording(path.clone()).unwrap();
        pipeline
            .sender()
            .send_sample(Sample {
                elapsed_ns: 10,
                wall_time_unix_ns: 1_800_000_000_000_000_010,
                source_id: "simulated-obd".into(),
                ecu_id: Some("engine".into()),
                signal_id: "engine_rpm".into(),
                value: f64::NAN,
                unit: "rpm".into(),
                quality: SampleQuality::Valid,
                origin: SampleOrigin::Live,
                raw_bytes: None,
            })
            .unwrap();

        let first = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let second = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            matches!(first, EngineEvent::RecordingFailed(_))
                || matches!(second, EngineEvent::RecordingFailed(_))
        );
        assert!(!pipeline.is_recording());
        assert!(read_recording(&path).unwrap().samples.is_empty());
        drop(pipeline);
        std::fs::remove_file(path).unwrap();
    }
}
