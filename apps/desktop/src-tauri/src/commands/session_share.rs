use super::{engine, sessions::selected_recording_path};
use crate::AppState;
use lognook_core::inspect_recording;
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::State;

const MAX_UPLOAD_BYTES: u64 = 200_000_000;
const MAX_RESPONSE_BYTES: usize = 4096;
const UPLOAD_URL: &str = "https://catbox.moe/user/api.php";

#[derive(Serialize)]
pub struct ShareLink {
    pub url: String,
    pub expires_in: String,
    pub filename: String,
}

#[tauri::command]
pub async fn share_recording_link(
    state: State<'_, AppState>,
    path: String,
) -> Result<ShareLink, String> {
    let (snapshot, filename) = {
        let engine = engine(&state)?;
        if engine.is_recording() {
            return Err("Stop recording before sharing a session".into());
        }
        let source = selected_recording_path(&state.recordings_dir, Path::new(&path))?;
        ensure_no_sidecars(&source)?;
        inspect_recording(&source).map_err(|error| error.to_string())?;
        let size = std::fs::metadata(&source)
            .map_err(|error| error.to_string())?
            .len();
        check_upload_size(size)?;
        let filename = source
            .file_name()
            .ok_or_else(|| "Session filename is unavailable".to_owned())?
            .to_string_lossy()
            .into_owned();
        let temp_dir = TempShareDir::create()?;
        let snapshot = temp_dir.path.join(&filename);
        std::fs::copy(&source, &snapshot).map_err(|error| error.to_string())?;
        check_upload_size(
            std::fs::metadata(&snapshot)
                .map_err(|error| error.to_string())?
                .len(),
        )?;
        inspect_recording(&snapshot).map_err(|error| error.to_string())?;
        drop(engine);
        (temp_dir, filename)
    };

    upload_snapshot(&snapshot.path.join(&filename), filename).await
}

fn check_upload_size(size: u64) -> Result<(), String> {
    if size > MAX_UPLOAD_BYTES {
        return Err("Session exceeds Catbox's 200 MB upload limit".into());
    }
    Ok(())
}

fn ensure_no_sidecars(source: &Path) -> Result<(), String> {
    for suffix in ["-wal", "-shm", "-journal"] {
        if PathBuf::from(format!("{}{suffix}", source.to_string_lossy())).exists() {
            return Err(
                "Close the session's writer before sharing; SQLite sidecar data must be finalized"
                    .into(),
            );
        }
    }
    Ok(())
}

async fn upload_snapshot(path: &Path, filename: String) -> Result<ShareLink, String> {
    upload_snapshot_to(path, filename, UPLOAD_URL).await
}

async fn upload_snapshot_to(
    path: &Path,
    filename: String,
    endpoint: &str,
) -> Result<ShareLink, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    let file = reqwest::multipart::Part::file(path)
        .await
        .map_err(|error| error.to_string())?
        .file_name(filename.clone())
        .mime_str("application/octet-stream")
        .map_err(|error| error.to_string())?;
    let form = reqwest::multipart::Form::new()
        .text("reqtype", "fileupload")
        .part("fileToUpload", file);
    let response = client
        .post(endpoint)
        .multipart(form)
        .send()
        .await
        .map_err(|error| format!("Temporary upload failed: {error}"))?;
    let status = response.status();
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err("Temporary upload returned an oversized response".into());
        }
        body.extend_from_slice(&chunk);
    }
    let body = String::from_utf8(body)
        .map_err(|_| "Temporary upload returned a non-text response".to_owned())?;
    let url = parse_upload_response(status, &body)?;
    Ok(ShareLink {
        url,
        expires_in: "No scheduled expiry; service retention is not guaranteed".into(),
        filename,
    })
}

fn parse_upload_response(status: reqwest::StatusCode, body: &str) -> Result<String, String> {
    if !status.is_success() {
        return Err(format!("Temporary upload failed with HTTP {status}"));
    }
    validate_share_url(body.trim())
}

fn validate_share_url(value: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| "Temporary upload returned an invalid link".to_owned())?;
    if url.scheme() != "https"
        || url.host_str() != Some("files.catbox.moe")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !url.path().starts_with('/')
        || url.path() == "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Temporary upload returned an unsafe link".into());
    }
    Ok(url.to_string())
}

struct TempShareDir {
    path: PathBuf,
}

impl TempShareDir {
    fn create() -> Result<Self, String> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        for _ in 0..16 {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = base.join(format!(
                "lognook-share-{}-{timestamp}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Err(error) =
                            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                        {
                            let _ = std::fs::remove_dir(&path);
                            return Err(error.to_string());
                        }
                    }
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("Could not create a temporary session copy".into())
    }
}

impl Drop for TempShareDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_UPLOAD_BYTES, TempShareDir, check_upload_size, ensure_no_sidecars,
        parse_upload_response, upload_snapshot_to, validate_share_url,
    };
    use reqwest::StatusCode;
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn validates_catbox_https_link_without_credentials_or_extra_parts() {
        assert_eq!(
            validate_share_url("https://files.catbox.moe/1234/session.lognook").unwrap(),
            "https://files.catbox.moe/1234/session.lognook"
        );
        for value in [
            "http://files.catbox.moe/1234/session.lognook",
            "https://files.catbox.moe.evil.test/1234/session.lognook",
            "https://user@files.catbox.moe/1234/session.lognook",
            "https://files.catbox.moe:8443/1234/session.lognook",
            "https://files.catbox.moe/1234/session.lognook?redirect=evil",
        ] {
            assert!(validate_share_url(value).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn checks_provider_http_status_and_plain_text_link() {
        assert_eq!(
            parse_upload_response(
                StatusCode::OK,
                "https://files.catbox.moe/1234/run.lognook\n"
            )
            .unwrap(),
            "https://files.catbox.moe/1234/run.lognook"
        );
        assert!(parse_upload_response(StatusCode::BAD_REQUEST, "Internal Server Error").is_err());
        assert!(parse_upload_response(StatusCode::OK, "not a URL").is_err());
        assert!(parse_upload_response(StatusCode::OK, "https://example.com/x").is_err());
    }

    #[test]
    fn temp_copy_directory_is_removed_on_drop() {
        let dir = TempShareDir::create().unwrap();
        let path = dir.path.clone();
        assert!(path.exists());
        drop(dir);
        assert!(!path.exists());
        assert_eq!(MAX_UPLOAD_BYTES, 200_000_000);
    }

    #[test]
    fn refuses_snapshot_while_sqlite_sidecar_data_exists() {
        let dir = TempShareDir::create().unwrap();
        let source = dir.path.join("session.lognook");
        assert!(ensure_no_sidecars(&source).is_ok());
        let wal = dir.path.join("session.lognook-wal");
        std::fs::write(&wal, b"pending data").unwrap();
        assert!(ensure_no_sidecars(&source).is_err());
        std::fs::remove_file(wal).unwrap();
        assert!(ensure_no_sidecars(&source).is_ok());
    }

    #[test]
    fn enforces_provider_size_limit() {
        assert!(check_upload_size(MAX_UPLOAD_BYTES).is_ok());
        assert!(check_upload_size(MAX_UPLOAD_BYTES + 1).is_err());
    }

    #[test]
    fn accepts_only_native_files_inside_managed_recordings_directory() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let recordings = std::env::temp_dir().join(format!("lognook-share-path-{nonce}"));
        let outside = recordings.with_extension("lognook");
        std::fs::create_dir(&recordings).unwrap();
        let managed = recordings.join("managed.lognook");
        std::fs::write(&managed, b"").unwrap();
        std::fs::write(&outside, b"").unwrap();

        assert!(super::selected_recording_path(&recordings, &managed).is_ok());
        assert!(super::selected_recording_path(&recordings, &outside).is_err());
        assert!(super::selected_recording_path(&recordings, &recordings).is_err());

        std::fs::remove_dir_all(&recordings).unwrap();
        std::fs::remove_file(outside).unwrap();
    }

    #[test]
    fn serializes_share_link_fields_as_snake_case() {
        let link = super::ShareLink {
            url: "https://files.catbox.moe/x.lognook".into(),
            expires_in: "No scheduled expiry; service retention is not guaranteed".into(),
            filename: "x.lognook".into(),
        };
        let value = serde_json::to_value(link).unwrap();
        assert!(value.get("expires_in").is_some());
        assert!(value.get("expiresIn").is_none());
    }

    #[tokio::test]
    async fn sends_catbox_multipart_to_local_mock() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                headers.push_str(&line);
            }
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .expect("multipart request should have a content length");
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let body = String::from_utf8(body).unwrap();
            assert!(body.contains("name=\"reqtype\""));
            assert!(body.contains("fileupload"));
            assert!(body.contains("name=\"fileToUpload\"; filename=\"session.lognook\""));
            assert!(body.contains("recording-bytes"));
            let stream = reader.get_mut();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                "https://files.catbox.moe/mock.lognook".len(),
                "https://files.catbox.moe/mock.lognook"
            )
            .unwrap();
        });
        let path = std::env::temp_dir().join(format!(
            "lognook-share-http-{}.lognook",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"recording-bytes").unwrap();

        let result = upload_snapshot_to(
            &path,
            "session.lognook".into(),
            &format!("http://{address}/upload"),
        )
        .await;

        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(result.unwrap().url, "https://files.catbox.moe/mock.lognook");
    }
}
