use lognook_core::{
    CanCaptureReader, CanEvidenceRecord, CanReplayPlan, SavedCanReplaySelection,
    SavedCanReplaySummary, prepare_saved_can_replay,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const PREVIEW_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_JOURNAL_LINE_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct SavedCanReplaySourceRef {
    pub source_id: String,
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct SavedCanReplayTargetView {
    pub generation: u64,
    pub connected: bool,
    pub source_id: Option<String>,
    pub channel: Option<u8>,
    pub bitrate: Option<u32>,
    pub listen_only: Option<bool>,
    pub output_capture_active: bool,
    pub output_capture_id: Option<u64>,
    pub can_start: bool,
    pub start_block_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
pub struct SavedCanReplayPreviewView {
    pub preview_id: String,
    pub source_id: String,
    pub source_name: String,
    pub summary: SavedCanReplaySummary,
    pub source_bitrate: Option<u32>,
    pub expires_at_unix_ms: u64,
    pub target: SavedCanReplayTargetView,
}

#[derive(Clone)]
pub(crate) struct CanReplayPreviewRegistry {
    inner: Arc<Mutex<RegistryState>>,
}

#[derive(Default)]
struct RegistryState {
    generation: u64,
    next_preview_id: u64,
    next_source_id: u64,
    preparation: Option<(u64, Arc<AtomicBool>)>,
    preview: Option<StoredPreview>,
    sources: HashMap<String, RegisteredSource>,
}

#[derive(Clone)]
struct RegisteredSource {
    reference: SavedCanReplaySourceRef,
    path: PathBuf,
}

#[derive(Clone)]
pub(crate) struct StoredPreview {
    pub(crate) preview_id: String,
    pub(crate) source_id: String,
    pub(crate) source_name: String,
    pub(crate) source_path: PathBuf,
    pub(crate) plan: CanReplayPlan,
    pub(crate) summary: SavedCanReplaySummary,
    expires_at: Instant,
    pub(crate) expires_at_unix_ms: u64,
}

impl Default for CanReplayPreviewRegistry {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RegistryState {
                next_preview_id: 1,
                next_source_id: 1,
                ..RegistryState::default()
            })),
        }
    }
}

impl CanReplayPreviewRegistry {
    pub(crate) fn register_source(&self, path: &Path) -> Result<SavedCanReplaySourceRef, String> {
        let path = canonical_can_capture(path)?;
        let metadata = std::fs::metadata(&path)
            .map_err(|error| format!("Could not inspect CAN capture: {error}"))?;
        if metadata.len() > MAX_SOURCE_BYTES {
            return Err("Saved CAN replay source exceeds 256 MiB".into());
        }
        CanCaptureReader::open(&path).map_err(|error| format!("Invalid CAN capture: {error}"))?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "CAN capture".into());
        let mut state = self.lock()?;
        if let Some(existing) = state.sources.values().find(|source| source.path == path) {
            return Ok(existing.reference.clone());
        }
        let id = state.next_source_id;
        state.next_source_id = id
            .checked_add(1)
            .ok_or_else(|| "Saved CAN source identifier space is exhausted".to_owned())?;
        let reference = SavedCanReplaySourceRef {
            source_id: format!("saved-can-source-{id:016x}"),
            name,
            size_bytes: metadata.len(),
        };
        state.sources.insert(
            reference.source_id.clone(),
            RegisteredSource {
                reference: reference.clone(),
                path,
            },
        );
        Ok(reference)
    }

    pub(crate) fn registered_sources(&self) -> Result<Vec<SavedCanReplaySourceRef>, String> {
        let mut state = self.lock()?;
        state.sources.retain(|_, source| source.path.is_file());
        Ok(state
            .sources
            .values()
            .map(|source| source.reference.clone())
            .collect())
    }

    pub(crate) fn registered_path(&self, source_id: &str) -> Result<Option<PathBuf>, String> {
        let state = self.lock()?;
        let Some(source) = state.sources.get(source_id) else {
            return Ok(None);
        };
        let current_path = canonical_can_capture(&source.path)?;
        if !same_path(&current_path, &source.path) {
            return Err("Saved CAN source path changed after registration".into());
        }
        Ok(Some(current_path))
    }

    pub(crate) fn prepare(
        &self,
        path: PathBuf,
        source_id: String,
        source_name: String,
        selection: SavedCanReplaySelection,
    ) -> Result<StoredPreview, String> {
        let (generation, cancelled) = self.begin_preparation()?;
        let result = (|| {
            let path = canonical_can_capture(&path)?;
            let source_bitrate = read_source_bitrate(&path)?;
            let prepared = prepare_saved_can_replay(&path, selection, &cancelled)
                .map_err(|error| error.to_string())?
                .with_source_bitrate(source_bitrate);
            if cancelled.load(Ordering::Acquire) {
                return Err("Saved CAN replay preparation was cancelled".into());
            }
            let mut state = self.lock()?;
            if state.generation != generation
                || state
                    .preparation
                    .as_ref()
                    .is_none_or(|(active_generation, _)| *active_generation != generation)
            {
                return Err("Saved CAN replay preparation was replaced".into());
            }
            let id = state.next_preview_id;
            state.next_preview_id = id.checked_add(1).ok_or_else(|| {
                "Saved CAN replay preview identifier space is exhausted".to_owned()
            })?;
            let expires_at_unix_ms = now_unix_ms().saturating_add(PREVIEW_TTL.as_millis() as u64);
            let preview = StoredPreview {
                preview_id: format!("saved-can-preview-{id:016x}"),
                source_id,
                source_name,
                source_path: path,
                summary: prepared.summary,
                plan: prepared.plan,
                expires_at: Instant::now() + PREVIEW_TTL,
                expires_at_unix_ms,
            };
            state.preparation = None;
            state.preview = Some(preview.clone());
            Ok(preview)
        })();
        if result.is_err() {
            self.finish_failed_preparation(generation);
        }
        result
    }

    pub(crate) fn preview(&self, preview_id: &str) -> Result<StoredPreview, String> {
        let mut state = self.lock()?;
        expire_preview(&mut state);
        state
            .preview
            .as_ref()
            .filter(|preview| preview.preview_id == preview_id)
            .cloned()
            .ok_or_else(|| "Saved CAN replay preview is missing, replaced, or expired".into())
    }

    pub(crate) fn start<T>(
        &self,
        preview_id: &str,
        start: impl FnOnce(&StoredPreview) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut state = self.lock()?;
        expire_preview(&mut state);
        let preview = state
            .preview
            .as_ref()
            .filter(|preview| preview.preview_id == preview_id)
            .cloned()
            .ok_or_else(|| {
                "Saved CAN replay preview is missing, replaced, or expired".to_owned()
            })?;
        let result = start(&preview)?;
        if state
            .preview
            .as_ref()
            .is_some_and(|current| current.preview_id == preview_id)
        {
            state.preview = None;
        }
        Ok(result)
    }

    pub(crate) fn discard(&self, preview_id: Option<&str>) -> Result<(), String> {
        let mut state = self.lock()?;
        if preview_id.is_some_and(|preview_id| {
            state
                .preview
                .as_ref()
                .is_none_or(|preview| preview.preview_id != preview_id)
        }) {
            return Err("Saved CAN replay preview is missing, replaced, or expired".into());
        }
        if let Some((_, cancellation)) = state.preparation.take() {
            cancellation.store(true, Ordering::Release);
        }
        state.preview = None;
        state.generation = state.generation.checked_add(1).ok_or_else(|| {
            "Saved CAN replay preparation generation space is exhausted".to_owned()
        })?;
        Ok(())
    }

    fn begin_preparation(&self) -> Result<(u64, Arc<AtomicBool>), String> {
        let mut state = self.lock()?;
        if let Some((_, cancellation)) = state.preparation.take() {
            cancellation.store(true, Ordering::Release);
        }
        state.preview = None;
        state.generation = state.generation.checked_add(1).ok_or_else(|| {
            "Saved CAN replay preparation generation space is exhausted".to_owned()
        })?;
        let generation = state.generation;
        let cancellation = Arc::new(AtomicBool::new(false));
        state.preparation = Some((generation, cancellation.clone()));
        Ok((generation, cancellation))
    }

    fn finish_failed_preparation(&self, generation: u64) {
        if let Ok(mut state) = self.inner.lock()
            && state.generation == generation
        {
            state.preparation = None;
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, RegistryState>, String> {
        self.inner
            .lock()
            .map_err(|_| "Saved CAN replay preview state is unavailable".into())
    }
}

fn expire_preview(state: &mut RegistryState) {
    if state
        .preview
        .as_ref()
        .is_some_and(|preview| Instant::now() >= preview.expires_at)
    {
        state.preview = None;
    }
}

pub(crate) fn canonical_can_capture(path: &Path) -> Result<PathBuf, String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("Could not open CAN capture: {error}"))?;
    if !path.is_file() || !path.file_name().is_some_and(is_can_capture_name) {
        return Err("Saved CAN replay accepts only .candump.csv captures".into());
    }
    Ok(path)
}

fn is_can_capture_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy()
        .to_ascii_lowercase()
        .ends_with(".candump.csv")
}

fn read_source_bitrate(path: &Path) -> Result<Option<u32>, String> {
    let evidence_path = lognook_core::can_evidence_path(path);
    let file = match File::open(evidence_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read CAN source evidence: {error}")),
    };
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let count = reader
        .by_ref()
        .take(MAX_JOURNAL_LINE_BYTES + 1)
        .read_until(b'\n', &mut line)
        .map_err(|error| format!("Could not read CAN source evidence: {error}"))?;
    if count as u64 > MAX_JOURNAL_LINE_BYTES {
        return Err("CAN source evidence line exceeds 256 KiB".into());
    }
    if count == 0 {
        return Ok(None);
    }
    let record: CanEvidenceRecord = serde_json::from_slice(&line)
        .map_err(|error| format!("Invalid CAN source evidence: {error}"))?;
    Ok(match record {
        CanEvidenceRecord::SessionStarted { bitrate, .. } => Some(bitrate),
        _ => None,
    })
}

#[cfg(windows)]
pub(crate) fn same_path(left: &Path, right: &Path) -> bool {
    if left == right
        || left
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    {
        return true;
    }
    fn identity(path: &Path) -> Option<(u32, u64)> {
        use std::os::windows::io::AsRawHandle;
        let file = File::open(path).ok()?;
        let mut info = std::mem::MaybeUninit::<
            windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION,
        >::uninit();
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(
                file.as_raw_handle(),
                info.as_mut_ptr(),
            )
        };
        if ok == 0 {
            return None;
        }
        let info = unsafe { info.assume_init() };
        Some((
            info.dwVolumeSerialNumber,
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        ))
    }
    identity(left)
        .zip(identity(right))
        .is_some_and(|(left, right)| left == right)
}

#[cfg(unix)]
pub(crate) fn same_path(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    if left == right {
        return true;
    }
    let (Ok(left), Ok(right)) = (std::fs::metadata(left), std::fs::metadata(right)) else {
        return false;
    };
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(any(windows, unix)))]
pub(crate) fn same_path(left: &Path, right: &Path) -> bool {
    left == right
}

pub(crate) fn source_is_active(source_path: &Path, active_path: Option<&Path>) -> bool {
    active_path.is_some_and(|active_path| same_path(source_path, active_path))
}

pub(crate) fn preview_view(
    preview: &StoredPreview,
    target: SavedCanReplayTargetView,
) -> SavedCanReplayPreviewView {
    SavedCanReplayPreviewView {
        preview_id: preview.preview_id.clone(),
        source_id: preview.source_id.clone(),
        source_name: preview.source_name.clone(),
        summary: preview.summary.clone(),
        source_bitrate: preview.summary.source_bitrate,
        expires_at_unix_ms: preview.expires_at_unix_ms,
        target,
    }
}

pub(crate) fn target_view(
    generation: u64,
    connected: bool,
    source_id: Option<String>,
    channel: Option<u8>,
    bitrate: Option<u32>,
    listen_only: Option<bool>,
    output_capture_active: bool,
    output_capture_id: Option<u64>,
    source_path: &Path,
    source_bitrate: Option<u32>,
    output_path: Option<&Path>,
) -> SavedCanReplayTargetView {
    let start_block_reason = if !connected {
        Some("Connect the intended Candlelight target".into())
    } else if listen_only == Some(true) {
        Some("Switch Candlelight to active mode".into())
    } else if !output_capture_active {
        Some("Start a separate output capture".into())
    } else if output_capture_id.is_none() {
        Some("The output capture changed; inspect the updated target".into())
    } else if output_path.is_some_and(|path| same_path(source_path, path)) {
        Some("Choose an output capture separate from the source".into())
    } else if source_bitrate.is_some_and(|source| Some(source) != bitrate) {
        Some("Target bitrate does not match the source capture".into())
    } else {
        None
    };
    SavedCanReplayTargetView {
        generation,
        connected,
        source_id,
        channel,
        bitrate,
        listen_only,
        output_capture_active,
        output_capture_id,
        can_start: start_block_reason.is_none(),
        start_block_reason,
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use lognook_core::{CanCaptureWriter, CanDirection, CanFrame};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FILE: AtomicU64 = AtomicU64::new(1);

    struct TempCapture(PathBuf);

    impl TempCapture {
        fn new() -> Self {
            let id = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "lognook-preview-test-{}-{id}.candump.csv",
                now_unix_ms()
            )))
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, values: &[u8]) {
            let _ = std::fs::remove_file(&self.0);
            let mut writer = CanCaptureWriter::create_new(&self.0).unwrap();
            for (index, value) in values.iter().enumerate() {
                writer
                    .write_frame(&CanFrame {
                        elapsed_ns: index as i64 * 1_000_000,
                        wall_time_unix_ns: 1_700_000_000_000_000_000,
                        can_id: 0x123,
                        extended: false,
                        direction: CanDirection::Rx,
                        data: vec![*value],
                        flags: 0,
                    })
                    .unwrap();
            }
            writer.finish().unwrap();
        }
    }

    impl Drop for TempCapture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            let _ = std::fs::remove_file(lognook_core::can_evidence_path(&self.0));
        }
    }

    fn selection() -> SavedCanReplaySelection {
        SavedCanReplaySelection {
            can_id: 0x123,
            extended: false,
            start_elapsed_ns: 0,
            end_elapsed_ns: 100_000_000,
        }
    }

    #[test]
    fn registration_rejects_non_capture_paths_and_source_path_aliases_are_detected() {
        let capture = TempCapture::new();
        capture.write(&[1]);
        let alias = capture
            .path()
            .with_file_name("lognook-preview-alias.candump.csv");
        std::fs::hard_link(capture.path(), &alias).unwrap();
        assert!(same_path(capture.path(), &alias));
        let registry = CanReplayPreviewRegistry::default();
        assert_eq!(
            registry.register_source(capture.path()).unwrap().name,
            capture
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        );
        assert!(registry.register_source(&alias).is_ok());
        assert!(source_is_active(capture.path(), Some(&alias)));
        assert!(!source_is_active(capture.path(), None));
        let invalid = capture.path().with_extension("txt");
        assert!(registry.register_source(&invalid).is_err());
        let _ = std::fs::remove_file(alias);
    }

    #[test]
    fn prepare_keeps_a_frozen_snapshot_and_new_previews_replace_old_tokens() {
        let capture = TempCapture::new();
        capture.write(&[0x11, 0x22]);
        let registry = CanReplayPreviewRegistry::default();
        let source = registry.register_source(capture.path()).unwrap();
        let first = registry
            .prepare(
                capture.path().to_owned(),
                source.source_id.clone(),
                source.name.clone(),
                selection(),
            )
            .unwrap();
        let digest = first.summary.source_sha256.clone();
        capture.write(&[0x99]);
        assert_eq!(first.plan.frames()[0].frame.data, [0x11]);
        assert_eq!(first.plan.frames()[1].frame.data, [0x22]);
        assert_eq!(first.summary.source_sha256, digest);

        let second = registry
            .prepare(
                capture.path().to_owned(),
                source.source_id,
                source.name,
                selection(),
            )
            .unwrap();
        assert_ne!(first.preview_id, second.preview_id);
        assert!(registry.preview(&first.preview_id).is_err());
        assert!(
            registry
                .start(&second.preview_id, |_| Err::<(), String>(
                    "guard failed".into()
                ))
                .is_err()
        );
        assert!(registry.preview(&second.preview_id).is_ok());
        assert_eq!(registry.start(&second.preview_id, |_| Ok(17)).unwrap(), 17);
        assert!(registry.preview(&second.preview_id).is_err());
    }

    #[test]
    fn discard_cancels_an_in_flight_preparation_token() {
        let registry = CanReplayPreviewRegistry::default();
        let (_, cancelled) = registry.begin_preparation().unwrap();
        assert!(!cancelled.load(Ordering::Acquire));
        registry.discard(None).unwrap();
        assert!(cancelled.load(Ordering::Acquire));
    }

    #[test]
    fn expired_preview_tokens_cannot_be_inspected_or_started() {
        let capture = TempCapture::new();
        capture.write(&[1]);
        let registry = CanReplayPreviewRegistry::default();
        let source = registry.register_source(capture.path()).unwrap();
        let preview = registry
            .prepare(
                capture.path().to_owned(),
                source.source_id,
                source.name,
                selection(),
            )
            .unwrap();
        registry
            .inner
            .lock()
            .unwrap()
            .preview
            .as_mut()
            .unwrap()
            .expires_at = Instant::now() - Duration::from_millis(1);
        assert!(registry.preview(&preview.preview_id).is_err());
        assert!(
            registry
                .start(&preview.preview_id, |_| Ok::<_, String>(()))
                .is_err()
        );
    }

    #[test]
    fn target_view_blocks_passive_capture_alias_and_bitrate_mismatch() {
        let source = TempCapture::new();
        source.write(&[1]);
        let output = TempCapture::new();
        output.write(&[2]);
        let passive = target_view(
            4,
            true,
            Some("can:fake".into()),
            Some(2),
            Some(500_000),
            Some(true),
            true,
            Some(2),
            source.path(),
            Some(500_000),
            Some(output.path()),
        );
        assert!(!passive.can_start);
        assert_eq!(
            passive.start_block_reason.as_deref(),
            Some("Switch Candlelight to active mode")
        );

        let alias = target_view(
            4,
            true,
            Some("can:fake".into()),
            Some(2),
            Some(500_000),
            Some(false),
            true,
            Some(2),
            source.path(),
            Some(500_000),
            Some(source.path()),
        );
        assert_eq!(
            alias.start_block_reason.as_deref(),
            Some("Choose an output capture separate from the source")
        );

        let mismatch = target_view(
            4,
            true,
            Some("can:fake".into()),
            Some(2),
            Some(500_000),
            Some(false),
            true,
            Some(2),
            source.path(),
            Some(250_000),
            Some(output.path()),
        );
        assert_eq!(
            mismatch.start_block_reason.as_deref(),
            Some("Target bitrate does not match the source capture")
        );
    }

    #[test]
    fn sidecar_session_metadata_provides_only_the_recorded_bitrate() {
        let capture = TempCapture::new();
        capture.write(&[1]);
        let sidecar = lognook_core::can_evidence_path(capture.path());
        let record = CanEvidenceRecord::SessionStarted {
            format_version: 2,
            capture_id: 1,
            bitrate: 250_000,
            listen_only: true,
            source_id: "can:source".into(),
        };
        std::fs::write(
            sidecar,
            format!("{}\n", serde_json::to_string(&record).unwrap()),
        )
        .unwrap();
        assert_eq!(read_source_bitrate(capture.path()).unwrap(), Some(250_000));
    }

    #[test]
    fn missing_source_sidecar_permits_unknown_bitrate() {
        let capture = TempCapture::new();
        capture.write(&[1]);
        assert_eq!(read_source_bitrate(capture.path()).unwrap(), None);
    }
}
