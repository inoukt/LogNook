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
        let response = match decode_request_line(&line) {
            Ok(request) => handle_request(request, &mut session),
            Err(error) => WorkerResponse::Error {
                kind: WorkerErrorKind::IpcFailure,
                message: error.to_string(),
            },
        };
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
) -> lognook_j2534_protocol::WorkerResponse {
    use j2534::{DynamicJ2534, J2534Failure};
    use lognook_j2534_protocol::{WorkerErrorKind, WorkerRequest, WorkerResponse};

    let failed = |error: J2534Failure| WorkerResponse::Error {
        kind: error.kind,
        message: error.message,
    };
    match request {
        WorkerRequest::ListProviders => match registry::list_openport2_providers() {
            Ok(providers) => WorkerResponse::Providers { providers },
            Err(error) => failed(error),
        },
        WorkerRequest::Identify { provider_id } => {
            if session.is_some() {
                return WorkerResponse::Error {
                    kind: WorkerErrorKind::ChannelFailed,
                    message: "Disconnect the current provider before identifying another".into(),
                };
            }
            let result = (|| {
                let provider = registry::resolve_openport2_provider(&provider_id)?;
                let api = DynamicJ2534::load(&provider.dll_path)?;
                let mut opened = session::J2534Session::open(Box::new(api))?;
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
                let provider = registry::resolve_openport2_provider(&provider_id)?;
                let api = DynamicJ2534::load(&provider.dll_path)?;
                let mut opened = session::J2534Session::open(Box::new(api))?;
                let identity = opened.connect()?;
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
            match connected.read_mode01(pid) {
                Ok(responses) => WorkerResponse::Mode01Responses { responses },
                Err(error) => {
                    drop(session.take());
                    failed(error)
                }
            }
        }
        WorkerRequest::Disconnect => {
            drop(session.take());
            WorkerResponse::Disconnected
        }
    }
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
}
