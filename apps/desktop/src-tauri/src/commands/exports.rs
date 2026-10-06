use lognook_core::{ResearchRunReport, Sample, SampleOrigin, export_csv};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

const MAX_LIVE_EXPORT_SAMPLES: usize = 100_000;

#[tauri::command]
pub async fn export_research_run_report(
    app: AppHandle,
    report: ResearchRunReport,
) -> Result<Option<String>, String> {
    let selected = app
        .dialog()
        .file()
        .add_filter("JSON reports", &["json"])
        .set_file_name("research-run-report.json")
        .blocking_save_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let mut path = selected.into_path().map_err(|error| error.to_string())?;
    path.set_extension("json");
    write_research_run_report(&report, &path)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

fn write_research_run_report(report: &ResearchRunReport, path: &Path) -> Result<(), String> {
    let mut json = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
    json.push(b'\n');
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    if let Err(error) = output.write_all(&json).and_then(|()| output.flush()) {
        drop(output);
        let _ = fs::remove_file(path);
        return Err(error.to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn export_live_csv(
    app: AppHandle,
    samples: Vec<Sample>,
) -> Result<Option<String>, String> {
    validate_live_samples(&samples)?;
    let selected = app
        .dialog()
        .file()
        .add_filter("CSV files", &["csv"])
        .set_file_name("live-samples.csv")
        .blocking_save_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected.into_path().map_err(|error| error.to_string())?;
    let mut path = path;
    path.set_extension("csv");
    write_live_samples_csv(&samples, &path)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

fn validate_live_samples(samples: &[Sample]) -> Result<(), String> {
    if samples.is_empty() {
        return Err("No live samples are available to export".into());
    }
    if samples.len() > MAX_LIVE_EXPORT_SAMPLES {
        return Err(format!(
            "Live CSV export is limited to {MAX_LIVE_EXPORT_SAMPLES} samples"
        ));
    }
    if samples.iter().any(|sample| {
        !matches!(&sample.origin, SampleOrigin::Live)
            || sample.elapsed_ns < 0
            || !sample.value.is_finite()
    }) {
        return Err("Live CSV export contains an invalid or non-live sample".into());
    }
    Ok(())
}

fn write_live_samples_csv(samples: &[Sample], path: &Path) -> Result<(), String> {
    validate_live_samples(samples)?;
    let mut ordered = samples.to_vec();
    ordered.sort_by(|left, right| {
        left.elapsed_ns
            .cmp(&right.elapsed_ns)
            .then_with(|| left.signal_id.cmp(&right.signal_id))
    });
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    if let Err(error) = export_csv(&ordered, output) {
        let _ = fs::remove_file(path);
        return Err(error.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_live_samples, write_live_samples_csv, write_research_run_report};
    use lognook_core::{Sample, SampleOrigin, SampleQuality, import_csv};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn sample(elapsed_ns: i64, value: f64) -> Sample {
        Sample {
            elapsed_ns,
            wall_time_unix_ns: elapsed_ns,
            source_id: "simulator".into(),
            ecu_id: None,
            signal_id: "engine_rpm".into(),
            value,
            unit: "rpm".into(),
            quality: SampleQuality::Valid,
            origin: SampleOrigin::Live,
            raw_bytes: None,
        }
    }

    fn test_path() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "lognook-live-export-{}-{stamp}.csv",
            std::process::id()
        ))
    }

    #[test]
    fn exports_the_live_buffer_as_csv_without_overwriting_existing_files() {
        let path = test_path();
        let samples = [sample(20, 900.0), sample(10, 800.0)];

        write_live_samples_csv(&samples, &path).unwrap();

        let restored = import_csv(fs::File::open(&path).unwrap()).unwrap();
        assert_eq!(
            restored
                .iter()
                .map(|sample| sample.elapsed_ns)
                .collect::<Vec<_>>(),
            [10, 20]
        );
        assert!(write_live_samples_csv(&samples, &path).is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn live_csv_rejects_empty_replay_and_invalid_samples() {
        assert!(validate_live_samples(&[]).is_err());
        let mut replay = sample(10, 800.0);
        replay.origin = SampleOrigin::Replay;
        assert!(validate_live_samples(&[replay]).is_err());
        assert!(validate_live_samples(&[sample(-1, 800.0)]).is_err());
        assert!(validate_live_samples(&[sample(10, f64::NAN)]).is_err());
    }

    #[test]
    fn run_report_json_round_trips_and_never_overwrites() {
        let path = test_path().with_extension("json");
        let report: lognook_core::ResearchRunReport = serde_json::from_value(serde_json::json!({
            "run_id": "run-42",
            "adapter": {"stable_id": "active-a30", "hardware_family": "A30", "firmware": "v1", "path": null},
            "started_at_unix_ms": 100,
            "completed_at_unix_ms": 200,
            "items": [{
                "queue_index": 1, "queue_size": 1,
                "selection": {"type": "a30ReadTest", "payload": "mode_06_tid_a2"},
                "result": {"type": "a30ReadTest", "payload": {
                    "test": "mode_06_tid_a2", "request_description": "Mode 06 TID A2",
                    "exact_adapter_command": "A30 RFCOMM TX 55 AA",
                    "adapter": {"stable_id": "active-a30", "hardware_family": "A30", "firmware": "v1", "path": null},
                    "protocol": "A30", "started_at_unix_ms": 110, "latency_ms": 20,
                    "outcome": "negative_response", "responder_header": "7E8",
                    "response_payloads": [[127, 6, 18]], "negative_response_code": 18,
                    "graph_values": [], "detail": null, "raw_response": "7F 06 12",
                    "evidence_saved": true, "evidence_save_error": null
                }}
            }]
        })).unwrap();
        write_research_run_report(&report, &path).unwrap();
        let original_bytes = fs::read(&path).unwrap();
        let restored: lognook_core::ResearchRunReport =
            serde_json::from_slice(&original_bytes).unwrap();
        assert_eq!(restored, report);
        let json: serde_json::Value = serde_json::from_slice(&original_bytes).unwrap();
        assert_eq!(json["run_id"], "run-42");
        assert_eq!(
            json["items"][0]["result"]["payload"]["negative_response_code"],
            18
        );
        let mut changed = report.clone();
        changed.run_id = "must-not-overwrite".into();
        assert!(write_research_run_report(&changed, &path).is_err());
        assert_eq!(fs::read(&path).unwrap(), original_bytes);
        fs::remove_file(path).unwrap();
    }
}
