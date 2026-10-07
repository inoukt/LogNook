use super::protocol::*;
use crate::AppState;
use interprocess::local_socket::traits::tokio::Listener as _;
#[cfg(unix)]
use interprocess::local_socket::{GenericFilePath, ToFsName};
#[cfg(windows)]
use interprocess::local_socket::{GenericNamespaced, ToNsName};
use interprocess::local_socket::{ListenerOptions, Name};
#[cfg(windows)]
use interprocess::os::windows::{
    local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
};
use std::{
    io,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::{AppHandle, Manager, async_runtime};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    sync::watch,
};

const ENDPOINT_NAME: &str = "LogNookAgentControl";

#[derive(Default)]
pub(crate) struct AgentIpcService {
    endpoint: String,
    running: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    shutdown: Mutex<Option<watch::Sender<bool>>>,
}

impl AgentIpcService {
    pub(crate) fn new(app_data_dir: &Path) -> Self {
        Self {
            endpoint: endpoint_name(app_data_dir),
            running: Arc::new(AtomicBool::new(false)),
            connected: Arc::new(AtomicBool::new(false)),
            shutdown: Mutex::new(None),
        }
    }

    pub(crate) fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub(crate) fn connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub(crate) async fn start(&self, app: AppHandle) -> Result<(), String> {
        if self
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return self.running_start_result();
        }
        let listener = match bind_listener(&self.endpoint) {
            Ok(listener) => listener,
            Err(error) => {
                self.running.store(false, Ordering::Release);
                return Err(error.to_string());
            }
        };
        let (shutdown, receiver) = watch::channel(false);
        let task_shutdown = shutdown.clone();
        *self
            .shutdown
            .lock()
            .map_err(|_| "Agent Access state is unavailable".to_owned())? = Some(shutdown);
        let running = Arc::clone(&self.running);
        let connected = Arc::clone(&self.connected);
        async_runtime::spawn(async move {
            if let Err(error) = serve(
                listener,
                app,
                receiver,
                task_shutdown,
                Arc::clone(&connected),
            )
            .await
            {
                eprintln!("LogNook Agent Access IPC stopped: {error}");
            }
            connected.store(false, Ordering::Release);
            running.store(false, Ordering::Release);
        });
        Ok(())
    }

    pub(crate) fn stop(&self) {
        if let Ok(mut shutdown) = self.shutdown.lock() {
            if let Some(shutdown) = shutdown.take() {
                let _ = shutdown.send(true);
            }
        }
    }

    fn running_start_result(&self) -> Result<(), String> {
        let shutdown = self
            .shutdown
            .lock()
            .map_err(|_| "Agent Access state is unavailable".to_owned())?;
        if shutdown
            .as_ref()
            .is_some_and(|shutdown| !*shutdown.borrow())
        {
            Ok(())
        } else {
            Err("Agent Access listener is stopping; wait before unlocking again".into())
        }
    }
}

pub(crate) fn endpoint_name(app_data_dir: &Path) -> String {
    if cfg!(windows) {
        ENDPOINT_NAME.to_owned()
    } else {
        app_data_dir
            .join("agent-control.sock")
            .to_string_lossy()
            .into_owned()
    }
}

pub(crate) fn parse_endpoint(endpoint: &str) -> io::Result<Name<'static>> {
    #[cfg(windows)]
    {
        endpoint.to_owned().to_ns_name::<GenericNamespaced>()
    }
    #[cfg(not(windows))]
    {
        endpoint.to_owned().to_fs_name::<GenericFilePath>()
    }
}

fn bind_listener(endpoint: &str) -> io::Result<interprocess::local_socket::tokio::Listener> {
    let name = parse_endpoint(endpoint)?;
    let options = ListenerOptions::new().name(name);
    #[cfg(windows)]
    let options = {
        let descriptor_text = widestring::U16CString::from_str("D:P(A;;GA;;;SY)(A;;GA;;;OW)")
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let descriptor = SecurityDescriptor::deserialize(&descriptor_text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        options.security_descriptor(descriptor)
    };
    #[cfg(unix)]
    let options = options.mode(0o600);
    options.create_tokio()
}

async fn serve(
    listener: interprocess::local_socket::tokio::Listener,
    app: AppHandle,
    mut shutdown: watch::Receiver<bool>,
    stop_sender: watch::Sender<bool>,
    connected: Arc<AtomicBool>,
) -> io::Result<()> {
    let mut access_check = tokio::time::interval(Duration::from_secs(1));
    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        tokio::select! {
            _ = shutdown.changed() => return Ok(()),
            _ = access_check.tick() => {
                if !agent_access_unlocked(&app) {
                    let _ = stop_sender.send(true);
                    let _ = super::tools::stop_active_operations(
                        &app,
                        lognook_core::CanScanStopReason::AgentAccessRevoked,
                    );
                    return Ok(());
                }
            }
            result = listener.accept() => match result {
                Ok(stream) => {
                    if connected.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
                        tokio::spawn(async move {
                            let mut stream = stream;
                            let reply = AgentResponse::error(AgentErrorCode::OneAgentHost, "Only one local agent host can connect at a time");
                            let _ = write_message(&mut stream, &reply).await;
                        });
                        continue;
                    }
                    let app = app.clone();
                    let connected = Arc::clone(&connected);
                    let shutdown = shutdown.clone();
                    tokio::spawn(async move {
                        let _ = handle_client(stream, &app, shutdown).await;
                        connected.store(false, Ordering::Release);
                    });
                }
                Err(error) => return Err(error),
            }
        }
    }
}

async fn handle_client(
    stream: interprocess::local_socket::tokio::Stream,
    app: &AppHandle,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let mut reader = BufReader::new(stream);
    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        let bytes = tokio::select! {
            _ = shutdown.changed() => return Ok(()),
            result = read_message(&mut reader) => result?,
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if !agent_access_unlocked(app) { return Ok(()); }
                continue;
            }
        };
        let Some(bytes) = bytes else { return Ok(()) };
        let response = match decode_request(&bytes) {
            Ok(request) => super::tools::dispatch(app, request).await,
            Err(error) => AgentResponse::Error { error },
        };
        write_message(reader.get_mut(), &response).await?;
        if !agent_access_unlocked(app) {
            let _ = super::tools::stop_active_operations(
                app,
                lognook_core::CanScanStopReason::AgentAccessRevoked,
            );
            return Ok(());
        }
    }
}

fn agent_access_unlocked(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .agent_access
        .lock()
        .is_ok_and(|mut access| access.status().unlocked)
}

async fn read_message<R: AsyncBufRead + Unpin>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    let length = (&mut *reader)
        .take((MAX_IPC_MESSAGE_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .await?;
    if length == 0 {
        return Ok(None);
    }
    if length > MAX_IPC_MESSAGE_BYTES || bytes.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversized or incomplete Agent Access message",
        ));
    }
    bytes.pop();
    Ok(Some(bytes))
}

async fn write_message<W: AsyncWrite + Unpin, T: serde::Serialize>(
    writer: &mut W,
    value: &T,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > MAX_IPC_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Agent Access response exceeds the 1 MiB limit",
        ));
    }
    writer.write_all(&bytes).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::access::AgentAccessManager;
    use interprocess::{ConnectWaitMode, local_socket::ConnectOptions};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn ipc_requires_unlocked_session() {
        let directory =
            std::env::temp_dir().join(format!("lognook-agent-ipc-{}", std::process::id()));
        let mut access = AgentAccessManager::open(directory.join("pin.hash")).unwrap();
        assert!(access.authorize_action().is_err());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn ipc_allows_one_agent_host() {
        let connected = AtomicBool::new(false);
        assert!(
            connected
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        );
        assert!(
            connected
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        );
    }

    #[tokio::test]
    async fn ipc_reports_missing_app_endpoint() {
        let name = format!("LogNookAgentMissing{}", std::process::id());
        let result = ConnectOptions::new()
            .name(parse_endpoint(&name).unwrap())
            .wait_mode(ConnectWaitMode::Timeout(Duration::from_millis(50)))
            .connect_tokio()
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn ipc_does_not_report_success_while_listener_is_stopping() {
        let service = AgentIpcService::new(&std::env::temp_dir());
        let (shutdown, _receiver) = watch::channel(false);
        *service.shutdown.lock().unwrap() = Some(shutdown.clone());
        service.running.store(true, Ordering::Release);
        assert!(service.running_start_result().is_ok());
        shutdown.send(true).unwrap();
        assert!(service.running_start_result().is_err());
        service.stop();
        assert!(service.running_start_result().is_err());
    }
}
