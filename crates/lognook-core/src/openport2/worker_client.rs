use super::WorkerSession;
use crate::{
    DiagnosticPhase, DiagnosticProgress, DiagnosticSink, diagnostics::noop_diagnostic_sink,
    model::LogNookError,
};
use lognook_j2534_protocol::{
    MAX_MESSAGE_BYTES, WorkerRequest, WorkerResponse, decode_response_line, encode_request_line,
};
use std::{
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

const SHORT_TIMEOUT: Duration = Duration::from_secs(1);
const SETUP_TIMEOUT: Duration = Duration::from_secs(3);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct WorkerClient {
    child: Option<Child>,
    input: Option<BufWriter<ChildStdin>>,
    responses: Receiver<Result<Vec<u8>, String>>,
    closed: bool,
}

impl WorkerClient {
    pub(super) fn spawn(worker_path: &Path) -> Result<Self, LogNookError> {
        if !worker_path.is_file() {
            return Err(LogNookError::new(format!(
                "OpenPort worker is missing at {}. Build it with `npm run build:j2534-worker`.",
                worker_path.display()
            )));
        }
        let child = Command::new(worker_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                LogNookError::new(format!(
                    "Could not start OpenPort worker at {}: {error}",
                    worker_path.display()
                ))
            })?;
        Self::from_child(child)
    }

    fn from_child(mut child: Child) -> Result<Self, LogNookError> {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LogNookError::new("OpenPort worker stdout is unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| LogNookError::new("OpenPort worker stderr is unavailable"))?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| LogNookError::new("OpenPort worker stdin is unavailable"))?;
        let (sender, responses) = mpsc::channel();
        thread::spawn(move || read_worker_output(stdout, sender));
        thread::spawn(move || {
            let mut stderr = stderr;
            let _ = io::copy(&mut stderr, &mut io::sink());
        });
        Ok(Self {
            child: Some(child),
            input: Some(BufWriter::new(input)),
            responses,
            closed: false,
        })
    }

    fn terminate(&mut self) {
        self.input.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.closed = true;
    }
}

fn worker_timeout_message(request: &WorkerRequest, timeout: Duration) -> String {
    let stage = match request {
        WorkerRequest::ListProviders => "provider discovery".to_owned(),
        WorkerRequest::Identify { .. } => "adapter identification".to_owned(),
        WorkerRequest::Connect { .. } => "J2534 channel setup".to_owned(),
        WorkerRequest::DiscoverMode01 => "Mode 01 PID discovery".to_owned(),
        WorkerRequest::ReadMode01 { pid } => format!("Mode 01 PID {pid:02X} read"),
        WorkerRequest::Disconnect => "adapter disconnect".to_owned(),
    };
    format!(
        "OpenPort worker timed out after {} ms during {stage}; the worker was stopped",
        timeout.as_millis()
    )
}

impl WorkerSession for WorkerClient {
    fn request(&mut self, request: WorkerRequest) -> Result<WorkerResponse, LogNookError> {
        self.request_with_progress(request, &noop_diagnostic_sink())
    }
    fn request_with_progress(
        &mut self,
        request: WorkerRequest,
        sink: &DiagnosticSink,
    ) -> Result<WorkerResponse, LogNookError> {
        let timeout = match &request {
            WorkerRequest::DiscoverMode01 => DISCOVERY_TIMEOUT,
            WorkerRequest::Identify { .. } | WorkerRequest::Connect { .. } => SETUP_TIMEOUT,
            _ => SHORT_TIMEOUT,
        };
        self.request_with_timeout_and_progress(request, timeout, sink)
    }
    fn shutdown(&mut self) {
        self.terminate();
    }
}

impl WorkerClient {
    #[cfg(test)]
    fn request_with_timeout(
        &mut self,
        request: WorkerRequest,
        timeout: Duration,
    ) -> Result<WorkerResponse, LogNookError> {
        self.request_with_timeout_and_progress(request, timeout, &noop_diagnostic_sink())
    }

    fn request_with_timeout_and_progress(
        &mut self,
        request: WorkerRequest,
        timeout: Duration,
        sink: &DiagnosticSink,
    ) -> Result<WorkerResponse, LogNookError> {
        if self.closed {
            return Err(LogNookError::new("OpenPort worker is no longer running"));
        }
        let line = encode_request_line(&request).map_err(|error| {
            LogNookError::new(format!("Could not encode OpenPort worker request: {error}"))
        })?;
        let write_result = self
            .input
            .as_mut()
            .ok_or_else(|| LogNookError::new("OpenPort worker input is unavailable"))?
            .write_all(&line)
            .and_then(|()| self.input.as_mut().expect("worker input exists").flush());
        if let Err(error) = write_result {
            self.terminate();
            return Err(LogNookError::new(format!(
                "Could not write to OpenPort worker: {error}"
            )));
        }
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.terminate();
                return Err(LogNookError::new(worker_timeout_message(&request, timeout)));
            }
            let response_line = match self.responses.recv_timeout(remaining) {
                Ok(Ok(line)) => line,
                Ok(Err(error)) => {
                    self.terminate();
                    return Err(LogNookError::new(format!("OpenPort worker {error}")));
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.terminate();
                    return Err(LogNookError::new(worker_timeout_message(&request, timeout)));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.terminate();
                    return Err(LogNookError::new(
                        "OpenPort worker output closed unexpectedly",
                    ));
                }
            };
            match decode_response_line(&response_line) {
                Ok(WorkerResponse::Progress {
                    stage,
                    detail,
                    elapsed_ms,
                }) => {
                    let (operation, adapter_id) = match &request {
                        WorkerRequest::ListProviders => ("provider_discovery", None),
                        WorkerRequest::Identify { provider_id } => {
                            ("identify", Some(provider_id.clone()))
                        }
                        WorkerRequest::Connect { provider_id } => {
                            ("connect", Some(provider_id.clone()))
                        }
                        WorkerRequest::Disconnect => ("disconnect", None),
                        _ => ("acquisition", None),
                    };
                    sink(DiagnosticProgress {
                        adapter_id,
                        operation: operation.into(),
                        stage,
                        phase: DiagnosticPhase::Progress,
                        duration_ms: elapsed_ms,
                        signal_ids: Vec::new(),
                        detail,
                        consecutive_failures: None,
                        transient: None,
                    });
                }
                Ok(response) => return Ok(response),
                Err(error) => {
                    self.terminate();
                    return Err(LogNookError::new(format!(
                        "OpenPort worker returned malformed output: {error}"
                    )));
                }
            }
        }
    }
}

impl Drop for WorkerClient {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn read_worker_output(stdout: impl Read, sender: mpsc::Sender<Result<Vec<u8>, String>>) {
    let mut reader = BufReader::new(stdout);
    loop {
        match read_bounded_line(&mut reader, MAX_MESSAGE_BYTES) {
            Ok(Some(line)) => {
                if sender.send(Ok(line)).is_err() {
                    return;
                }
            }
            Ok(None) => {
                let _ = sender.send(Err("closed stdout".into()));
                return;
            }
            Err(error) => {
                let _ = sender.send(Err(format!("stdout read failed: {error}")));
                return;
            }
        }
    }
}

fn read_bounded_line<R: BufRead>(reader: &mut R, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::with_capacity(256);
    loop {
        let (count, has_newline, too_large) = {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                return Ok((!line.is_empty()).then_some(line));
            }
            let newline = available.iter().position(|byte| *byte == b'\n');
            let count = newline.map_or(available.len(), |position| position + 1);
            let too_large = line.len().saturating_add(count) > limit;
            if !too_large {
                line.extend_from_slice(&available[..count]);
            }
            (count, newline.is_some(), too_large)
        };
        reader.consume(count);
        if too_large {
            if !has_newline {
                discard_line(reader)?;
            }
            return Ok(Some(vec![0; limit.saturating_add(1)]));
        }
        if has_newline {
            return Ok(Some(line));
        }
    }
}

fn discard_line<R: BufRead>(reader: &mut R) -> io::Result<()> {
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(());
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(available.len(), |position| position + 1);
        reader.consume(count);
        if newline.is_some() {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkerClient, read_bounded_line, worker_timeout_message};
    use lognook_j2534_protocol::{
        MAX_MESSAGE_BYTES, ProtocolError, WorkerRequest, decode_request_line,
    };
    use std::{
        io::Cursor,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    #[test]
    fn output_reader_discards_oversized_lines_and_keeps_next_message() {
        let mut bytes = vec![b'x'; MAX_MESSAGE_BYTES + 1];
        bytes.extend_from_slice(b"\n{\"type\":\"disconnect\"}\n");
        let mut reader = Cursor::new(bytes);
        let oversized = read_bounded_line(&mut reader, MAX_MESSAGE_BYTES)
            .unwrap()
            .unwrap();
        assert_eq!(
            decode_request_line(&oversized),
            Err(ProtocolError::TooLarge)
        );
        let next = read_bounded_line(&mut reader, MAX_MESSAGE_BYTES)
            .unwrap()
            .unwrap();
        assert_eq!(
            decode_request_line(&next).unwrap(),
            WorkerRequest::Disconnect
        );
    }

    #[test]
    fn worker_timeout_names_the_openport_request_stage() {
        let message = worker_timeout_message(
            &WorkerRequest::ReadMode01 { pid: 0x00 },
            Duration::from_millis(1_000),
        );

        assert!(message.contains("timed out after 1000 ms"));
        assert!(message.contains("Mode 01 PID 00 read"));
    }

    #[cfg(windows)]
    #[test]
    fn worker_timeout_kills_the_stuck_child_process() {
        let child = Command::new("cmd.exe")
            .args(["/C", "ping -n 10 127.0.0.1 > NUL"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut worker = WorkerClient::from_child(child).unwrap();
        let started = Instant::now();
        let error = worker
            .request_with_timeout(WorkerRequest::ListProviders, Duration::from_millis(50))
            .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(worker.closed);
        assert!(worker.child.is_none());
    }
    #[cfg(windows)]
    fn scripted_worker(script: &str) -> WorkerClient {
        let child = Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        WorkerClient::from_child(child).unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn ordered_progress_returns_only_terminal_success_or_typed_error() {
        use crate::DiagnosticSink;
        use crate::openport2::WorkerSession;
        use lognook_j2534_protocol::{WorkerErrorKind, WorkerResponse};
        use std::sync::{Arc, Mutex};
        for terminal in [
            WorkerResponse::Disconnected,
            WorkerResponse::Error {
                kind: WorkerErrorKind::PassThruFailed,
                message: "PassThruOpen status 0x08: exact failure".into(),
            },
        ] {
            let terminal_line =
                String::from_utf8(lognook_j2534_protocol::encode_response_line(&terminal).unwrap())
                    .unwrap();
            let script = format!(
                r#"$null = [Console]::ReadLine(); [Console]::WriteLine('{{"type":"progress","stage":"provider_resolution","detail":null,"elapsed_ms":0}}'); [Console]::WriteLine('{{"type":"progress","stage":"dll_loading","detail":"LoadLibraryW","elapsed_ms":1}}'); [Console]::WriteLine('{}')"#,
                terminal_line.trim()
            );
            let mut worker = scripted_worker(&script);
            let events = Arc::new(Mutex::new(Vec::new()));
            let captured = events.clone();
            let sink: DiagnosticSink = Arc::new(move |event| captured.lock().unwrap().push(event));
            assert_eq!(
                worker
                    .request_with_progress(
                        WorkerRequest::Identify {
                            provider_id: "opta".into()
                        },
                        &sink
                    )
                    .unwrap(),
                terminal
            );
            let events = events.lock().unwrap();
            assert_eq!(
                events.iter().map(|e| e.stage.as_str()).collect::<Vec<_>>(),
                ["provider_resolution", "dll_loading"]
            );
            assert_eq!(events[0].adapter_id.as_deref(), Some("opta"));
            assert_eq!(events[0].operation, "identify");
        }
    }

    #[cfg(windows)]
    #[test]
    fn progress_does_not_extend_deadline_and_timeout_stops_worker() {
        let mut worker = scripted_worker(
            r#"$null = [Console]::ReadLine(); while ($true) { [Console]::WriteLine('{"type":"progress","stage":"dll_loading","detail":null,"elapsed_ms":0}'); Start-Sleep -Milliseconds 100 }"#,
        );
        let started = Instant::now();
        let error = worker
            .request_with_timeout_and_progress(
                WorkerRequest::ListProviders,
                Duration::from_millis(1200),
                &crate::diagnostics::noop_diagnostic_sink(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("timed out after 1200 ms"));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(worker.closed);
        assert!(worker.child.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn oversized_progress_stops_worker() {
        let mut worker = scripted_worker(
            r#"$null = [Console]::ReadLine(); [Console]::WriteLine('{"type":"progress","stage":"dll_loading","detail":"' + ('x' * 17000) + '","elapsed_ms":0}'); Start-Sleep -Seconds 10"#,
        );
        let error = worker
            .request_with_timeout_and_progress(
                WorkerRequest::ListProviders,
                Duration::from_secs(3),
                &crate::diagnostics::noop_diagnostic_sink(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("exceeds"));
        assert!(worker.closed);
    }
}
