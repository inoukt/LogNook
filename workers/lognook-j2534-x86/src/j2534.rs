#[cfg(all(target_os = "windows", target_arch = "x86"))]
use lognook_j2534_protocol::OemCandidateFailure;
use lognook_j2534_protocol::{
    J2534Identity, Mode01Payload, Mode09Failure, Mode09MalformedFrame, Mode09Payload,
    Mode09Response, OemCandidateMalformedFrame, OemCandidatePayload, OemCandidateRequestKind,
    OemCandidateResponse, StandardDtcFailure, StandardDtcMalformedFrame, StandardDtcPayload,
    StandardDtcResponse, StandardDtcService, WorkerErrorKind,
};
use std::fmt;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
use std::time::Duration;

pub(crate) const ISO15765_PROTOCOL_ID: u32 = 6;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
const ISO9141_PROTOCOL_ID: u32 = 3;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
const ISO9141_NO_CHECKSUM: u32 = 0x200;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
const PASS_FILTER: u32 = 0x01;
const ISO15765_FRAME_PAD: u32 = 0x40;
const FLOW_CONTROL_FILTER: u32 = 0x03;
const RX_STATUS_TX_MSG_TYPE: u32 = 0x01;
// J2534 ISO-TP receive-start notifications carry only the CAN address.
const RX_STATUS_START_OF_MESSAGE: u32 = 0x02;
const RX_STATUS_RX_BREAK: u32 = 0x04;
const RX_STATUS_ISO15765_ADDR_TYPE: u32 = 0x80;
const RX_STATUS_CAN_29BIT_ID: u32 = 0x100;
const ERR_TIMEOUT: u32 = 0x09;
const ERR_BUFFER_EMPTY: u32 = 0x10;
const MESSAGE_DATA_SIZE: usize = 4128;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
// OPTA can lose an already-received reply while waiting to fill a larger batch.
// Read one at a time and retain the bounded loop's per-ECU response collection.
const MAX_READ_MESSAGES: usize = 1;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
const READ_WINDOW: Duration = Duration::from_millis(250);
#[cfg(all(target_os = "windows", target_arch = "x86"))]
const IDENTITY_READ_WINDOW: Duration = Duration::from_millis(1_500);
#[cfg(all(target_os = "windows", target_arch = "x86"))]
const OEM_CANDIDATE_READ_WINDOW: Duration = Duration::from_millis(2_500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadBatch {
    Messages { count: usize, stop_after: bool },
    Retry,
    Stop,
    ApiError(u32),
}

fn classify_read_batch(
    status: u32,
    count: u32,
    maximum: usize,
) -> Result<ReadBatch, WorkerErrorKind> {
    if count as usize > maximum {
        return Err(WorkerErrorKind::MalformedResponse);
    }
    match status {
        0 if count > 0 => Ok(ReadBatch::Messages {
            count: count as usize,
            stop_after: false,
        }),
        0 => Err(WorkerErrorKind::MalformedResponse),
        ERR_TIMEOUT if count > 0 => Ok(ReadBatch::Messages {
            count: count as usize,
            stop_after: true,
        }),
        ERR_TIMEOUT => Ok(ReadBatch::Stop),
        ERR_BUFFER_EMPTY if count == 0 => Ok(ReadBatch::Retry),
        ERR_BUFFER_EMPTY => Err(WorkerErrorKind::MalformedResponse),
        error => Ok(ReadBatch::ApiError(error)),
    }
}

fn no_response_diagnostic(pid: u8, elapsed_ms: u128) -> String {
    format!("No response for standard Mode 01 PID {pid:02X} after {elapsed_ms} ms")
}

fn no_response_mode09_diagnostic(pid: u8, elapsed_ms: u128) -> String {
    format!("No response for standard Mode 09 PID {pid:02X} after {elapsed_ms} ms")
}

#[repr(C)]
#[derive(Clone)]
pub(crate) struct J2534Message {
    pub protocol_id: u32,
    pub rx_status: u32,
    pub tx_flags: u32,
    pub timestamp: u32,
    pub data_size: u32,
    pub extra_data_index: u32,
    pub data: [u8; MESSAGE_DATA_SIZE],
}

impl Default for J2534Message {
    fn default() -> Self {
        Self {
            protocol_id: 0,
            rx_status: 0,
            tx_flags: 0,
            timestamp: 0,
            data_size: 0,
            extra_data_index: 0,
            data: [0; MESSAGE_DATA_SIZE],
        }
    }
}

struct J2534Filter {
    filter_type: u32,
    mask: J2534Message,
    pattern: J2534Message,
    flow_control: J2534Message,
}

fn build_obd_flow_control_filters() -> Vec<J2534Filter> {
    (0..8)
        .map(|index| {
            let mut mask = J2534Message::default();
            mask.protocol_id = ISO15765_PROTOCOL_ID;
            mask.tx_flags = ISO15765_FRAME_PAD;
            mask.data_size = 4;
            mask.data[..4].copy_from_slice(&[0x00, 0x00, 0x07, 0xFF]);

            let mut pattern = J2534Message::default();
            pattern.protocol_id = ISO15765_PROTOCOL_ID;
            pattern.tx_flags = ISO15765_FRAME_PAD;
            pattern.data_size = 4;
            pattern.data[..4].copy_from_slice(&(0x7E8_u32 + index).to_be_bytes());

            let mut flow_control = J2534Message::default();
            flow_control.protocol_id = ISO15765_PROTOCOL_ID;
            flow_control.tx_flags = ISO15765_FRAME_PAD;
            flow_control.data_size = 4;
            flow_control.data[..4].copy_from_slice(&(0x7E0_u32 + index).to_be_bytes());

            J2534Filter {
                filter_type: FLOW_CONTROL_FILTER,
                mask,
                pattern,
                flow_control,
            }
        })
        .collect()
}

fn is_standard_obd_flow_control_route(request_id: u16, response_id: u16) -> bool {
    (0x7E0..=0x7E7).contains(&request_id) && response_id == request_id + 8
}

fn build_oem_flow_control_filter(request_id: u16, response_id: u16) -> J2534Filter {
    let mut mask = J2534Message::default();
    mask.protocol_id = ISO15765_PROTOCOL_ID;
    mask.tx_flags = ISO15765_FRAME_PAD;
    mask.data_size = 4;
    mask.data[..4].copy_from_slice(&0x7FF_u32.to_be_bytes());

    let mut pattern = J2534Message::default();
    pattern.protocol_id = ISO15765_PROTOCOL_ID;
    pattern.tx_flags = ISO15765_FRAME_PAD;
    pattern.data_size = 4;
    pattern.data[..4].copy_from_slice(&u32::from(response_id).to_be_bytes());

    let mut flow_control = J2534Message::default();
    flow_control.protocol_id = ISO15765_PROTOCOL_ID;
    flow_control.tx_flags = ISO15765_FRAME_PAD;
    flow_control.data_size = 4;
    flow_control.data[..4].copy_from_slice(&u32::from(request_id).to_be_bytes());

    J2534Filter {
        filter_type: FLOW_CONTROL_FILTER,
        mask,
        pattern,
        flow_control,
    }
}

fn oem_candidate_request_message(
    request_id: u16,
    request: OemCandidateRequestKind,
) -> J2534Message {
    let payload = match request {
        OemCandidateRequestKind::Mode21 { identifier } => vec![0x21, identifier],
        OemCandidateRequestKind::Mode22 { did } => {
            vec![0x22, (did >> 8) as u8, did as u8]
        }
    };
    let mut message = J2534Message::default();
    message.protocol_id = ISO15765_PROTOCOL_ID;
    message.tx_flags = ISO15765_FRAME_PAD;
    message.data_size = (4 + payload.len()) as u32;
    message.data[..4].copy_from_slice(&u32::from(request_id).to_be_bytes());
    message.data[4..4 + payload.len()].copy_from_slice(&payload);
    message
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct J2534Failure {
    pub kind: WorkerErrorKind,
    pub message: String,
}

impl J2534Failure {
    pub(crate) fn new(kind: WorkerErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

fn retain_mode09_failure(
    mut responses: Vec<Mode09Response>,
    failure: J2534Failure,
    latency_us: u64,
) -> Result<Vec<Mode09Response>, J2534Failure> {
    if responses.is_empty() {
        return Err(failure);
    }
    responses.push(Mode09Response::Failure(Mode09Failure {
        kind: failure.kind,
        message: failure.message,
        latency_us,
    }));
    Ok(responses)
}

fn retain_standard_dtc_failure(
    mut responses: Vec<StandardDtcResponse>,
    failure: J2534Failure,
    latency_us: u64,
) -> Result<Vec<StandardDtcResponse>, J2534Failure> {
    if responses.is_empty() {
        return Err(failure);
    }
    responses.push(StandardDtcResponse::Failure(StandardDtcFailure {
        kind: failure.kind,
        message: failure.message,
        latency_us,
    }));
    Ok(responses)
}

impl fmt::Display for J2534Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}: {}", self.kind, self.message)
    }
}

pub(crate) trait PassThruApi {
    fn open_device(&mut self) -> Result<u32, J2534Failure>;
    fn read_version(&mut self, device: u32) -> Result<J2534Identity, J2534Failure>;
    fn connect_iso15765(&mut self, device: u32) -> Result<u32, J2534Failure>;
    fn connect_subaru_ssm(&mut self, _device: u32) -> Result<u32, J2534Failure> {
        Err(J2534Failure::new(
            WorkerErrorKind::ChannelFailed,
            "J2534 provider does not support the ISO9141 K-Line channel",
        ))
    }
    fn start_obd_filter(&mut self, channel: u32) -> Result<(), J2534Failure>;
    fn start_subaru_ssm_filter(&mut self, _channel: u32) -> Result<(), J2534Failure> {
        Err(J2534Failure::new(
            WorkerErrorKind::ChannelFailed,
            "J2534 provider does not support Subaru SSM K-Line filtering",
        ))
    }
    fn exchange_subaru_ssm(
        &mut self,
        _channel: u32,
        _request_frame: &[u8],
        _ecu_address: u8,
        _tool_address: u8,
        _timeout_ms: u32,
    ) -> Result<SubaruSsmExchange, J2534Failure> {
        Err(J2534Failure::new(
            WorkerErrorKind::ChannelFailed,
            "J2534 provider does not support Subaru SSM K-Line reads",
        ))
    }
    fn read_mode01(
        &mut self,
        channel: u32,
        pid: u8,
        timeout_ms: u32,
        expected_ecu: Option<u16>,
    ) -> Result<Vec<Mode01Payload>, J2534Failure>;
    fn read_mode06(
        &mut self,
        channel: u32,
        test_id: u8,
        timeout_ms: u32,
    ) -> Result<Vec<Mode09Response>, J2534Failure>;
    fn read_mode09(
        &mut self,
        channel: u32,
        pid: u8,
        timeout_ms: u32,
    ) -> Result<Vec<Mode09Response>, J2534Failure>;
    fn read_standard_dtc(
        &mut self,
        channel: u32,
        service: StandardDtcService,
        timeout_ms: u32,
    ) -> Result<Vec<StandardDtcResponse>, J2534Failure>;
    fn read_oem_candidate(
        &mut self,
        channel: u32,
        request_id: u16,
        response_id: u16,
        request: OemCandidateRequestKind,
        timeout_ms: u32,
    ) -> Result<Vec<OemCandidateResponse>, J2534Failure>;
    fn disconnect(&mut self, channel: u32) -> Result<(), J2534Failure>;
    fn close_device(&mut self, device: u32) -> Result<(), J2534Failure>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SubaruSsmExchange {
    pub response_frame: Vec<u8>,
    pub latency_us: u64,
}

pub(crate) fn decode_mode01_message(
    message: &J2534Message,
    requested_pid: u8,
    latency_us: u64,
) -> Result<Option<Mode01Payload>, WorkerErrorKind> {
    if message.rx_status & (RX_STATUS_TX_MSG_TYPE | RX_STATUS_START_OF_MESSAGE) != 0 {
        return Ok(None);
    }
    if message.rx_status
        & (RX_STATUS_RX_BREAK | RX_STATUS_ISO15765_ADDR_TYPE | RX_STATUS_CAN_29BIT_ID)
        != 0
    {
        return Err(WorkerErrorKind::MalformedResponse);
    }
    let length = message.data_size as usize;
    if message.protocol_id != ISO15765_PROTOCOL_ID
        || !(7..=10).contains(&length)
        || length > MESSAGE_DATA_SIZE
    {
        return Err(WorkerErrorKind::MalformedResponse);
    }

    let can_id = u32::from_be_bytes(message.data[..4].try_into().expect("four bytes checked"));
    if !(0x7E8..=0x7EF).contains(&can_id) {
        return Err(WorkerErrorKind::MalformedResponse);
    }

    let payload = &message.data[4..length];
    match payload.first() {
        Some(0x41) if payload.get(1) == Some(&requested_pid) && payload.len() <= 6 => {}
        Some(0x7F) if payload.get(1) == Some(&0x01) && payload.len() == 3 => {}
        _ => return Err(WorkerErrorKind::MalformedResponse),
    }

    Ok(Some(Mode01Payload {
        ecu_id: format!("{can_id:03X}"),
        bytes: payload.to_vec(),
        latency_us,
    }))
}

fn mode09_request_id() -> u32 {
    0x7DF
}

fn standard_dtc_request(service: StandardDtcService) -> J2534Message {
    let mut request = J2534Message::default();
    request.protocol_id = ISO15765_PROTOCOL_ID;
    request.tx_flags = ISO15765_FRAME_PAD;
    request.data_size = 5;
    request.data[..4].copy_from_slice(&mode09_request_id().to_be_bytes());
    request.data[4] = service.request_byte();
    request
}

fn decode_mode09_message(message: &J2534Message, latency_us: u64) -> Option<Mode09Response> {
    if message.rx_status & (RX_STATUS_TX_MSG_TYPE | RX_STATUS_START_OF_MESSAGE) != 0 {
        return None;
    }
    let length = message.data_size as usize;
    let bad_flags = message.rx_status
        & (RX_STATUS_RX_BREAK | RX_STATUS_ISO15765_ADDR_TYPE | RX_STATUS_CAN_29BIT_ID)
        != 0;
    let responder = (message.protocol_id == ISO15765_PROTOCOL_ID
        && !bad_flags
        && length.min(MESSAGE_DATA_SIZE) >= 4)
        .then(|| u32::from_be_bytes(message.data[..4].try_into().expect("four bytes checked")))
        .filter(|can_id| (0x7E8..=0x7EF).contains(can_id));
    let malformed_reason = if message.protocol_id != ISO15765_PROTOCOL_ID {
        Some("message is not ISO15765")
    } else if bad_flags {
        Some("message has unsupported addressing or receive flags")
    } else if length > MESSAGE_DATA_SIZE {
        Some("declared data size exceeds the J2534 message buffer")
    } else if length < 5 {
        Some("message does not contain a CAN header and response byte")
    } else if responder.is_none() {
        Some("message has an invalid standard responder CAN ID")
    } else {
        None
    };
    if let Some(reason) = malformed_reason {
        let raw_length = length.min(MESSAGE_DATA_SIZE);
        return Some(Mode09Response::MalformedFrame(Mode09MalformedFrame {
            ecu_id: responder.map(|can_id| format!("{can_id:03X}")),
            raw_bytes: message.data[..raw_length].to_vec(),
            latency_us,
            protocol_id: message.protocol_id,
            rx_status: message.rx_status,
            declared_data_size: message.data_size,
            detail: format!(
                "Malformed J2534 Mode 09 frame: {reason} (ProtocolID=0x{:08X}, RxStatus=0x{:08X}, DataSize={})",
                message.protocol_id, message.rx_status, message.data_size
            ),
        }));
    }

    Some(Mode09Response::Payload(Mode09Payload {
        ecu_id: format!("{:03X}", responder.expect("valid responder was checked")),
        bytes: message.data[4..length].to_vec(),
        latency_us,
    }))
}

fn decode_standard_dtc_message(
    message: &J2534Message,
    service: StandardDtcService,
    latency_us: u64,
) -> Option<StandardDtcResponse> {
    if message.rx_status & (RX_STATUS_TX_MSG_TYPE | RX_STATUS_START_OF_MESSAGE) != 0 {
        return None;
    }
    let length = message.data_size as usize;
    let bad_flags = message.rx_status
        & (RX_STATUS_RX_BREAK | RX_STATUS_ISO15765_ADDR_TYPE | RX_STATUS_CAN_29BIT_ID)
        != 0;
    let responder = (message.protocol_id == ISO15765_PROTOCOL_ID
        && !bad_flags
        && length.min(MESSAGE_DATA_SIZE) >= 4)
        .then(|| u32::from_be_bytes(message.data[..4].try_into().expect("four bytes checked")))
        .filter(|can_id| (0x7E8..=0x7EF).contains(can_id));
    let payload = responder.map(|_| &message.data[4..length.min(MESSAGE_DATA_SIZE)]);
    let expected_positive = service.positive_response_byte();
    let expected_request = service.request_byte();
    let malformed_reason = if message.protocol_id != ISO15765_PROTOCOL_ID {
        Some("message is not ISO15765")
    } else if bad_flags {
        Some("message has unsupported addressing or receive flags")
    } else if length > MESSAGE_DATA_SIZE {
        Some("declared data size exceeds the J2534 message buffer")
    } else if length < 5 {
        Some("message does not contain a CAN header and response byte")
    } else if responder.is_none() {
        Some("message has an invalid standard responder CAN ID")
    } else if !matches!(payload.and_then(|data| data.first()), Some(byte) if *byte == expected_positive || *byte == 0x7F)
    {
        Some("message has an unexpected diagnostic response service")
    } else if let Some(data) = payload {
        match data.first() {
            Some(byte) if *byte == expected_positive => None,
            Some(0x7F) if data.len() == 3 && data[1] == expected_request => None,
            Some(0x7F) => Some("negative response does not echo the requested service"),
            _ => Some("message has an unexpected diagnostic response service"),
        }
    } else {
        Some("message has no responder payload")
    };
    if let Some(reason) = malformed_reason {
        let raw_length = length.min(MESSAGE_DATA_SIZE);
        return Some(StandardDtcResponse::MalformedFrame(
            StandardDtcMalformedFrame {
                ecu_id: responder.map(|can_id| format!("{can_id:03X}")),
                raw_bytes: message.data[..raw_length].to_vec(),
                latency_us,
                protocol_id: message.protocol_id,
                rx_status: message.rx_status,
                declared_data_size: message.data_size,
                detail: format!(
                    "Malformed J2534 Mode {:02X} DTC response: {reason} (ProtocolID=0x{:08X}, RxStatus=0x{:08X}, DataSize={})",
                    expected_request, message.protocol_id, message.rx_status, message.data_size
                ),
            },
        ));
    }

    Some(StandardDtcResponse::Payload(StandardDtcPayload {
        ecu_id: format!("{:03X}", responder.expect("valid responder was checked")),
        bytes: payload.expect("responder payload was checked").to_vec(),
        latency_us,
    }))
}

fn decode_oem_candidate_message(
    message: &J2534Message,
    latency_us: u64,
) -> Option<OemCandidateResponse> {
    if message.rx_status & (RX_STATUS_TX_MSG_TYPE | RX_STATUS_START_OF_MESSAGE) != 0 {
        return None;
    }
    let length = message.data_size as usize;
    let raw_length = length.min(MESSAGE_DATA_SIZE);
    let bad_flags = message.rx_status
        & (RX_STATUS_RX_BREAK | RX_STATUS_ISO15765_ADDR_TYPE | RX_STATUS_CAN_29BIT_ID)
        != 0;
    let can_id = (raw_length >= 4)
        .then(|| u32::from_be_bytes(message.data[..4].try_into().expect("four bytes checked")))
        .filter(|can_id| *can_id <= 0x7FF)
        .map(|can_id| can_id as u16);
    let malformed_reason = if message.protocol_id != ISO15765_PROTOCOL_ID {
        Some("message is not ISO15765")
    } else if bad_flags {
        Some("message has unsupported addressing or receive flags")
    } else if length > MESSAGE_DATA_SIZE {
        Some("declared data size exceeds the J2534 message buffer")
    } else if length < 5 {
        Some("message does not contain a CAN header and diagnostic bytes")
    } else if can_id.is_none() {
        Some("message has an invalid 11-bit CAN responder ID")
    } else {
        None
    };
    if let Some(reason) = malformed_reason {
        return Some(OemCandidateResponse::MalformedFrame(
            OemCandidateMalformedFrame {
                response_id: can_id,
                raw_bytes: message.data[..raw_length].to_vec(),
                latency_us,
                protocol_id: message.protocol_id,
                rx_status: message.rx_status,
                declared_data_size: message.data_size,
                detail: format!(
                    "Malformed J2534 OEM candidate frame: {reason} (ProtocolID=0x{:08X}, RxStatus=0x{:08X}, DataSize={})",
                    message.protocol_id, message.rx_status, message.data_size
                ),
            },
        ));
    }

    Some(OemCandidateResponse::Payload(OemCandidatePayload {
        response_id: can_id.expect("responder ID was validated"),
        bytes: message.data[4..length].to_vec(),
        latency_us,
    }))
}

#[cfg(all(target_os = "windows", target_arch = "x86"))]
fn oem_candidate_failure_response(
    request_id: u16,
    response_id: u16,
    kind: WorkerErrorKind,
    message: impl Into<String>,
    latency_us: u64,
) -> OemCandidateResponse {
    OemCandidateResponse::Failure(OemCandidateFailure {
        request_id,
        expected_response_id: response_id,
        kind,
        message: message.into(),
        latency_us,
    })
}

#[derive(Default)]
struct OemCandidateResponseState {
    pending: Option<OemCandidateResponse>,
}

impl OemCandidateResponseState {
    fn accept(
        &mut self,
        response: OemCandidateResponse,
        expected_response_id: u16,
        request: &OemCandidateRequestKind,
    ) -> Option<OemCandidateResponse> {
        if !matches_oem_candidate_response(&response, expected_response_id, request) {
            return None;
        }
        if is_oem_candidate_response_pending(&response, request) {
            self.pending = Some(response);
            None
        } else {
            Some(response)
        }
    }

    fn finish(&mut self, terminal: OemCandidateResponse) -> Vec<OemCandidateResponse> {
        let mut responses = self.pending.take().into_iter().collect::<Vec<_>>();
        responses.push(terminal);
        responses
    }
}

fn matches_oem_candidate_response(
    response: &OemCandidateResponse,
    expected_response_id: u16,
    request: &OemCandidateRequestKind,
) -> bool {
    let (service, positive_prefix) = match request {
        OemCandidateRequestKind::Mode21 { identifier } => (0x21, vec![0x61, *identifier]),
        OemCandidateRequestKind::Mode22 { did } => {
            (0x22, vec![0x62, (*did >> 8) as u8, *did as u8])
        }
    };
    match response {
        OemCandidateResponse::Payload(payload) if payload.response_id == expected_response_id => {
            match payload.bytes.first() {
                Some(byte) if *byte == positive_prefix[0] => {
                    payload.bytes.len() < positive_prefix.len()
                        || payload.bytes.starts_with(&positive_prefix)
                }
                Some(0x7F) => payload.bytes.get(1).is_none_or(|value| *value == service),
                _ => false,
            }
        }
        OemCandidateResponse::MalformedFrame(frame) => {
            frame.response_id == Some(expected_response_id)
        }
        _ => false,
    }
}

fn is_oem_candidate_response_pending(
    response: &OemCandidateResponse,
    request: &OemCandidateRequestKind,
) -> bool {
    let service = match request {
        OemCandidateRequestKind::Mode21 { .. } => 0x21,
        OemCandidateRequestKind::Mode22 { .. } => 0x22,
    };
    matches!(
        response,
        OemCandidateResponse::Payload(payload)
            if payload.bytes == [0x7F, service, 0x78]
    )
}

fn decode_standard_dtc_batch(
    messages: &[J2534Message],
    service: StandardDtcService,
    latency_us: u64,
) -> Vec<StandardDtcResponse> {
    messages
        .iter()
        .filter_map(|message| decode_standard_dtc_message(message, service, latency_us))
        .collect()
}

fn decode_standard_dtc_api_error_messages(
    messages: &[J2534Message],
    count: u32,
    service: StandardDtcService,
    latency_us: u64,
) -> Vec<StandardDtcResponse> {
    let available = messages
        .iter()
        .take(count as usize)
        .filter(|message| j2534_message_has_data(message))
        .cloned()
        .collect::<Vec<_>>();
    decode_standard_dtc_batch(&available, service, latency_us)
}

fn timeout_left_messages_unfilled(status: u32, messages: &[J2534Message]) -> bool {
    status == ERR_TIMEOUT
        && messages.iter().all(|message| {
            message.protocol_id == 0
                && message.rx_status == 0
                && message.tx_flags == 0
                && message.timestamp == 0
                && message.data_size == 0
                && message.extra_data_index == 0
                && message.data.iter().all(|byte| *byte == 0)
        })
}

fn j2534_message_has_data(message: &J2534Message) -> bool {
    message.protocol_id != 0
        || message.rx_status != 0
        || message.tx_flags != 0
        || message.timestamp != 0
        || message.data_size != 0
        || message.extra_data_index != 0
        || message.data.iter().any(|byte| *byte != 0)
}

#[derive(Debug)]
struct Mode01DecodeFailure {
    kind: WorkerErrorKind,
    diagnostic: String,
}

fn malformed_mode01_diagnostic(message: &J2534Message, requested_pid: u8) -> String {
    let length = (message.data_size as usize).min(message.data.len()).min(16);
    let prefix = message.data[..length]
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "J2534 returned an invalid ISO15765 OBD response for PID {requested_pid:02X} (RxStatus=0x{:08X}, ProtocolID=0x{:08X}, DataSize={}, DataPrefix=[{prefix}])",
        message.rx_status, message.protocol_id, message.data_size
    )
}

fn decode_mode01_batch(
    messages: &[J2534Message],
    requested_pid: u8,
    latency_us: u64,
) -> Result<Vec<Mode01Payload>, Mode01DecodeFailure> {
    let mut payloads = Vec::new();
    for message in messages {
        match decode_mode01_message(message, requested_pid, latency_us) {
            Ok(Some(payload)) => payloads.push(payload),
            Ok(None) => {}
            Err(kind) => {
                return Err(Mode01DecodeFailure {
                    kind,
                    diagnostic: malformed_mode01_diagnostic(message, requested_pid),
                });
            }
        }
    }
    Ok(payloads)
}

fn filter_standard_pid_responses(
    responses: Vec<Mode09Response>,
    service: u8,
    identifier: u8,
) -> Vec<Mode09Response> {
    responses
        .into_iter()
        .filter_map(|response| match response {
            Mode09Response::Payload(payload) => {
                let matches = payload.bytes.starts_with(&[service + 0x40, identifier])
                    || (payload.bytes.len() >= 3 && payload.bytes.starts_with(&[0x7F, service]));
                matches.then_some(Mode09Response::Payload(payload))
            }
            Mode09Response::MalformedFrame(mut frame) => {
                frame.detail = frame
                    .detail
                    .replace("Mode 09", &format!("Mode {service:02X}"));
                Some(Mode09Response::MalformedFrame(frame))
            }
            failure => Some(failure),
        })
        .collect()
}

fn decode_mode09_batch(messages: &[J2534Message], latency_us: u64) -> Vec<Mode09Response> {
    messages
        .iter()
        .filter_map(|message| decode_mode09_message(message, latency_us))
        .collect()
}

fn decode_mode09_api_error_messages(
    messages: &[J2534Message],
    count: u32,
    latency_us: u64,
) -> Vec<Mode09Response> {
    let available = messages
        .iter()
        .take(count as usize)
        .filter(|message| j2534_message_has_data(message))
        .cloned()
        .collect::<Vec<_>>();
    decode_mode09_batch(&available, latency_us)
}

fn mode01_request_id(expected_ecu: Option<u16>) -> Result<u32, J2534Failure> {
    match expected_ecu {
        None => Ok(0x7DF),
        Some(ecu @ 0x7E8..=0x7EF) => Ok(u32::from(ecu - 8)),
        Some(_) => Err(J2534Failure::new(
            WorkerErrorKind::MalformedResponse,
            "Invalid standard OBD ECU route",
        )),
    }
}

fn matching_physical_reply(expected_ecu: Option<u16>, replies: &[Mode01Payload]) -> bool {
    expected_ecu.is_some_and(|ecu| {
        replies
            .iter()
            .any(|reply| u16::from_str_radix(&reply.ecu_id, 16).ok() == Some(ecu))
    })
}

#[cfg(all(target_os = "windows", target_arch = "x86"))]
mod dynamic {
    use super::{
        ERR_BUFFER_EMPTY, ERR_TIMEOUT, IDENTITY_READ_WINDOW, ISO15765_FRAME_PAD,
        ISO15765_PROTOCOL_ID, ISO9141_NO_CHECKSUM, ISO9141_PROTOCOL_ID, J2534Failure,
        J2534Message, PASS_FILTER,
        MAX_READ_MESSAGES, OEM_CANDIDATE_READ_WINDOW, OemCandidateResponseState, PassThruApi,
        READ_WINDOW, ReadBatch, build_obd_flow_control_filters, build_oem_flow_control_filter,
        classify_read_batch, decode_mode01_batch, decode_mode09_api_error_messages,
        decode_mode09_batch, decode_oem_candidate_message, decode_standard_dtc_api_error_messages,
        decode_standard_dtc_batch, filter_standard_pid_responses,
        is_standard_obd_flow_control_route, j2534_message_has_data, matching_physical_reply,
        mode01_request_id, mode09_request_id, no_response_diagnostic,
        no_response_mode09_diagnostic, oem_candidate_failure_response,
        oem_candidate_request_message, retain_standard_dtc_failure, standard_dtc_request,
        SubaruSsmExchange, timeout_left_messages_unfilled,
    };
    use lognook_j2534_protocol::{
        J2534Identity, Mode01Payload, Mode09Response, OemCandidateRequestKind,
        OemCandidateResponse, StandardDtcResponse, StandardDtcService, WorkerErrorKind,
        is_supported_mode09_pid,
    };
    use lognook_j2534_protocol::subaru_ssm::parse_ssm2_frame;
    use std::{
        ffi::{OsStr, c_char, c_void},
        os::windows::ffi::OsStrExt,
        path::Path,
        ptr, thread,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::{FARPROC, FreeLibrary, GetLastError, HMODULE},
        System::LibraryLoader::{GetProcAddress, LoadLibraryW},
    };

    type OpenFn = unsafe extern "system" fn(*const c_void, *mut u32) -> u32;
    type ReadVersionFn =
        unsafe extern "system" fn(u32, *mut c_char, *mut c_char, *mut c_char) -> u32;
    type CloseFn = unsafe extern "system" fn(u32) -> u32;
    type ConnectFn = unsafe extern "system" fn(u32, u32, u32, u32, *mut u32) -> u32;
    type DisconnectFn = unsafe extern "system" fn(u32) -> u32;
    type StartFilterFn = unsafe extern "system" fn(
        u32,
        u32,
        *mut J2534Message,
        *mut J2534Message,
        *mut J2534Message,
        *mut u32,
    ) -> u32;
    type StopFilterFn = unsafe extern "system" fn(u32, u32) -> u32;
    type ReadMessagesFn = unsafe extern "system" fn(u32, *mut J2534Message, *mut u32, u32) -> u32;
    type WriteMessagesFn = unsafe extern "system" fn(u32, *mut J2534Message, *mut u32, u32) -> u32;
    type GetLastErrorFn = unsafe extern "system" fn(*mut c_char) -> u32;

    pub(crate) struct DynamicJ2534 {
        module: HMODULE,
        open: OpenFn,
        read_version: ReadVersionFn,
        close: CloseFn,
        connect: ConnectFn,
        disconnect: DisconnectFn,
        start_filter: StartFilterFn,
        stop_filter: StopFilterFn,
        read_messages: ReadMessagesFn,
        write_messages: WriteMessagesFn,
        get_last_error: Option<GetLastErrorFn>,
    }

    impl DynamicJ2534 {
        pub(crate) fn load(path: &Path) -> Result<Self, J2534Failure> {
            let wide_path: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
            let module = unsafe { LoadLibraryW(wide_path.as_ptr()) };
            if module.is_null() {
                return Err(J2534Failure::new(
                    WorkerErrorKind::DllNotFound,
                    format!(
                        "Could not load registered provider DLL (Windows error {})",
                        unsafe { GetLastError() }
                    ),
                ));
            }

            let functions = unsafe {
                Ok(Self {
                    module,
                    open: load_function(module, b"PassThruOpen\0")?,
                    read_version: load_function(module, b"PassThruReadVersion\0")?,
                    close: load_function(module, b"PassThruClose\0")?,
                    connect: load_function(module, b"PassThruConnect\0")?,
                    disconnect: load_function(module, b"PassThruDisconnect\0")?,
                    start_filter: load_function(module, b"PassThruStartMsgFilter\0")?,
                    stop_filter: load_function(module, b"PassThruStopMsgFilter\0")?,
                    read_messages: load_function(module, b"PassThruReadMsgs\0")?,
                    write_messages: load_function(module, b"PassThruWriteMsgs\0")?,
                    get_last_error: load_optional_function(module, b"PassThruGetLastError\0"),
                })
            };
            match functions {
                Ok(api) => Ok(api),
                Err(error) => {
                    unsafe {
                        FreeLibrary(module);
                    }
                    Err(error)
                }
            }
        }

        fn call_error(&self, kind: WorkerErrorKind, function: &str, status: u32) -> J2534Failure {
            let mut detail = [0i8; 512];
            let message = if let Some(get_last_error) = self.get_last_error {
                let error_status = unsafe { get_last_error(detail.as_mut_ptr()) };
                if error_status == 0 {
                    nul_terminated(&detail)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let detail = if message.is_empty() {
                String::new()
            } else {
                format!(" ({message})")
            };
            J2534Failure::new(kind, format!("{function} returned 0x{status:08X}{detail}"))
        }

        fn check(&self, function: &str, status: u32) -> Result<(), J2534Failure> {
            self.check_with_kind(WorkerErrorKind::PassThruFailed, function, status)
        }

        fn check_with_kind(
            &self,
            kind: WorkerErrorKind,
            function: &str,
            status: u32,
        ) -> Result<(), J2534Failure> {
            if status == 0 {
                Ok(())
            } else {
                Err(self.call_error(kind, function, status))
            }
        }

        fn start_oem_flow_control_filter(
            &self,
            channel: u32,
            request_id: u16,
            response_id: u16,
        ) -> Result<u32, J2534Failure> {
            let mut filter = build_oem_flow_control_filter(request_id, response_id);
            let mut filter_id = 0;
            let status = unsafe {
                (self.start_filter)(
                    channel,
                    filter.filter_type,
                    &mut filter.mask,
                    &mut filter.pattern,
                    &mut filter.flow_control,
                    &mut filter_id,
                )
            };
            self.check_with_kind(
                WorkerErrorKind::ChannelFailed,
                "PassThruStartMsgFilter(OEM route)",
                status,
            )?;
            Ok(filter_id)
        }

        fn read_oem_candidate_exchange(
            &mut self,
            channel: u32,
            request_id: u16,
            response_id: u16,
            request: OemCandidateRequestKind,
            timeout_ms: u32,
        ) -> Result<Vec<OemCandidateResponse>, J2534Failure> {
            if request_id > 0x7FF || response_id > 0x7FF || request_id == response_id {
                return Err(J2534Failure::new(
                    WorkerErrorKind::MalformedResponse,
                    "OEM candidate requires distinct exact 11-bit request and response IDs",
                ));
            }
            let started = Instant::now();
            let mut request_message = oem_candidate_request_message(request_id, request.clone());
            let mut written = 1;
            let status = unsafe {
                (self.write_messages)(channel, &mut request_message, &mut written, timeout_ms)
            };
            self.check("PassThruWriteMsgs(OEM candidate)", status)?;
            if written != 1 {
                return Err(J2534Failure::new(
                    WorkerErrorKind::PassThruFailed,
                    "PassThruWriteMsgs did not accept the single source-backed OEM read",
                ));
            }

            let deadline =
                started + OEM_CANDIDATE_READ_WINDOW.min(Duration::from_millis(timeout_ms as u64));
            let mut response_state = OemCandidateResponseState::default();
            loop {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                let remaining_ms = (deadline - now).as_millis().clamp(1, u32::MAX as u128) as u32;
                let mut messages = vec![J2534Message::default(); MAX_READ_MESSAGES];
                let mut count = MAX_READ_MESSAGES as u32;
                let status = unsafe {
                    (self.read_messages)(channel, messages.as_mut_ptr(), &mut count, remaining_ms)
                };
                if count as usize <= MAX_READ_MESSAGES
                    && timeout_left_messages_unfilled(status, &messages[..count as usize])
                {
                    break;
                }
                let batch = match classify_read_batch(status, count, MAX_READ_MESSAGES) {
                    Ok(batch) => batch,
                    Err(kind) => {
                        let latency_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
                        let failure = oem_candidate_failure_response(
                            request_id,
                            response_id,
                            kind,
                            "PassThruReadMsgs returned an invalid OEM candidate message count",
                            latency_us,
                        );
                        let mut outcomes = response_state.pending.into_iter().collect::<Vec<_>>();
                        outcomes.push(failure);
                        return Ok(outcomes);
                    }
                };
                let batch_count = match batch {
                    ReadBatch::Messages { count, .. } => count,
                    ReadBatch::Retry => {
                        if Instant::now() >= deadline {
                            break;
                        }
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    ReadBatch::Stop => break,
                    ReadBatch::ApiError(status) => {
                        if let Some(response) = messages
                            .first()
                            .filter(|message| j2534_message_has_data(message))
                            .and_then(|message| {
                                decode_oem_candidate_message(
                                    message,
                                    started.elapsed().as_micros().min(u64::MAX as u128) as u64,
                                )
                            })
                        {
                            if let Some(response) =
                                response_state.accept(response, response_id, &request)
                            {
                                return Ok(response_state.finish(response));
                            }
                        }
                        let failure = self
                            .check("PassThruReadMsgs(OEM candidate)", status)
                            .expect_err("ApiError always carries a failed J2534 status");
                        let failure = oem_candidate_failure_response(
                            request_id,
                            response_id,
                            failure.kind,
                            failure.message,
                            started.elapsed().as_micros().min(u64::MAX as u128) as u64,
                        );
                        let mut outcomes = response_state.pending.into_iter().collect::<Vec<_>>();
                        outcomes.push(failure);
                        return Ok(outcomes);
                    }
                };
                if let Some(response) = messages[..batch_count].iter().find_map(|message| {
                    decode_oem_candidate_message(
                        message,
                        started.elapsed().as_micros().min(u64::MAX as u128) as u64,
                    )
                }) {
                    if let Some(response) = response_state.accept(response, response_id, &request) {
                        return Ok(response_state.finish(response));
                    }
                }
            }
            if let Some(pending) = response_state.pending {
                return Ok(vec![pending]);
            }
            Ok(vec![oem_candidate_failure_response(
                request_id,
                response_id,
                WorkerErrorKind::NoResponse,
                format!(
                    "No response from OEM candidate route {response_id:03X} after {} ms",
                    started.elapsed().as_millis()
                ),
                started.elapsed().as_micros().min(u64::MAX as u128) as u64,
            )])
        }
    }

    impl DynamicJ2534 {
        fn read_standard_pid(
            &mut self,
            channel: u32,
            service: u8,
            pid: u8,
            timeout_ms: u32,
        ) -> Result<Vec<Mode09Response>, J2534Failure> {
            let started = Instant::now();
            let mut request = J2534Message::default();
            request.protocol_id = ISO15765_PROTOCOL_ID;
            request.tx_flags = ISO15765_FRAME_PAD;
            request.data_size = 6;
            request.data[..4].copy_from_slice(&mode09_request_id().to_be_bytes());
            request.data[4..6].copy_from_slice(&[service, pid]);
            let mut written = 1;
            let status =
                unsafe { (self.write_messages)(channel, &mut request, &mut written, timeout_ms) };
            self.check("PassThruWriteMsgs(standard PID)", status)?;
            if written != 1 {
                return Err(J2534Failure::new(
                    WorkerErrorKind::PassThruFailed,
                    "PassThruWriteMsgs did not accept the single fixed standard read request",
                ));
            }

            let deadline =
                started + IDENTITY_READ_WINDOW.min(Duration::from_millis(timeout_ms as u64));
            let mut first_latency = None;
            let mut responses = Vec::new();
            let mut read_failure = None;
            loop {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                let remaining_ms = (deadline - now).as_millis().clamp(1, u32::MAX as u128) as u32;
                let mut messages = vec![J2534Message::default(); MAX_READ_MESSAGES];
                let mut count = MAX_READ_MESSAGES as u32;
                let status = unsafe {
                    (self.read_messages)(channel, messages.as_mut_ptr(), &mut count, remaining_ms)
                };
                if count as usize <= MAX_READ_MESSAGES
                    && timeout_left_messages_unfilled(status, &messages[..count as usize])
                {
                    break;
                }
                let batch = match classify_read_batch(status, count, MAX_READ_MESSAGES) {
                    Ok(batch) => batch,
                    Err(kind) => {
                        let latency_us = *first_latency.get_or_insert_with(|| {
                            started.elapsed().as_micros().min(u64::MAX as u128) as u64
                        });
                        responses.extend(filter_standard_pid_responses(
                            decode_mode09_api_error_messages(&messages, count, latency_us),
                            service,
                            pid,
                        ));
                        read_failure = Some(J2534Failure::new(
                            kind,
                            "PassThruReadMsgs returned an invalid standard read message count",
                        ));
                        break;
                    }
                };
                let (batch_count, stop_after) = match batch {
                    ReadBatch::Messages { count, stop_after } => (count, stop_after),
                    ReadBatch::Retry => {
                        if Instant::now() >= deadline {
                            break;
                        }
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    ReadBatch::Stop => break,
                    ReadBatch::ApiError(status) => {
                        let latency_us = *first_latency.get_or_insert_with(|| {
                            started.elapsed().as_micros().min(u64::MAX as u128) as u64
                        });
                        responses.extend(filter_standard_pid_responses(
                            decode_mode09_api_error_messages(&messages, count, latency_us),
                            service,
                            pid,
                        ));
                        read_failure = Some(
                            self.check("PassThruReadMsgs(standard PID)", status)
                                .expect_err("ApiError always carries a failing J2534 status"),
                        );
                        break;
                    }
                };
                let latency_us = *first_latency.get_or_insert_with(|| {
                    started.elapsed().as_micros().min(u64::MAX as u128) as u64
                });
                responses.extend(filter_standard_pid_responses(
                    decode_mode09_batch(&messages[..batch_count], latency_us),
                    service,
                    pid,
                ));
                if stop_after {
                    break;
                }
            }
            if let Some(error) = read_failure {
                let latency_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
                responses = super::retain_mode09_failure(responses, error, latency_us)?;
            }
            if responses.is_empty() {
                return Err(J2534Failure::new(
                    WorkerErrorKind::NoResponse,
                    if service == 0x09 {
                        no_response_mode09_diagnostic(pid, started.elapsed().as_millis())
                    } else {
                        format!(
                            "No response for standard Mode {service:02X} test {pid:02X} after {} ms",
                            started.elapsed().as_millis()
                        )
                    },
                ));
            }
            Ok(responses)
        }
    }

    impl Drop for DynamicJ2534 {
        fn drop(&mut self) {
            unsafe {
                FreeLibrary(self.module);
            }
        }
    }

    impl PassThruApi for DynamicJ2534 {
        fn open_device(&mut self) -> Result<u32, J2534Failure> {
            let mut device = 0;
            let status = unsafe { (self.open)(ptr::null(), &mut device) };
            self.check_with_kind(WorkerErrorKind::ProviderOpenFailed, "PassThruOpen", status)?;
            Ok(device)
        }

        fn read_version(&mut self, device: u32) -> Result<J2534Identity, J2534Failure> {
            let mut firmware = [0i8; 512];
            let mut dll_version = [0i8; 512];
            let mut api_version = [0i8; 512];
            let status = unsafe {
                (self.read_version)(
                    device,
                    firmware.as_mut_ptr(),
                    dll_version.as_mut_ptr(),
                    api_version.as_mut_ptr(),
                )
            };
            self.check_with_kind(
                WorkerErrorKind::UnsupportedApi,
                "PassThruReadVersion",
                status,
            )?;
            Ok(J2534Identity {
                firmware: nul_terminated(&firmware),
                dll_version: nul_terminated(&dll_version),
                api_version: nul_terminated(&api_version),
            })
        }

        fn connect_iso15765(&mut self, device: u32) -> Result<u32, J2534Failure> {
            let mut channel = 0;
            let status =
                unsafe { (self.connect)(device, ISO15765_PROTOCOL_ID, 0, 500_000, &mut channel) };
            self.check_with_kind(
                WorkerErrorKind::ChannelFailed,
                "PassThruConnect(ISO15765)",
                status,
            )?;
            Ok(channel)
        }

        fn connect_subaru_ssm(&mut self, device: u32) -> Result<u32, J2534Failure> {
            let mut channel = 0;
            let status = unsafe {
                (self.connect)(
                    device,
                    ISO9141_PROTOCOL_ID,
                    ISO9141_NO_CHECKSUM,
                    4_800,
                    &mut channel,
                )
            };
            self.check_with_kind(
                WorkerErrorKind::ChannelFailed,
                "PassThruConnect(ISO9141, 4800)",
                status,
            )?;
            Ok(channel)
        }

        fn start_obd_filter(&mut self, channel: u32) -> Result<(), J2534Failure> {
            for mut filter in build_obd_flow_control_filters() {
                let mut filter_id = 0;
                let status = unsafe {
                    (self.start_filter)(
                        channel,
                        filter.filter_type,
                        &mut filter.mask,
                        &mut filter.pattern,
                        &mut filter.flow_control,
                        &mut filter_id,
                    )
                };
                self.check_with_kind(
                    WorkerErrorKind::ChannelFailed,
                    "PassThruStartMsgFilter(FLOW_CONTROL)",
                    status,
                )?;
            }
            Ok(())
        }

        fn start_subaru_ssm_filter(&mut self, channel: u32) -> Result<(), J2534Failure> {
            let mut mask = J2534Message::default();
            mask.protocol_id = ISO9141_PROTOCOL_ID;
            mask.data_size = 1;
            let mut pattern = J2534Message::default();
            pattern.protocol_id = ISO9141_PROTOCOL_ID;
            pattern.data_size = 1;
            let mut filter_id = 0;
            let status = unsafe {
                (self.start_filter)(channel, PASS_FILTER, &mut mask, &mut pattern, ptr::null_mut(), &mut filter_id)
            };
            self.check_with_kind(
                WorkerErrorKind::ChannelFailed,
                "PassThruStartMsgFilter(Subaru SSM K-Line)",
                status,
            )
        }

        fn exchange_subaru_ssm(
            &mut self,
            channel: u32,
            request_frame: &[u8],
            ecu_address: u8,
            tool_address: u8,
            timeout_ms: u32,
        ) -> Result<SubaruSsmExchange, J2534Failure> {
            if request_frame.len() < 6
                || request_frame[0] != 0x80
                || request_frame[1] != ecu_address
                || request_frame[2] != tool_address
                || request_frame[3] as usize + 5 != request_frame.len()
                || lognook_j2534_protocol::subaru_ssm::checksum(
                    &request_frame[..request_frame.len() - 1],
                ) != request_frame[request_frame.len() - 1]
            {
                return Err(J2534Failure::new(
                    WorkerErrorKind::MalformedResponse,
                    "Invalid SSM2 request frame",
                ));
            }
            let started = Instant::now();
            let mut request = J2534Message::default();
            request.protocol_id = ISO9141_PROTOCOL_ID;
            request.data_size = request_frame.len() as u32;
            request.data[..request_frame.len()].copy_from_slice(request_frame);
            let mut written = 1;
            let status = unsafe {
                (self.write_messages)(channel, &mut request, &mut written, timeout_ms)
            };
            self.check("PassThruWriteMsgs(Subaru SSM read)", status)?;
            if written != 1 {
                return Err(J2534Failure::new(
                    WorkerErrorKind::PassThruFailed,
                    "PassThruWriteMsgs did not accept the fixed read-only SSM2 request",
                ));
            }

            let deadline = started + Duration::from_millis(timeout_ms as u64);
            let mut stream = Vec::new();
            while Instant::now() < deadline {
                if let Some(frame) = lognook_j2534_protocol::subaru_ssm::take_ssm2_reply(
                    &mut stream,
                    request_frame,
                    ecu_address,
                    tool_address,
                )
                .map_err(|detail| {
                    J2534Failure::new(WorkerErrorKind::MalformedResponse, detail)
                })? {
                    parse_ssm2_frame(&frame, ecu_address, tool_address).map_err(|detail| {
                        J2534Failure::new(WorkerErrorKind::MalformedResponse, detail)
                    })?;
                    return Ok(SubaruSsmExchange {
                        response_frame: frame,
                        latency_us: started.elapsed().as_micros().min(u64::MAX as u128) as u64,
                    });
                }

                let remaining_ms = (deadline - Instant::now())
                    .as_millis()
                    .clamp(1, 100) as u32;
                let mut message = J2534Message::default();
                let mut count = 1_u32;
                let status = unsafe {
                    (self.read_messages)(channel, &mut message, &mut count, remaining_ms)
                };
                if count > 1 {
                    return Err(J2534Failure::new(
                        WorkerErrorKind::MalformedResponse,
                        "PassThruReadMsgs returned more than one K-Line message",
                    ));
                }
                if count == 0 {
                    if status == ERR_BUFFER_EMPTY || status == ERR_TIMEOUT {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    return Err(self.call_error(
                        WorkerErrorKind::PassThruFailed,
                        "PassThruReadMsgs(Subaru SSM)",
                        status,
                    ));
                }
                if message.data_size == 0 && (status == ERR_BUFFER_EMPTY || status == ERR_TIMEOUT)
                {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                if status != 0 && status != ERR_TIMEOUT {
                    return Err(self.call_error(
                        WorkerErrorKind::PassThruFailed,
                        "PassThruReadMsgs(Subaru SSM)",
                        status,
                    ));
                }
                let data_size = usize::try_from(message.data_size).unwrap_or(usize::MAX);
                if message.protocol_id != ISO9141_PROTOCOL_ID
                    || data_size > message.data.len()
                    || stream.len().saturating_add(data_size) > 520
                {
                    return Err(J2534Failure::new(
                        WorkerErrorKind::MalformedResponse,
                        format!(
                            "J2534 returned an invalid K-Line message (ProtocolID=0x{:08X}, DataSize={})",
                            message.protocol_id, message.data_size
                        ),
                    ));
                }
                if data_size > 0 {
                    stream.extend_from_slice(&message.data[..data_size]);
                }
            }
            Err(J2534Failure::new(
                WorkerErrorKind::NoResponse,
                format!("No Subaru SSM K-Line response within {timeout_ms} ms"),
            ))
        }

        fn read_mode01(
            &mut self,
            channel: u32,
            pid: u8,
            timeout_ms: u32,
            expected_ecu: Option<u16>,
        ) -> Result<Vec<Mode01Payload>, J2534Failure> {
            let started = Instant::now();
            let mut request = J2534Message::default();
            request.protocol_id = ISO15765_PROTOCOL_ID;
            request.tx_flags = ISO15765_FRAME_PAD;
            request.data_size = 6;
            request.data[..4].copy_from_slice(&mode01_request_id(expected_ecu)?.to_be_bytes());
            request.data[4..6].copy_from_slice(&[0x01, pid]);
            let mut written = 1;
            let status =
                unsafe { (self.write_messages)(channel, &mut request, &mut written, timeout_ms) };
            self.check("PassThruWriteMsgs(Mode01)", status)?;
            if written != 1 {
                return Err(J2534Failure::new(
                    WorkerErrorKind::PassThruFailed,
                    "PassThruWriteMsgs did not accept the single fixed Mode 01 request",
                ));
            }

            let deadline = started + READ_WINDOW.min(Duration::from_millis(timeout_ms as u64));
            let mut first_latency = None;
            let mut responses = Vec::new();
            loop {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                let remaining_ms = (deadline - now).as_millis().clamp(1, u32::MAX as u128) as u32;
                let mut messages = vec![J2534Message::default(); MAX_READ_MESSAGES];
                let mut count = MAX_READ_MESSAGES as u32;
                let status = unsafe {
                    (self.read_messages)(channel, messages.as_mut_ptr(), &mut count, remaining_ms)
                };
                // OPTA leaves NumMsgs unchanged on a timeout and does not fill pMsg.
                // Preserve genuine partial replies, but never decode untouched slots.
                if count as usize <= MAX_READ_MESSAGES
                    && timeout_left_messages_unfilled(status, &messages[..count as usize])
                {
                    break;
                }
                let batch =
                    classify_read_batch(status, count, MAX_READ_MESSAGES).map_err(|kind| {
                        J2534Failure::new(
                            kind,
                            "PassThruReadMsgs returned an invalid message count",
                        )
                    })?;
                let (batch_count, stop_after) = match batch {
                    ReadBatch::Messages { count, stop_after } => (count, stop_after),
                    ReadBatch::Retry => {
                        if Instant::now() >= deadline {
                            break;
                        }
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    ReadBatch::Stop => break,
                    ReadBatch::ApiError(status) => {
                        self.check("PassThruReadMsgs(Mode01)", status)?;
                        unreachable!("nonzero J2534 status was not reported as an error")
                    }
                };
                let latency_us = first_latency.get_or_insert_with(|| {
                    started.elapsed().as_micros().min(u64::MAX as u128) as u64
                });
                responses.extend(
                    decode_mode01_batch(&messages[..batch_count], pid, *latency_us)
                        .map_err(|failure| J2534Failure::new(failure.kind, failure.diagnostic))?,
                );
                if stop_after || matching_physical_reply(expected_ecu, &responses) {
                    break;
                }
            }
            if responses.is_empty() {
                return Err(J2534Failure::new(
                    WorkerErrorKind::NoResponse,
                    no_response_diagnostic(pid, started.elapsed().as_millis()),
                ));
            }
            Ok(responses)
        }

        fn read_mode09(
            &mut self,
            channel: u32,
            pid: u8,
            timeout_ms: u32,
        ) -> Result<Vec<Mode09Response>, J2534Failure> {
            if !is_supported_mode09_pid(pid) {
                return Err(J2534Failure::new(
                    WorkerErrorKind::MalformedResponse,
                    format!("Mode 09 PID {pid:02X} is not an identity PID"),
                ));
            }
            self.read_standard_pid(channel, 0x09, pid, timeout_ms)
        }

        fn read_mode06(
            &mut self,
            channel: u32,
            test_id: u8,
            timeout_ms: u32,
        ) -> Result<Vec<Mode09Response>, J2534Failure> {
            self.read_standard_pid(channel, 0x06, test_id, timeout_ms)
        }

        fn read_standard_dtc(
            &mut self,
            channel: u32,
            service: StandardDtcService,
            timeout_ms: u32,
        ) -> Result<Vec<StandardDtcResponse>, J2534Failure> {
            // The closed service enum admits only standard DTC operations. Even
            // `Clear` transmits one fixed 7DF/04 frame and is never retried.
            let started = Instant::now();
            let request_service = service.request_byte();
            let mut request = standard_dtc_request(service);
            let mut written = 1;
            let status =
                unsafe { (self.write_messages)(channel, &mut request, &mut written, timeout_ms) };
            self.check("PassThruWriteMsgs(standard DTC)", status)?;
            if written != 1 {
                return Err(J2534Failure::new(
                    WorkerErrorKind::PassThruFailed,
                    "PassThruWriteMsgs did not accept the single fixed standard DTC request",
                ));
            }

            let deadline =
                started + IDENTITY_READ_WINDOW.min(Duration::from_millis(timeout_ms as u64));
            let mut first_latency = None;
            let mut responses = Vec::new();
            let mut read_failure = None;
            loop {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                let remaining_ms = (deadline - now).as_millis().clamp(1, u32::MAX as u128) as u32;
                let mut messages = vec![J2534Message::default(); MAX_READ_MESSAGES];
                let mut count = MAX_READ_MESSAGES as u32;
                let status = unsafe {
                    (self.read_messages)(channel, messages.as_mut_ptr(), &mut count, remaining_ms)
                };
                if count as usize <= MAX_READ_MESSAGES
                    && timeout_left_messages_unfilled(status, &messages[..count as usize])
                {
                    break;
                }
                let batch = match classify_read_batch(status, count, MAX_READ_MESSAGES) {
                    Ok(batch) => batch,
                    Err(kind) => {
                        let latency_us = *first_latency.get_or_insert_with(|| {
                            started.elapsed().as_micros().min(u64::MAX as u128) as u64
                        });
                        responses.extend(decode_standard_dtc_api_error_messages(
                            &messages, count, service, latency_us,
                        ));
                        read_failure = Some(J2534Failure::new(
                            kind,
                            "PassThruReadMsgs returned an invalid standard DTC message count",
                        ));
                        break;
                    }
                };
                // Functional DTC requests may have several ECUs queued. Keep draining
                // even when J2534 reports a timeout alongside one buffered response.
                let batch_count = match batch {
                    ReadBatch::Messages { count, .. } => count,
                    ReadBatch::Retry => {
                        if Instant::now() >= deadline {
                            break;
                        }
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    ReadBatch::Stop => break,
                    ReadBatch::ApiError(status) => {
                        let latency_us = *first_latency.get_or_insert_with(|| {
                            started.elapsed().as_micros().min(u64::MAX as u128) as u64
                        });
                        responses.extend(decode_standard_dtc_api_error_messages(
                            &messages, count, service, latency_us,
                        ));
                        read_failure = Some(
                            self.check("PassThruReadMsgs(standard DTC)", status)
                                .expect_err("ApiError always carries a failing J2534 status"),
                        );
                        break;
                    }
                };
                let latency_us = *first_latency.get_or_insert_with(|| {
                    started.elapsed().as_micros().min(u64::MAX as u128) as u64
                });
                responses.extend(decode_standard_dtc_batch(
                    &messages[..batch_count],
                    service,
                    latency_us,
                ));
            }
            if let Some(error) = read_failure {
                let latency_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
                responses = retain_standard_dtc_failure(responses, error, latency_us)?;
            }
            if responses.is_empty() {
                return Err(J2534Failure::new(
                    WorkerErrorKind::NoResponse,
                    format!(
                        "No response for standard Mode {:02X} DTC request after {} ms",
                        request_service,
                        started.elapsed().as_millis()
                    ),
                ));
            }
            Ok(responses)
        }

        fn read_oem_candidate(
            &mut self,
            channel: u32,
            request_id: u16,
            response_id: u16,
            request: OemCandidateRequestKind,
            timeout_ms: u32,
        ) -> Result<Vec<OemCandidateResponse>, J2534Failure> {
            if request_id > 0x7FF || response_id > 0x7FF || request_id == response_id {
                return Err(J2534Failure::new(
                    WorkerErrorKind::MalformedResponse,
                    "OEM candidate requires distinct exact 11-bit request and response IDs",
                ));
            }
            let filter_id = if is_standard_obd_flow_control_route(request_id, response_id) {
                None
            } else {
                Some(self.start_oem_flow_control_filter(channel, request_id, response_id)?)
            };
            let started = Instant::now();
            let result = self.read_oem_candidate_exchange(
                channel,
                request_id,
                response_id,
                request,
                timeout_ms,
            );
            let cleanup = filter_id.and_then(|filter_id| {
                let status = unsafe { (self.stop_filter)(channel, filter_id) };
                self.check_with_kind(
                    WorkerErrorKind::ChannelFailed,
                    "PassThruStopMsgFilter(OEM route)",
                    status,
                )
                .err()
            });
            match (result, cleanup) {
                (Ok(mut responses), Some(error)) => {
                    responses.push(oem_candidate_failure_response(
                        request_id,
                        response_id,
                        error.kind,
                        format!(
                            "Temporary OEM route filter cleanup failed: {}",
                            error.message
                        ),
                        started.elapsed().as_micros().min(u64::MAX as u128) as u64,
                    ));
                    Ok(responses)
                }
                (Err(exchange_error), Some(cleanup_error)) => Err(J2534Failure::new(
                    exchange_error.kind,
                    format!(
                        "{}; temporary OEM route filter cleanup failed: {}",
                        exchange_error.message, cleanup_error.message
                    ),
                )),
                (result, None) => result,
            }
        }

        fn disconnect(&mut self, channel: u32) -> Result<(), J2534Failure> {
            let status = unsafe { (self.disconnect)(channel) };
            self.check("PassThruDisconnect", status)
        }

        fn close_device(&mut self, device: u32) -> Result<(), J2534Failure> {
            let status = unsafe { (self.close)(device) };
            self.check("PassThruClose", status)
        }
    }

    unsafe fn load_function<T: Copy>(
        module: HMODULE,
        name: &'static [u8],
    ) -> Result<T, J2534Failure> {
        let function = unsafe { GetProcAddress(module, name.as_ptr()) }.ok_or_else(|| {
            J2534Failure::new(
                WorkerErrorKind::UnsupportedApi,
                format!(
                    "Registered J2534 DLL is missing {}",
                    String::from_utf8_lossy(name)
                ),
            )
        })?;
        Ok(unsafe { std::mem::transmute_copy::<FARPROC, T>(&Some(function)) })
    }

    unsafe fn load_optional_function<T: Copy>(module: HMODULE, name: &'static [u8]) -> Option<T> {
        let function = unsafe { GetProcAddress(module, name.as_ptr()) }?;
        Some(unsafe { std::mem::transmute_copy::<FARPROC, T>(&Some(function)) })
    }

    fn nul_terminated(value: &[c_char]) -> String {
        let length = value
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(value.len());
        String::from_utf8_lossy(
            &value[..length]
                .iter()
                .map(|character| *character as u8)
                .collect::<Vec<_>>(),
        )
        .into_owned()
    }
}

#[cfg(all(target_os = "windows", target_arch = "x86"))]
pub(crate) use dynamic::DynamicJ2534;

#[cfg(test)]
mod tests {
    use super::{
        ERR_BUFFER_EMPTY, ERR_TIMEOUT, ISO15765_PROTOCOL_ID, J2534Message,
        OemCandidateResponseState, RX_STATUS_TX_MSG_TYPE, ReadBatch,
        build_obd_flow_control_filters, build_oem_flow_control_filter, classify_read_batch,
        decode_mode01_batch, decode_mode01_message, decode_mode09_api_error_messages,
        decode_mode09_batch, decode_mode09_message, decode_oem_candidate_message,
        decode_standard_dtc_api_error_messages, decode_standard_dtc_message,
        is_standard_obd_flow_control_route, matching_physical_reply, mode01_request_id,
        mode09_request_id, no_response_diagnostic, no_response_mode09_diagnostic,
        oem_candidate_request_message, retain_mode09_failure, retain_standard_dtc_failure,
        standard_dtc_request, timeout_left_messages_unfilled,
    };
    use lognook_j2534_protocol::{
        Mode09Payload, Mode09Response, OemCandidateRequestKind, OemCandidateResponse,
        StandardDtcResponse, StandardDtcService, WorkerErrorKind,
    };

    fn rpm_message() -> J2534Message {
        let mut message = J2534Message::default();
        message.protocol_id = ISO15765_PROTOCOL_ID;
        message.data[..8].copy_from_slice(&[0x00, 0x00, 0x07, 0xE8, 0x41, 0x0C, 0x1A, 0xF8]);
        message.data_size = 8;
        message
    }

    #[test]
    fn mode06_filters_stale_payloads_but_retains_pending_and_matching_results() {
        let messages = [
            mode09_message(&[0x41, 0x0C, 0, 0]),
            mode09_message(&[0x46, 0x20, 0, 0, 0, 0]),
            mode09_message(&[0x7F, 6, 0x78]),
            mode09_message(&[0x46, 0, 0x80, 0, 0, 0]),
        ];
        let filtered =
            super::filter_standard_pid_responses(decode_mode09_batch(&messages, 1_000), 6, 0);
        assert_eq!(filtered.len(), 2);
        assert!(
            matches!(&filtered[0], Mode09Response::Payload(payload) if payload.bytes == [0x7F, 6, 0x78])
        );
        assert!(
            matches!(&filtered[1], Mode09Response::Payload(payload) if payload.bytes == [0x46, 0, 0x80, 0, 0, 0])
        );
        let mut malformed = mode09_message(&[0x46]);
        malformed.protocol_id = 5;
        let raw = malformed.data[..malformed.data_size as usize].to_vec();
        let filtered =
            super::filter_standard_pid_responses(decode_mode09_batch(&[malformed], 1_000), 6, 0);
        assert!(
            matches!(&filtered[0], Mode09Response::MalformedFrame(frame) if frame.raw_bytes == raw && frame.detail.contains("Mode 06"))
        );
    }

    fn mode09_message(bytes: &[u8]) -> J2534Message {
        let mut message = J2534Message::default();
        message.protocol_id = ISO15765_PROTOCOL_ID;
        message.data[..4].copy_from_slice(&0x7E8_u32.to_be_bytes());
        message.data[4..4 + bytes.len()].copy_from_slice(bytes);
        message.data_size = (4 + bytes.len()) as u32;
        message
    }

    #[test]
    fn oem_candidate_routes_use_shared_standard_filters_or_one_padded_exact_filter() {
        assert!(is_standard_obd_flow_control_route(0x7E0, 0x7E8));
        assert!(is_standard_obd_flow_control_route(0x7E7, 0x7EF));
        assert!(!is_standard_obd_flow_control_route(0x710, 0x718));
        assert!(!is_standard_obd_flow_control_route(0x7E0, 0x7E9));

        let filter = build_oem_flow_control_filter(0x710, 0x718);
        assert_eq!(filter.filter_type, super::FLOW_CONTROL_FILTER);
        assert_eq!(filter.mask.tx_flags, super::ISO15765_FRAME_PAD);
        assert_eq!(&filter.mask.data[..4], &[0, 0, 0x07, 0xFF]);
        assert_eq!(&filter.pattern.data[..4], &0x718_u32.to_be_bytes());
        assert_eq!(filter.pattern.tx_flags, super::ISO15765_FRAME_PAD);
        assert_eq!(&filter.flow_control.data[..4], &0x710_u32.to_be_bytes());
        assert_eq!(filter.flow_control.tx_flags, super::ISO15765_FRAME_PAD);
    }

    #[test]
    fn oem_candidate_request_is_one_padded_iso15765_payload() {
        let request =
            oem_candidate_request_message(0x710, OemCandidateRequestKind::Mode22 { did: 0x1234 });
        assert_eq!(request.protocol_id, ISO15765_PROTOCOL_ID);
        assert_eq!(request.tx_flags, super::ISO15765_FRAME_PAD);
        assert_eq!(request.data_size, 7);
        assert_eq!(&request.data[..7], &[0, 0, 0x07, 0x10, 0x22, 0x12, 0x34]);
    }

    #[test]
    fn oem_candidate_response_ignores_start_notification_and_wrong_responder() {
        let mut start = dtc_message(0x718, &[]);
        start.rx_status = 0x02;
        assert!(decode_oem_candidate_message(&start, 1).is_none());

        let request = OemCandidateRequestKind::Mode22 { did: 0x1234 };
        let wrong_route = dtc_message(0x719, &[0x62, 0x12, 0x34, 0x2A]);
        let mut state = OemCandidateResponseState::default();
        assert!(
            decode_oem_candidate_message(&wrong_route, 12_500)
                .is_some_and(|response| state.accept(response, 0x718, &request).is_none())
        );
        assert!(state.pending.is_none());
    }

    #[test]
    fn oem_candidate_exchange_skips_stale_mode01_and_other_oem_identifier() {
        let request = OemCandidateRequestKind::Mode22 { did: 0x1234 };
        let mut state = OemCandidateResponseState::default();

        for stale in [
            dtc_message(0x718, &[0x41, 0x0C, 0x1A, 0xF8]),
            dtc_message(0x718, &[0x62, 0x12, 0x35, 0x2A]),
        ] {
            let response = decode_oem_candidate_message(&stale, 1).unwrap();
            assert!(state.accept(response, 0x718, &request).is_none());
        }
        let matching =
            decode_oem_candidate_message(&dtc_message(0x718, &[0x62, 0x12, 0x34, 0x2A]), 1)
                .unwrap();
        assert!(state.accept(matching, 0x718, &request).is_some());
    }

    #[test]
    fn oem_candidate_pending_is_retained_until_a_final_response_arrives() {
        let request = OemCandidateRequestKind::Mode22 { did: 0x1234 };
        let mut state = OemCandidateResponseState::default();
        let pending =
            decode_oem_candidate_message(&dtc_message(0x718, &[0x7F, 0x22, 0x78]), 1).unwrap();
        assert!(state.accept(pending, 0x718, &request).is_none());
        assert!(state.pending.is_some());

        let positive =
            decode_oem_candidate_message(&dtc_message(0x718, &[0x62, 0x12, 0x34, 0x2A]), 2)
                .unwrap();
        let terminal = state.accept(positive, 0x718, &request).unwrap();
        let outcomes = state.finish(terminal);
        assert!(matches!(
            outcomes.as_slice(),
            [OemCandidateResponse::Payload(pending), OemCandidateResponse::Payload(positive)]
                if pending.bytes == [0x7F, 0x22, 0x78]
                    && positive.bytes == [0x62, 0x12, 0x34, 0x2A]
        ));
    }

    #[test]
    fn oem_candidate_malformed_frame_retains_raw_j2534_bytes() {
        let mut malformed = dtc_message(0x718, &[0x61, 0x01]);
        malformed.rx_status = 0x100;
        let raw = malformed.data[..malformed.data_size as usize].to_vec();
        assert!(matches!(
            decode_oem_candidate_message(&malformed, 4),
            Some(OemCandidateResponse::MalformedFrame(frame))
                if frame.response_id == Some(0x718) && frame.raw_bytes == raw
        ));
        let response = decode_oem_candidate_message(&malformed, 4).unwrap();
        let mut state = OemCandidateResponseState::default();
        assert!(
            state
                .accept(
                    response,
                    0x718,
                    &OemCandidateRequestKind::Mode21 { identifier: 1 }
                )
                .is_some()
        );
    }

    #[test]
    fn tactrix_start_of_message_is_not_a_completed_obd_response() {
        // Captured from the original Tactrix driver during Ford Mode 09 reads.
        let mut start = mode09_message(&[]);
        start.rx_status = 0x02;
        assert_eq!(start.data_size, 4);
        assert_eq!(&start.data[..4], &[0, 0, 0x07, 0xE8]);
        assert!(decode_mode01_message(&start, 0x0C, 0).unwrap().is_none());
        assert!(decode_mode09_message(&start, 0).is_none());
        for service in [
            StandardDtcService::Stored,
            StandardDtcService::Pending,
            StandardDtcService::Permanent,
            StandardDtcService::Clear,
        ] {
            assert!(decode_standard_dtc_message(&start, service, 0).is_none());
        }

        // A header-only completed message is still genuinely malformed.
        start.rx_status = 0;
        assert!(decode_mode01_message(&start, 0x0C, 0).is_err());
        assert!(matches!(
            decode_mode09_message(&start, 0),
            Some(Mode09Response::MalformedFrame(_))
        ));
        assert!(matches!(
            decode_standard_dtc_message(&start, StandardDtcService::Stored, 0),
            Some(StandardDtcResponse::MalformedFrame(_))
        ));
    }

    #[test]
    fn tactrix_start_notification_does_not_hide_the_completed_vin_reply() {
        let mut start = mode09_message(&[]);
        start.rx_status = 0x02;
        let bytes = [vec![0x49, 0x02, 0x01], b"1HGCM82633A004352".to_vec()].concat();
        let completed = mode09_message(&bytes);
        let responses = decode_mode09_batch(&[start, completed], 12_000);
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            &responses[0],
            Mode09Response::Payload(payload)
                if payload.ecu_id == "7E8" && payload.bytes == bytes
        ));
    }

    #[test]
    fn j2534_message_matches_the_v0404_buffer_layout() {
        assert_eq!(std::mem::size_of::<J2534Message>(), 4152);
    }

    #[test]
    fn live_read_targets_only_a_discovered_standard_ecu_and_finishes_on_its_reply() {
        assert_eq!(mode01_request_id(None).unwrap(), 0x7DF);
        assert_eq!(mode01_request_id(Some(0x7E8)).unwrap(), 0x7E0);
        assert_eq!(mode01_request_id(Some(0x7EF)).unwrap(), 0x7E7);
        assert!(mode01_request_id(Some(0x700)).is_err());
        let reply = decode_mode01_message(&rpm_message(), 0x0C, 1000)
            .unwrap()
            .unwrap();
        assert!(!matching_physical_reply(None, std::slice::from_ref(&reply)));
        assert!(!matching_physical_reply(
            Some(0x7E9),
            std::slice::from_ref(&reply)
        ));
        assert!(matching_physical_reply(Some(0x7E8), &[reply]));
    }

    #[test]
    fn iso15765_filters_use_flow_control_and_cover_each_standard_ecu_route() {
        let filters = build_obd_flow_control_filters();
        assert_eq!(filters.len(), 8);
        for (index, filter) in filters.iter().enumerate() {
            assert_eq!(filter.filter_type, 0x03);
            assert_eq!(filter.mask.protocol_id, ISO15765_PROTOCOL_ID);
            assert_eq!(filter.mask.data_size, 4);
            assert_eq!(&filter.mask.data[..4], &[0x00, 0x00, 0x07, 0xFF]);
            assert_eq!(filter.pattern.protocol_id, ISO15765_PROTOCOL_ID);
            assert_eq!(
                u32::from_be_bytes(filter.pattern.data[..4].try_into().unwrap()),
                0x7E8 + index as u32
            );
            assert_eq!(filter.flow_control.protocol_id, ISO15765_PROTOCOL_ID);
            assert_eq!(filter.flow_control.data_size, 4);
            assert_eq!(
                u32::from_be_bytes(filter.flow_control.data[..4].try_into().unwrap()),
                0x7E0 + index as u32
            );
        }
    }

    #[test]
    fn flow_control_filters_use_tactrix_padded_iso15765_settings() {
        // Installed Tactrix samples/canlogger/canlogger.cpp initializes all
        // three filter messages from an ISO15765_FRAME_PAD message.
        for filter in build_obd_flow_control_filters() {
            for message in [&filter.mask, &filter.pattern, &filter.flow_control] {
                assert_eq!(message.tx_flags, super::ISO15765_FRAME_PAD);
            }
        }
    }

    #[test]
    fn read_timeout_keeps_partial_messages_and_only_empty_buffer_retries() {
        assert_eq!(
            classify_read_batch(ERR_TIMEOUT, 1, 8),
            Ok(ReadBatch::Messages {
                count: 1,
                stop_after: true,
            })
        );
        assert_eq!(
            classify_read_batch(0, 2, 8),
            Ok(ReadBatch::Messages {
                count: 2,
                stop_after: false,
            })
        );
        assert_eq!(classify_read_batch(ERR_TIMEOUT, 0, 8), Ok(ReadBatch::Stop));
        assert_eq!(
            classify_read_batch(ERR_BUFFER_EMPTY, 0, 8),
            Ok(ReadBatch::Retry)
        );
    }

    #[test]
    fn no_response_diagnostic_names_the_pid_and_actual_wait() {
        assert_eq!(
            no_response_diagnostic(0x00, 250),
            "No response for standard Mode 01 PID 00 after 250 ms"
        );
    }

    #[test]
    fn mode09_uses_functional_route_and_preserves_all_raw_identity_payloads() {
        assert_eq!(mode09_request_id(), 0x7DF);
        for bytes in [
            &[0x49, 0x02, 0x01, b'1'][..],
            &[0x7F, 0x09, 0x12][..],
            &[0x49, 0x04][..],
        ] {
            let Some(Mode09Response::Payload(payload)) =
                decode_mode09_message(&mode09_message(bytes), 2_500)
            else {
                panic!("valid ISO15765 frame was not retained as a Mode 09 payload");
            };
            assert_eq!(payload.ecu_id, "7E8");
            assert_eq!(payload.bytes, bytes);
            assert_eq!(payload.latency_us, 2_500);
        }
    }

    #[test]
    fn standard_dtc_builder_emits_one_fixed_functional_request_for_each_allowlisted_service() {
        for (service, service_byte) in [
            (StandardDtcService::Stored, 0x03),
            (StandardDtcService::Pending, 0x07),
            (StandardDtcService::Permanent, 0x0A),
            (StandardDtcService::Clear, 0x04),
        ] {
            let message = standard_dtc_request(service);
            assert_eq!(message.protocol_id, ISO15765_PROTOCOL_ID);
            assert_eq!(message.tx_flags, 0x40);
            assert_eq!(message.data_size, 5);
            assert_eq!(&message.data[..5], &[0x00, 0x00, 0x07, 0xDF, service_byte]);
            assert!(message.data[5..].iter().all(|byte| *byte == 0));
        }
    }

    #[test]
    fn mode09_malformed_can_frames_and_no_response_remain_typed() {
        let mut malformed = mode09_message(&[0x49, 0x02]);
        malformed.rx_status = 0x80;
        assert!(matches!(
            decode_mode09_message(&malformed, 0),
            Some(Mode09Response::MalformedFrame(frame))
                if frame.ecu_id.is_none()
                    && frame.raw_bytes == malformed.data[..malformed.data_size as usize]
                    && frame.rx_status == 0x80
        ));
        assert_eq!(
            no_response_mode09_diagnostic(0x02, 1_500),
            "No response for standard Mode 09 PID 02 after 1500 ms"
        );
    }

    #[test]
    fn mode09_keeps_a_partial_response_returned_with_a_read_timeout() {
        assert_eq!(
            classify_read_batch(ERR_TIMEOUT, 1, 1),
            Ok(ReadBatch::Messages {
                count: 1,
                stop_after: true,
            })
        );
        let Some(Mode09Response::Payload(payload)) =
            decode_mode09_message(&mode09_message(&[0x49, 0x02, 0x01, b'1']), 1_000)
        else {
            panic!("partial ISO15765 frame was not retained");
        };
        assert_eq!(payload.bytes, [0x49, 0x02, 0x01, b'1']);
    }

    #[test]
    fn mode09_batch_keeps_valid_peers_and_full_malformed_frame_bytes_in_order() {
        let mut vin = vec![0x49, 0x02, 0x01];
        vin.extend_from_slice(b"1HGCM82633A004352");
        let first = mode09_message(&vin);

        let mut malformed = mode09_message(&[0x49; 28]);
        malformed.protocol_id = 0x05;
        let malformed_bytes = malformed.data[..malformed.data_size as usize].to_vec();
        let malformed_protocol_id = malformed.protocol_id;
        let malformed_data_size = malformed.data_size;
        let mut last = mode09_message(&vin);
        last.data[..4].copy_from_slice(&0x7E9_u32.to_be_bytes());

        let responses = decode_mode09_batch(&[first, malformed, last], 9_000);

        assert_eq!(responses.len(), 3);
        assert!(matches!(
            &responses[0],
            Mode09Response::Payload(payload)
                if payload.ecu_id == "7E8"
                    && payload.bytes == vin
                    && payload.latency_us == 9_000
        ));
        assert!(matches!(
            &responses[1],
            Mode09Response::MalformedFrame(frame)
                if frame.ecu_id.is_none()
                    && frame.raw_bytes == malformed_bytes
                    && frame.raw_bytes.len() > 16
                    && frame.declared_data_size == malformed_data_size
                    && frame.protocol_id == malformed_protocol_id
                    && frame.detail.contains("ProtocolID=0x00000005")
        ));
        assert!(matches!(
            &responses[2],
            Mode09Response::Payload(payload)
                if payload.ecu_id == "7E9"
                    && payload.bytes == vin
                    && payload.latency_us == 9_000
        ));
    }

    #[test]
    fn fatal_mode09_read_keeps_accumulated_frames_and_exposes_the_failure() {
        let valid = Mode09Response::Payload(Mode09Payload {
            ecu_id: "7E8".into(),
            bytes: vec![0x49, 0x02, 0x01, b'1'],
            latency_us: 900,
        });
        let failure = super::J2534Failure::new(
            WorkerErrorKind::PassThruFailed,
            "PassThruReadMsgs returned 0x00000001",
        );

        let retained = retain_mode09_failure(vec![valid.clone()], failure.clone(), 1_700).unwrap();
        assert_eq!(retained[0], valid);
        assert!(matches!(
            &retained[1],
            Mode09Response::Failure(failure)
                if failure.kind == WorkerErrorKind::PassThruFailed
                    && failure.message.contains("0x00000001")
                    && failure.latency_us == 1_700
        ));
        assert_eq!(
            retain_mode09_failure(Vec::new(), failure.clone(), 1),
            Err(failure)
        );
    }

    #[test]
    fn mode09_api_error_ignores_an_untouched_buffer_but_keeps_a_real_partial_frame() {
        let untouched = [J2534Message::default()];
        let failure = super::J2534Failure::new(
            WorkerErrorKind::PassThruFailed,
            "PassThruReadMsgs returned 0x00000001",
        );
        let untouched_results = decode_mode09_api_error_messages(&untouched, 1, 900);
        assert!(untouched_results.is_empty());
        assert_eq!(
            retain_mode09_failure(untouched_results, failure.clone(), 1_000),
            Err(failure.clone())
        );

        let partial = [mode09_message(&[0x49, 0x02, 0x01, b'1'])];
        let partial_results = decode_mode09_api_error_messages(&partial, 1, 900);
        let partial_results = retain_mode09_failure(partial_results, failure, 1_000).unwrap();
        assert_eq!(partial_results.len(), 2);
        assert!(matches!(
            &partial_results[0],
            Mode09Response::Payload(payload)
                if payload.ecu_id == "7E8"
                    && payload.bytes == [0x49, 0x02, 0x01, b'1']
                    && payload.latency_us == 900
        ));
        assert!(matches!(partial_results[1], Mode09Response::Failure(_)));
    }

    #[test]
    fn standard_dtc_preserves_positive_and_negative_responder_bytes() {
        for (service, positive, negative) in [
            (
                StandardDtcService::Stored,
                &[0x43, 0x01, 0x33][..],
                &[0x7F, 0x03, 0x22][..],
            ),
            (
                StandardDtcService::Pending,
                &[0x47, 0x01, 0x33][..],
                &[0x7F, 0x07, 0x22][..],
            ),
            (
                StandardDtcService::Permanent,
                &[0x4A, 0x01, 0x33][..],
                &[0x7F, 0x0A, 0x22][..],
            ),
            (
                StandardDtcService::Clear,
                &[0x44][..],
                &[0x7F, 0x04, 0x22][..],
            ),
        ] {
            for (ecu, bytes) in [(0x7E8_u32, positive), (0x7E9_u32, negative)] {
                let message = dtc_message(ecu, bytes);
                let Some(StandardDtcResponse::Payload(payload)) =
                    decode_standard_dtc_message(&message, service, 2_500)
                else {
                    panic!("valid standard DTC payload was not retained");
                };
                assert_eq!(payload.ecu_id, format!("{ecu:03X}"));
                assert_eq!(payload.bytes, bytes);
                assert_eq!(payload.latency_us, 2_500);
            }
        }
    }

    #[test]
    fn standard_dtc_rejects_wrong_service_and_keeps_malformed_frames() {
        let wrong = dtc_message(0x7E8, &[0x62, 0xF1, 0x90]);
        assert!(matches!(
            decode_standard_dtc_message(&wrong, StandardDtcService::Stored, 0),
            Some(StandardDtcResponse::MalformedFrame(frame))
                if frame.ecu_id == Some("7E8".into())
                    && frame.raw_bytes == wrong.data[..wrong.data_size as usize]
                    && frame.detail.contains("unexpected diagnostic response service")
        ));

        let wrong_negative = dtc_message(0x7E8, &[0x7F, 0x07, 0x22]);
        assert!(matches!(
            decode_standard_dtc_message(&wrong_negative, StandardDtcService::Stored, 0),
            Some(StandardDtcResponse::MalformedFrame(frame))
                if frame.detail.contains("does not echo the requested service")
        ));

        let mut bad_flags = dtc_message(0x7E8, &[0x43]);
        bad_flags.rx_status = 0x80;
        assert!(matches!(
            decode_standard_dtc_message(&bad_flags, StandardDtcService::Stored, 0),
            Some(StandardDtcResponse::MalformedFrame(frame))
                if frame.ecu_id.is_none() && frame.rx_status == 0x80
        ));
    }

    #[test]
    fn standard_dtc_partial_read_errors_and_empty_timeout_remain_distinct() {
        let partial = [dtc_message(0x7E8, &[0x44])];
        let responses =
            decode_standard_dtc_api_error_messages(&partial, 1, StandardDtcService::Clear, 900);
        let failure =
            super::J2534Failure::new(WorkerErrorKind::PassThruFailed, "fixture read error");
        let responses = retain_standard_dtc_failure(responses, failure, 1_000).unwrap();
        assert!(matches!(responses[0], StandardDtcResponse::Payload(_)));
        assert!(matches!(
            &responses[1],
            StandardDtcResponse::Failure(failure)
                if failure.kind == WorkerErrorKind::PassThruFailed
                    && failure.latency_us == 1_000
        ));

        assert!(
            decode_standard_dtc_api_error_messages(
                &[J2534Message::default()],
                1,
                StandardDtcService::Clear,
                1
            )
            .is_empty()
        );
        assert_eq!(classify_read_batch(ERR_TIMEOUT, 0, 1), Ok(ReadBatch::Stop));
    }

    fn dtc_message(ecu: u32, bytes: &[u8]) -> J2534Message {
        let mut message = J2534Message::default();
        message.protocol_id = ISO15765_PROTOCOL_ID;
        message.data[..4].copy_from_slice(&ecu.to_be_bytes());
        message.data[4..4 + bytes.len()].copy_from_slice(bytes);
        message.data_size = (4 + bytes.len()) as u32;
        message
    }

    #[test]
    fn ignores_tx_done_indications_and_rejects_nonstandard_address_flags() {
        let mut tx_done = J2534Message::default();
        tx_done.protocol_id = ISO15765_PROTOCOL_ID;
        tx_done.rx_status = RX_STATUS_TX_MSG_TYPE;
        tx_done.data_size = 4;
        tx_done.data[..4].copy_from_slice(&[0x00, 0x00, 0x07, 0xDF]);
        let response = rpm_message();
        let decoded =
            decode_mode01_batch(&[tx_done.clone(), response.clone()], 0x0C, 11_300).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].bytes, vec![0x41, 0x0C, 0x1A, 0xF8]);
        assert!(
            decode_mode01_batch(&[tx_done], 0x0C, 11_300)
                .unwrap()
                .is_empty()
        );

        let mut extended_address = response;
        extended_address.rx_status = 0x80;
        assert!(decode_mode01_message(&extended_address, 0x0C, 0).is_err());
        let mut extended_can = rpm_message();
        extended_can.rx_status = 0x100;
        assert!(decode_mode01_message(&extended_can, 0x0C, 0).is_err());
    }

    #[test]
    fn parses_iso15765_mode01_response_with_ecu_identity() {
        let payload = decode_mode01_message(&rpm_message(), 0x0C, 11_300)
            .unwrap()
            .unwrap();
        assert_eq!(payload.ecu_id, "7E8");
        assert_eq!(payload.bytes, vec![0x41, 0x0C, 0x1A, 0xF8]);
        assert_eq!(payload.latency_us, 11_300);
    }

    #[test]
    fn rejects_non_iso15765_messages() {
        let mut message = rpm_message();
        message.protocol_id = 0x05;
        assert_eq!(
            decode_mode01_message(&message, 0x0C, 0),
            Err(WorkerErrorKind::MalformedResponse)
        );
    }

    #[test]
    fn opta_timeout_with_unchanged_count_does_not_fabricate_a_response() {
        assert!(timeout_left_messages_unfilled(
            ERR_TIMEOUT,
            &[J2534Message::default()]
        ));
        assert!(!timeout_left_messages_unfilled(
            ERR_TIMEOUT,
            &[rpm_message()]
        ));
        assert!(!timeout_left_messages_unfilled(
            0,
            &[J2534Message::default()]
        ));
    }

    #[test]
    fn malformed_batch_diagnostic_includes_j2534_message_metadata() {
        let mut message = rpm_message();
        message.protocol_id = 0x05;

        let failure = decode_mode01_batch(&[message], 0x0C, 0).unwrap_err();
        assert_eq!(failure.kind, WorkerErrorKind::MalformedResponse);

        assert!(
            failure.diagnostic.contains("ProtocolID=0x00000005"),
            "{}",
            failure.diagnostic
        );
        assert!(
            failure.diagnostic.contains("RxStatus=0x00000000"),
            "{}",
            failure.diagnostic
        );
        assert!(
            failure.diagnostic.contains("DataSize=8"),
            "{}",
            failure.diagnostic
        );
        assert!(
            failure
                .diagnostic
                .contains("DataPrefix=[00 00 07 E8 41 0C 1A F8]"),
            "{}",
            failure.diagnostic
        );
    }

    #[test]
    fn rejects_unexpected_ecu_addresses() {
        let mut message = rpm_message();
        message.data[3] = 0xD8;
        assert_eq!(
            decode_mode01_message(&message, 0x0C, 0),
            Err(WorkerErrorKind::MalformedResponse)
        );
    }

    #[test]
    fn rejects_responses_for_a_different_pid_and_aggregated_payloads() {
        let mut wrong_pid = rpm_message();
        wrong_pid.data[5] = 0x05;
        assert_eq!(
            decode_mode01_message(&wrong_pid, 0x0C, 0),
            Err(WorkerErrorKind::MalformedResponse)
        );

        let mut aggregated = rpm_message();
        aggregated.data[8..11].copy_from_slice(&[0x41, 0x05, 0x64]);
        aggregated.data_size = 11;
        assert_eq!(
            decode_mode01_message(&aggregated, 0x0C, 0),
            Err(WorkerErrorKind::MalformedResponse)
        );
    }
}
