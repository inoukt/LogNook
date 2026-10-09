use crate::{AppState, diagnostics::DiagnosticRecord};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

#[tauri::command]
pub(crate) fn diagnostic_snapshot(
    state: State<'_, AppState>,
) -> Result<Vec<DiagnosticRecord>, String> {
    Ok(state.diagnostics.snapshot())
}

#[tauri::command]
pub async fn choose_diagnostics_recording(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("LogNook recordings", &["lognook"])
            .blocking_pick_file()
            .map(|selected| {
                let path = selected.into_path().map_err(|error| error.to_string())?;
                validate_recording(&path)?;
                Ok(path.to_string_lossy().into_owned())
            })
            .transpose()
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn export_support_bundle(
    app: AppHandle,
    state: State<'_, AppState>,
    recording_path: Option<String>,
) -> Result<Option<String>, String> {
    let diagnostics_path = state.diagnostics_path.clone();
    let evidence_directory = state
        .oem_validation_log
        .parent()
        .ok_or("OEM evidence directory is unavailable")?
        .to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let recording = recording_path.map(PathBuf::from);
        if let Some(path) = &recording {
            validate_recording(path)?;
        }
        let selected = app
            .dialog()
            .file()
            .add_filter("Support bundles", &["zip"])
            .set_file_name("lognook-support.zip")
            .blocking_save_file();
        let Some(selected) = selected else {
            return Ok(None);
        };
        let mut destination = selected.into_path().map_err(|error| error.to_string())?;
        destination.set_extension("zip");
        let diagnostics_paths = existing_diagnostics_paths(&diagnostics_path)?;
        let evidence_paths = evidence_paths(&evidence_directory)?;
        write_support_bundle(
            &destination,
            &bundle_metadata(),
            &diagnostics_paths,
            &evidence_paths,
            recording.as_deref(),
        )?;
        Ok(Some(destination.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|error| error.to_string())?
}

fn existing_diagnostics_paths(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = vec![];
    for candidate in [
        path.to_path_buf(),
        path.with_file_name("operations.1.jsonl"),
        path.with_file_name("operations.2.jsonl"),
        path.with_file_name("vehicle-reads.jsonl"),
    ] {
        if candidate.try_exists().map_err(|error| error.to_string())? {
            paths.push(candidate);
        }
    }
    Ok(paths)
}

fn evidence_paths(directory: &Path) -> Result<Vec<PathBuf>, String> {
    if !directory.try_exists().map_err(|error| error.to_string())? {
        return Ok(vec![]);
    }
    let mut paths = vec![];
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "jsonl")
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn validate_recording(path: &Path) -> Result<(), String> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lognook"))
        || !path.is_file()
    {
        return Err("Choose an existing .lognook recording file".into());
    }
    File::open(path).map_err(|error| format!("Cannot open recording: {error}"))?;
    Ok(())
}

fn bundle_metadata() -> serde_json::Value {
    serde_json::json!({
        "app_version": env!("CARGO_PKG_VERSION"),
        "os": {
            "name": std::env::consts::OS,
            "architecture": std::env::consts::ARCH,
            "version": os_version(),
        },
    })
}

#[cfg(not(windows))]
fn os_version() -> serde_json::Value {
    serde_json::Value::Null
}

#[cfg(windows)]
fn os_version() -> serde_json::Value {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW},
    };
    let key: Vec<u16> = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\0"
        .encode_utf16()
        .collect();
    let read_string = |name: &str| -> Option<String> {
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let mut buffer = [0u16; 256];
        let mut size = std::mem::size_of_val(&buffer) as u32;
        // The buffers are initialized, writable, and sized in bytes; key/value names are NUL terminated.
        let result = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        (result == ERROR_SUCCESS).then(|| {
            String::from_utf16_lossy(
                &buffer[..buffer
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(buffer.len())],
            )
        })
    };
    let mut ubr = 0u32;
    let mut size = std::mem::size_of_val(&ubr) as u32;
    let name: Vec<u16> = "UBR\0".encode_utf16().collect();
    // UBR is a DWORD; the output points to one writable u32 with its exact size.
    let result = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut ubr as *mut u32).cast(),
            &mut size,
        )
    };
    serde_json::json!({
        "product_name": read_string("ProductName"),
        "display_version": read_string("DisplayVersion"),
        "current_build_number": read_string("CurrentBuildNumber"),
        "ubr": (result == ERROR_SUCCESS).then_some(ubr),
    })
}

fn write_support_bundle(
    destination: &Path,
    metadata: &serde_json::Value,
    diagnostics_paths: &[PathBuf],
    evidence_paths: &[PathBuf],
    recording_path: Option<&Path>,
) -> Result<(), String> {
    if let Some(path) = recording_path {
        validate_recording(path)?;
    }
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                "Support bundle destination already exists".to_string()
            } else {
                error.to_string()
            }
        })?;
    let result = (|| -> Result<(), String> {
        let mut archive = ZipWriter::new(output);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        archive
            .start_file("metadata.json", options)
            .map_err(|error| error.to_string())?;
        serde_json::to_writer_pretty(&mut archive, metadata).map_err(|error| error.to_string())?;
        for (directory, paths) in [
            ("diagnostics", diagnostics_paths),
            ("oem-validation", evidence_paths),
        ] {
            for path in paths {
                copy_entry(&mut archive, directory, path, options)?;
            }
        }
        if let Some(path) = recording_path {
            copy_entry(&mut archive, "recordings", path, options)?;
        }
        archive
            .finish()
            .map_err(|error| error.to_string())?
            .sync_all()
            .map_err(|error| error.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        // This operation owns the file because create_new succeeded; never remove an existing output.
        fs::remove_file(destination).map_err(|error| {
            format!(
                "{}; partial ZIP cleanup failed: {error}",
                result.as_ref().unwrap_err()
            )
        })?;
    }
    result
}

fn copy_entry(
    archive: &mut ZipWriter<File>,
    directory: &str,
    path: &Path,
    options: SimpleFileOptions,
) -> Result<(), String> {
    let basename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Source filename is not valid Unicode")?;
    let source =
        File::open(path).map_err(|error| format!("Cannot open {}: {error}", path.display()))?;
    let metadata = source.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err(format!("Source is not a regular file: {}", path.display()));
    }
    archive
        .start_file(format!("{directory}/{basename}"), options.large_file(true))
        .map_err(|error| error.to_string())?;
    // Cap the stream at its starting size so concurrent logging cannot extend the export indefinitely.
    let copied =
        io::copy(&mut source.take(metadata.len()), archive).map_err(|error| error.to_string())?;
    if copied != metadata.len() {
        return Err(format!(
            "Source was shortened during export: {}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::Read,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "lognook-bundle-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn source(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, bytes).unwrap();
            path
        }
        fn destination(&self) -> PathBuf {
            self.0.join("support.zip")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn metadata() -> serde_json::Value {
        serde_json::json!({"app_version":"test", "os":{"name":"windows", "version":"test", "architecture":"x86_64"}})
    }
    fn entries(path: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        (0..archive.len())
            .map(|i| {
                let mut entry = archive.by_index(i).unwrap();
                let name = entry.name().to_string();
                let mut bytes = vec![];
                entry.read_to_end(&mut bytes).unwrap();
                (name, bytes)
            })
            .collect()
    }

    #[test]
    fn bundle_contains_metadata_diagnostics_and_oem_evidence() {
        let f = Fixture::new();
        let diagnostics = [
            "operations.jsonl",
            "operations.1.jsonl",
            "operations.2.jsonl",
        ]
        .map(|name| f.source(name, b"{\"stage\":\"complete\"}\n"));
        let evidence = f.source("oem.jsonl", b"{\"raw\":\"61 01 02\"}\n");
        write_support_bundle(
            &f.destination(),
            &metadata(),
            &diagnostics,
            std::slice::from_ref(&evidence),
            None,
        )
        .unwrap();
        let archive = entries(&f.destination());
        assert_eq!(archive.len(), 5);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&archive["metadata.json"]).unwrap(),
            metadata()
        );
        for path in diagnostics {
            let name = format!(
                "diagnostics/{}",
                path.file_name().unwrap().to_str().unwrap()
            );
            assert_eq!(archive[&name], fs::read(&path).unwrap());
            assert_eq!(fs::read(path).unwrap(), b"{\"stage\":\"complete\"}\n");
        }
        assert_eq!(
            archive["oem-validation/oem.jsonl"],
            b"{\"raw\":\"61 01 02\"}\n"
        );
        assert_eq!(fs::read(evidence).unwrap(), b"{\"raw\":\"61 01 02\"}\n");
    }

    #[test]
    fn bundle_includes_neutral_obd_read_test_evidence() {
        let f = Fixture::new();
        let evidence_directory = f.0.join("oem-validation");
        let a30 = f.source("oem-validation/a30-read-tests.jsonl", b"a30 evidence");
        let sx = f.source("oem-validation/obd-read-tests.jsonl", b"sx evidence");
        let paths = evidence_paths(&evidence_directory).unwrap();
        write_support_bundle(&f.destination(), &metadata(), &[], &paths, None).unwrap();

        let archive = entries(&f.destination());
        assert_eq!(
            archive["oem-validation/obd-read-tests.jsonl"],
            fs::read(sx).unwrap()
        );
        assert_eq!(
            archive["oem-validation/a30-read-tests.jsonl"],
            fs::read(a30).unwrap()
        );
    }

    #[test]
    fn bundle_omits_recording_by_default() {
        let f = Fixture::new();
        f.source("recordings/private.lognook", b"private recording");
        write_support_bundle(&f.destination(), &metadata(), &[], &[], None).unwrap();
        assert_eq!(
            entries(&f.destination()).keys().collect::<Vec<_>>(),
            vec!["metadata.json"]
        );
    }

    #[test]
    fn bundle_includes_only_the_selected_recording() {
        let f = Fixture::new();
        let selected = f.source("outside-app/selected.lognook", b"selected recording");
        let other = f.source("recordings/other.lognook", b"other recording");
        write_support_bundle(&f.destination(), &metadata(), &[], &[], Some(&selected)).unwrap();
        let archive = entries(&f.destination());
        assert_eq!(archive.len(), 2);
        assert_eq!(
            archive["recordings/selected.lognook"],
            b"selected recording"
        );
        assert_eq!(fs::read(selected).unwrap(), b"selected recording");
        assert_eq!(fs::read(other).unwrap(), b"other recording");
    }

    #[test]
    fn bundle_refuses_overwrite() {
        let f = Fixture::new();
        fs::write(f.destination(), b"existing").unwrap();
        let result = write_support_bundle(&f.destination(), &metadata(), &[], &[], None);
        assert!(result.unwrap_err().contains("exists"));
        assert_eq!(fs::read(f.destination()).unwrap(), b"existing");
    }

    #[test]
    fn failed_bundle_write_removes_partial_file() {
        let f = Fixture::new();
        let missing = f.0.join("missing.jsonl");
        let result = write_support_bundle(&f.destination(), &metadata(), &[missing], &[], None);
        assert!(result.unwrap_err().contains("missing.jsonl"));
        assert!(!f.destination().exists());
    }

    #[test]
    fn bundle_rejects_invalid_recording_selection() {
        let f = Fixture::new();
        let wrong_extension = f.source("session.csv", b"data");
        let directory = f.0.join("directory.lognook");
        fs::create_dir(&directory).unwrap();
        for source in [wrong_extension, directory, f.0.join("missing.lognook")] {
            assert!(
                write_support_bundle(&f.destination(), &metadata(), &[], &[], Some(&source))
                    .is_err()
            );
            assert!(!f.destination().exists());
        }
    }

    #[test]
    fn streamed_bundle_entries_use_zip64_and_round_trip() {
        let f = Fixture::new();
        let diagnostics = f.source("operations.jsonl", b"diagnostics");
        let evidence = f.source("evidence.jsonl", b"evidence");
        let recording = f.source("external/selected.lognook", b"selected recording");
        write_support_bundle(
            &f.destination(),
            &metadata(),
            std::slice::from_ref(&diagnostics),
            std::slice::from_ref(&evidence),
            Some(&recording),
        )
        .unwrap();
        let bytes = fs::read(f.destination()).unwrap();
        let mut archive = zip::ZipArchive::new(fs::File::open(f.destination()).unwrap()).unwrap();
        for (name, source) in [
            ("diagnostics/operations.jsonl", diagnostics),
            ("oem-validation/evidence.jsonl", evidence),
            ("recordings/selected.lognook", recording),
        ] {
            let mut entry = archive.by_name(name).unwrap();
            let header = entry.header_start() as usize;
            // A ZIP64 local header reserves both 32-bit size fields and stores sizes
            // in extra field 0x0001, even for this small fixture.
            assert_eq!(
                &bytes[header + 18..header + 26],
                &[0xff; 8],
                "{name} must reserve ZIP64 sizes"
            );
            let name_length =
                u16::from_le_bytes(bytes[header + 26..header + 28].try_into().unwrap()) as usize;
            let extra = header + 30 + name_length;
            assert_eq!(&bytes[extra..extra + 4], &[0x01, 0x00, 0x10, 0x00]);
            let mut restored = vec![];
            entry.read_to_end(&mut restored).unwrap();
            assert_eq!(restored, fs::read(source).unwrap());
        }
    }

    #[test]
    fn bundle_metadata_contains_only_application_and_os_fields() {
        let metadata = bundle_metadata();
        assert_eq!(metadata.as_object().unwrap().len(), 2);
        assert_eq!(metadata["app_version"], env!("CARGO_PKG_VERSION"));
        let os = metadata["os"].as_object().unwrap();
        assert_eq!(os.len(), 3);
        assert_eq!(os["name"], std::env::consts::OS);
        assert_eq!(os["architecture"], std::env::consts::ARCH);
        #[cfg(windows)]
        {
            let version = os["version"].as_object().unwrap();
            assert_eq!(
                version.keys().map(String::as_str).collect::<Vec<_>>(),
                vec![
                    "current_build_number",
                    "display_version",
                    "product_name",
                    "ubr"
                ]
            );
        }
    }

    #[test]
    fn bundle_sources_include_existing_rotations_and_only_jsonl_evidence() {
        let f = Fixture::new();
        let current = f.source("diagnostics/operations.jsonl", b"current");
        let previous = f.source("diagnostics/operations.2.jsonl", b"previous");
        let vehicle = f.source(
            "diagnostics/vehicle-reads.jsonl",
            b"vehicle diagnostic evidence",
        );
        f.source("diagnostics/unrelated.jsonl", b"unrelated");
        let evidence = f.source("oem-validation/research.jsonl", b"evidence");
        f.source("oem-validation/private.txt", b"private");
        fs::create_dir(f.0.join("oem-validation/directory.jsonl")).unwrap();
        assert_eq!(
            existing_diagnostics_paths(&current).unwrap(),
            vec![current, previous, vehicle]
        );
        assert_eq!(
            evidence_paths(&f.0.join("oem-validation")).unwrap(),
            vec![evidence]
        );
        assert!(evidence_paths(&f.0.join("absent")).unwrap().is_empty());
    }
}
