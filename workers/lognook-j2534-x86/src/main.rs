#[cfg(any(test, all(target_os = "windows", target_arch = "x86")))]
mod j2534;
#[cfg(any(test, all(target_os = "windows", target_arch = "x86")))]
mod registry;
#[cfg(any(test, all(target_os = "windows", target_arch = "x86")))]
mod session;

#[cfg(all(target_os = "windows", target_arch = "x86"))]
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(not(all(target_os = "windows", target_arch = "x86")))]
fn main() {
    eprintln!("The LogNook J2534 worker must run as 32-bit Windows.");
    std::process::exit(2);
}

#[cfg(all(target_os = "windows", target_arch = "x86"))]
fn run() -> Result<(), String> {
    use lognook_j2534_protocol::{
        MAX_MESSAGE_BYTES, WorkerErrorKind, WorkerResponse, decode_request_line,
        encode_response_line,
    };
    use std::io::{self, BufReader, BufWriter, Write};

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = BufReader::new(stdin.lock());
    let mut output = BufWriter::new(stdout.lock());
    let mut session: Option<session::J2534Session> = None;

    while let Some(line) =
        read_bounded_line(&mut input, MAX_MESSAGE_BYTES).map_err(|error| error.to_string())?
    {
        let started = std::time::Instant::now();
        let mut progress_error = None;
        let mut progress = |stage: &str| {
            if progress_error.is_none() {
                progress_error = write_progress(&mut output, stage, started).err();
            }
        };
        let response = match decode_request_line(&line) {
            Ok(request) => handle_request(request, &mut session, &mut progress),
            Err(error) => WorkerResponse::Error {
                kind: WorkerErrorKind::IpcFailure,
                message: error.to_string(),
            },
        };
        if let Some(error) = progress_error {
            return Err(error.to_string());
        }
        let encoded = encode_response_line(&response).unwrap_or_else(|_| {
            encode_response_line(&WorkerResponse::Error {
                kind: WorkerErrorKind::IpcFailure,
                message: "Worker response exceeds the IPC message limit".into(),
            })
            .expect("short worker error fits the IPC message limit")
        });
        output
            .write_all(&encoded)
            .map_err(|error| error.to_string())?;
        output.flush().map_err(|error| error.to_string())?;
    }
    drop(session);
    Ok(())
}

#[cfg(all(target_os = "windows", target_arch = "x86"))]
fn handle_request(
    request: lognook_j2534_protocol::WorkerRequest,
    session: &mut Option<session::J2534Session>,
    progress: &mut dyn FnMut(&str),
) -> lognook_j2534_protocol::WorkerResponse {
    use j2534::{DynamicJ2534, J2534Failure};
    use lognook_j2534_protocol::{WorkerErrorKind, WorkerRequest, WorkerResponse};

    let failed = |error: J2534Failure| WorkerResponse::Error {
        kind: error.kind,
        message: error.message,
    };
    match request {
        WorkerRequest::ListProviders => {
            progress("provider_resolution");
            match registry::list_openport2_providers() {
                Ok(discovery) => WorkerResponse::Providers {
                    providers: discovery.providers,
                    diagnostics: discovery.diagnostics,
                },
                Err(error) => failed(error),
            }
        }
        WorkerRequest::Identify { provider_id } => {
            if session.is_some() {
                return WorkerResponse::Error {
                    kind: WorkerErrorKind::ChannelFailed,
                    message: "Disconnect the current provider before identifying another".into(),
                };
            }
            let result = (|| {
                progress("provider_resolution");
                let provider = registry::resolve_openport2_provider(&provider_id)?;
                progress("dll_loading");
                let api = DynamicJ2534::load(&provider.dll_path)?;
                progress("passthru_open");
                let mut opened = session::J2534Session::open(Box::new(api))?;
                progress("version_read");
                opened.identity()
            })();
            match result {
                Ok(identity) => WorkerResponse::Identity { identity },
                Err(error) => failed(error),
            }
        }
        WorkerRequest::Connect { provider_id } => {
            if session.is_some() {
                return WorkerResponse::Error {
                    kind: WorkerErrorKind::ChannelFailed,
                    message: "A J2534 provider is already connected".into(),
                };
            }
            let result = (|| {
                progress("provider_resolution");
                let provider = registry::resolve_openport2_provider(&provider_id)?;
                progress("dll_loading");
                let api = DynamicJ2534::load(&provider.dll_path)?;
                progress("passthru_open");
                let mut opened = session::J2534Session::open(Box::new(api))?;
                let identity = opened.connect_with_progress(progress)?;
                Ok::<_, J2534Failure>((opened, identity))
            })();
            match result {
                Ok((opened, identity)) => {
                    *session = Some(opened);
                    WorkerResponse::Connected { identity }
                }
                Err(error) => failed(error),
            }
        }
        WorkerRequest::ConnectSubaruSsm { provider_id } => {
            if session.is_some() {
                return WorkerResponse::Error {
                    kind: WorkerErrorKind::ChannelFailed,
                    message: "A J2534 provider is already connected".into(),
                };
            }
            let result = (|| {
                progress("provider_resolution");
                let provider = registry::resolve_openport2_provider(&provider_id)?;
                progress("dll_loading");
                let api = DynamicJ2534::load(&provider.dll_path)?;
                progress("passthru_open");
                let mut opened = session::J2534Session::open(Box::new(api))?;
                let identity = opened.connect_subaru_ssm_with_progress(progress)?;
                Ok::<_, J2534Failure>((opened, identity))
            })();
            match result {
                Ok((opened, identity)) => {
                    *session = Some(opened);
                    WorkerResponse::Connected { identity }
                }
                Err(error) => failed(error),
            }
        }
        WorkerRequest::DiscoverMode01 => {
            let Some(connected) = session.as_mut() else {
                return WorkerResponse::Error {
                    kind: WorkerErrorKind::ChannelFailed,
                    message: "Connect a J2534 provider before PID discovery".into(),
                };
            };
            progress("mode01_discovery");
            match connected.discover_mode01() {
                Ok(responses) => WorkerResponse::Mode01Responses { responses },
                Err(error) => {
                    drop(session.take());
                    failed(error)
                }
            }
        }
        WorkerRequest::ReadMode01 { pid } => {
            let Some(connected) = session.as_mut() else {
                return WorkerResponse::Error {
                    kind: WorkerErrorKind::ChannelFailed,
                    message: "Connect a J2534 provider before reading a PID".into(),
                };
            };
            progress("pid_read");
            match connected.read_mode01(pid) {
                Ok(responses) => WorkerResponse::Mode01Responses { responses },
                Err(error) => {
                    drop(session.take());
                    failed(error)
                }
            }
        }
        WorkerRequest::ReadMode09 { pid } => {
            let Some(connected) = session.as_mut() else {
                return WorkerResponse::Error {
                    kind: WorkerErrorKind::ChannelFailed,
                    message: "Connect a J2534 provider before reading an identity PID".into(),
                };
            };
            progress("mode09_identity_read");
            match connected.read_mode09(pid) {
                Ok(responses) => WorkerResponse::Mode09Responses { responses },
                // Keep the serialized channel open so one unavailable identity PID cannot hide later reads.
                Err(error) => failed(error),
            }
        }
        WorkerRequest::ReadMode06 { test_id } => {
            let Some(connected) = session.as_mut() else {
                return WorkerResponse::Error {
                    kind: WorkerErrorKind::ChannelFailed,
                    message: "Connect a J2534 provider before reading Mode 06".into(),
                };
            };
            progress("mode06_read");
            match connected.read_mode06(test_id) {
                Ok(responses) => WorkerResponse::Mode06Responses { responses },
                Err(error) => failed(error),
            }
        }
        WorkerRequest::ReadStandardDtc { service } => {
            let Some(connected) = session.as_mut() else {
                return WorkerResponse::Error {
                    kind: WorkerErrorKind::ChannelFailed,
                    message: "Connect a J2534 provider before a standard DTC operation".into(),
                };
            };
            progress(
                if service == lognook_j2534_protocol::StandardDtcService::Clear {
                    "dtc_clear"
                } else {
                    "dtc_read"
                },
            );
            match connected.read_standard_dtc(service) {
                Ok(responses) => WorkerResponse::StandardDtcResponses { responses },
                // Keep the channel alive for the next explicit read. Clear is sent only
                // once by the J2534 session and is never retried here.
                Err(error) => failed(error),
            }
        }
        WorkerRequest::ReadOemCandidate {
            candidate_id,
            source_id,
            source_blob_sha,
            source_url,
            license,
            license_url,
            request_id,
            response_id,
            request,
        } => {
            if let Err(error) = lognook_j2534_protocol::validate_oem_candidate_request_fields(
                &candidate_id,
                &source_id,
                &source_blob_sha,
                &source_url,
                &license,
                &license_url,
                request_id,
                response_id,
            ) {
                return WorkerResponse::Error {
                    kind: WorkerErrorKind::MalformedResponse,
                    message: error.to_string(),
                };
            }
            let Some(connected) = session.as_mut() else {
                return WorkerResponse::Error {
                    kind: WorkerErrorKind::ChannelFailed,
                    message: "Connect a J2534 provider before an OEM candidate read".into(),
                };
            };
            progress("oem_candidate_read");
            match connected.read_oem_candidate(request_id, response_id, request) {
                Ok(responses) => WorkerResponse::OemCandidateResponses { responses },
                // One candidate is one request. Preserve the serialized channel for later
                // explicit attempts, with no automatic retry after a failed exchange.
                Err(error) => failed(error),
            }
        }
        WorkerRequest::ReadSubaruCandidate {
            candidate_id,
            source_id,
            catalog_sha256,
            request_id,
            response_id,
            request,
        } => {
            if let Err(error) = lognook_j2534_protocol::validate_subaru_catalog_request_fields(
                &candidate_id,
                &source_id,
                &catalog_sha256,
                request_id,
                response_id,
            ) {
                return WorkerResponse::Error {
                    kind: WorkerErrorKind::MalformedResponse,
                    message: error.to_string(),
                };
            }
            let Some(connected) = session.as_mut() else {
                return WorkerResponse::Error {
                    kind: WorkerErrorKind::ChannelFailed,
                    message: "Connect a J2534 provider before a Subaru catalog read".into(),
                };
            };
            progress("subaru_catalog_read");
            match connected.read_oem_candidate(request_id, response_id, request) {
                Ok(responses) => WorkerResponse::OemCandidateResponses { responses },
                Err(error) => failed(error),
            }
        }
        WorkerRequest::ReadSubaruSsmAddresses {
            candidate_ids,
            source_id,
            catalog_sha256,
            ecu_address,
            tool_address,
            addresses,
        } => {
            let Some(connected) = session.as_mut() else {
                return WorkerResponse::Error {
                    kind: WorkerErrorKind::ChannelFailed,
                    message: "Connect an OpenPort provider in Subaru SSM mode before an SSM read".into(),
                };
            };
            progress("subaru_ssm_read");
            match connected.read_subaru_ssm_addresses(
                &candidate_ids,
                &source_id,
                &catalog_sha256,
                ecu_address,
                tool_address,
                &addresses,
            ) {
                Ok(payload) => WorkerResponse::SubaruSsmRead { payload },
                Err(error) => failed(error),
            }
        }
        WorkerRequest::Disconnect => {
            drop(session.take());
            WorkerResponse::Disconnected
        }
    }
}

#[cfg(any(test, all(target_os = "windows", target_arch = "x86")))]
fn write_progress(
    output: &mut impl std::io::Write,
    stage: &str,
    started: std::time::Instant,
) -> std::io::Result<()> {
    let response = lognook_j2534_protocol::WorkerResponse::Progress {
        stage: stage.into(),
        detail: None,
        elapsed_ms: Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64),
    };
    let line =
        lognook_j2534_protocol::encode_response_line(&response).map_err(std::io::Error::other)?;
    output.write_all(&line)?;
    output.flush()
}

fn read_bounded_line<R: std::io::BufRead>(
    reader: &mut R,
    limit: usize,
) -> std::io::Result<Option<Vec<u8>>> {
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
                discard_to_newline(reader)?;
            }
            return Ok(Some(vec![0; limit.saturating_add(1)]));
        }
        if has_newline {
            return Ok(Some(line));
        }
    }
}

fn discard_to_newline<R: std::io::BufRead>(reader: &mut R) -> std::io::Result<()> {
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
    use super::read_bounded_line;
    use lognook_j2534_protocol::{
        MAX_MESSAGE_BYTES, ProtocolError, WorkerRequest, decode_request_line,
    };
    use std::io::Cursor;

    #[test]
    fn reads_one_json_line_without_consuming_the_next() {
        let mut input = Cursor::new(b"{\"type\":\"list_providers\"}\nnext\n".to_vec());
        let first = read_bounded_line(&mut input, MAX_MESSAGE_BYTES)
            .unwrap()
            .unwrap();
        assert_eq!(
            decode_request_line(&first).unwrap(),
            WorkerRequest::ListProviders
        );
        assert_eq!(
            read_bounded_line(&mut input, MAX_MESSAGE_BYTES)
                .unwrap()
                .unwrap(),
            b"next\n"
        );
    }

    #[test]
    fn oversized_line_is_discarded_before_the_next_message() {
        let mut input_bytes = vec![b'x'; MAX_MESSAGE_BYTES + 8];
        input_bytes.extend_from_slice(b"\n{\"type\":\"list_providers\"}\n");
        let mut input = Cursor::new(input_bytes);

        let oversized = read_bounded_line(&mut input, MAX_MESSAGE_BYTES)
            .unwrap()
            .unwrap();
        assert_eq!(
            decode_request_line(&oversized),
            Err(ProtocolError::TooLarge)
        );
        let next = read_bounded_line(&mut input, MAX_MESSAGE_BYTES)
            .unwrap()
            .unwrap();
        assert_eq!(
            decode_request_line(&next).unwrap(),
            WorkerRequest::ListProviders
        );
    }
    #[test]
    fn emitted_progress_has_stage_and_elapsed_time_without_traffic() {
        let mut output = Vec::new();
        super::write_progress(&mut output, "pid_read", std::time::Instant::now()).unwrap();
        assert!(
            matches!(lognook_j2534_protocol::decode_response_line(&output).unwrap(),
            lognook_j2534_protocol::WorkerResponse::Progress { stage, detail: None, elapsed_ms: Some(_) } if stage == "pid_read")
        );
        assert!(!String::from_utf8(output).unwrap().contains("bytes"));
    }

    #[cfg(all(target_os = "windows", target_arch = "x86"))]
    #[test]
    fn provider_resolution_progress_precedes_typed_terminal_failure() {
        for request in [
            lognook_j2534_protocol::WorkerRequest::Identify {
                provider_id: "lognook-nonexistent-test-provider".into(),
            },
            lognook_j2534_protocol::WorkerRequest::Connect {
                provider_id: "lognook-nonexistent-test-provider".into(),
            },
        ] {
            let mut session = None;
            let mut stages = Vec::new();
            let terminal = super::handle_request(request, &mut session, &mut |stage| {
                stages.push(stage.to_owned())
            });
            assert_eq!(stages, ["provider_resolution"]);
            assert!(matches!(
                terminal,
                lognook_j2534_protocol::WorkerResponse::Error {
                    kind: lognook_j2534_protocol::WorkerErrorKind::ProviderNotFound,
                    ..
                }
            ));
            assert!(session.is_none());
        }
    }
}
