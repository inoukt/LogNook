use lognook_j2534_protocol::{
    J2534Identity, J2534Provider, J2534ProviderDiscovery, MAX_MESSAGE_BYTES, Mode01Payload,
    ProtocolError, WorkerErrorKind, WorkerRequest, WorkerResponse, decode_request_line,
    decode_response_line, encode_request_line, encode_response_line,
};

#[test]
fn request_variants_round_trip_and_keep_the_line_framing() {
    assert_eq!(
        encode_request_line(&WorkerRequest::ListProviders).unwrap(),
        b"{\"type\":\"list_providers\"}\n"
    );

    let requests = [
        WorkerRequest::ListProviders,
        WorkerRequest::Identify {
            provider_id: "opta".into(),
        },
        WorkerRequest::Connect {
            provider_id: "tactrix".into(),
        },
        WorkerRequest::DiscoverMode01,
        WorkerRequest::ReadMode01 { pid: 0x0C },
        WorkerRequest::Disconnect,
    ];
    for request in requests {
        let line = encode_request_line(&request).unwrap();
        assert_eq!(line.last(), Some(&b'\n'));
        assert_eq!(decode_request_line(&line).unwrap(), request);
    }
}

#[test]
fn response_variants_round_trip_with_provider_identity_and_raw_payload() {
    let responses = [
        WorkerResponse::Providers {
            providers: vec![J2534Provider {
                id: "opta".into(),
                name: "OPTA Tactrix OpenPort 2.0".into(),
            }],
            diagnostics: vec!["Ignored DENSO: unsupported by LogNook".into()],
        },
        WorkerResponse::Identity {
            identity: J2534Identity {
                firmware: "1.17.4877".into(),
                dll_version: "0.1.0".into(),
                api_version: "04.04".into(),
            },
        },
        WorkerResponse::Connected {
            identity: J2534Identity {
                firmware: "1.17.4877".into(),
                dll_version: "0.1.0".into(),
                api_version: "04.04".into(),
            },
        },
        WorkerResponse::Mode01Responses {
            responses: vec![Mode01Payload {
                ecu_id: "7E8".into(),
                bytes: vec![0x41, 0x0C, 0x1A, 0xF8],
                latency_us: 11_300,
            }],
        },
        WorkerResponse::Disconnected,
        WorkerResponse::Error {
            kind: WorkerErrorKind::Timeout,
            message: "J2534 read timed out".into(),
        },
    ];

    for response in responses {
        let line = encode_response_line(&response).unwrap();
        assert_eq!(decode_response_line(&line).unwrap(), response);
    }
}

#[test]
fn provider_discovery_round_trips_empty_list_with_diagnostics() {
    let discovery = J2534ProviderDiscovery {
        providers: Vec::new(),
        diagnostics: vec![
            "Ignored registered provider 'DENSO': LogNook supports Tactrix/OpenPort/OPTA only."
                .into(),
        ],
    };
    let response = WorkerResponse::Providers {
        providers: discovery.providers.clone(),
        diagnostics: discovery.diagnostics.clone(),
    };

    let encoded = encode_response_line(&response).unwrap();
    assert_eq!(decode_response_line(&encoded).unwrap(), response);
}

#[test]
fn malformed_unknown_and_incomplete_messages_are_rejected() {
    assert!(matches!(
        decode_request_line(b"{"),
        Err(ProtocolError::Malformed(_))
    ));
    assert!(matches!(
        decode_request_line(b"{\"type\":\"scan_everything\"}\n"),
        Err(ProtocolError::Malformed(_))
    ));
    assert!(matches!(
        decode_request_line(b"{\"type\":\"identify\"}\n"),
        Err(ProtocolError::Malformed(_))
    ));
    assert!(matches!(
        decode_request_line(b"{\"type\":\"list_providers\",\"raw\":\"2101\"}\n"),
        Err(ProtocolError::Malformed(_))
    ));
    assert!(matches!(
        decode_response_line(b"{\"type\":\"mode01_responses\",\"responses\":[{}]}\n"),
        Err(ProtocolError::Malformed(_))
    ));
}

#[test]
fn oversized_request_and_response_lines_are_rejected() {
    let oversized_line = vec![b' '; MAX_MESSAGE_BYTES + 1];
    assert_eq!(
        decode_request_line(&oversized_line),
        Err(ProtocolError::TooLarge)
    );

    let oversized_response = WorkerResponse::Mode01Responses {
        responses: vec![Mode01Payload {
            ecu_id: "7E8".into(),
            bytes: vec![0; MAX_MESSAGE_BYTES],
            latency_us: 1,
        }],
    };
    assert_eq!(
        encode_response_line(&oversized_response),
        Err(ProtocolError::TooLarge)
    );
}

#[test]
fn progress_round_trips_and_rejects_oversized_details() {
    let progress = WorkerResponse::Progress {
        stage: "dll_loading".into(),
        detail: Some("LoadLibraryW".into()),
        elapsed_ms: Some(12),
    };
    assert_eq!(
        decode_response_line(&encode_response_line(&progress).unwrap()).unwrap(),
        progress
    );
    assert_eq!(
        encode_response_line(&WorkerResponse::Progress {
            stage: "dll_loading".into(),
            detail: Some("x".repeat(MAX_MESSAGE_BYTES)),
            elapsed_ms: None,
        }),
        Err(ProtocolError::TooLarge)
    );
    assert_eq!(
        decode_response_line(&vec![b' '; MAX_MESSAGE_BYTES + 1]),
        Err(ProtocolError::TooLarge)
    );
}
