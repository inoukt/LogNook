use lognook_j2534_protocol::{
    J2534Identity, Mode01Payload, Mode09Failure, Mode09MalformedFrame, Mode09Payload,
    Mode09Response, WorkerErrorKind,
};
use std::fmt;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
use std::time::Duration;

pub(crate) const ISO15765_PROTOCOL_ID: u32 = 6;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
const ISO15765_FRAME_PAD: u32 = 0x40;
const FLOW_CONTROL_FILTER: u32 = 0x03;
const RX_STATUS_TX_MSG_TYPE: u32 = 0x01;
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
            mask.data_size = 4;
            mask.data[..4].copy_from_slice(&[0x00, 0x00, 0x07, 0xFF]);

            let mut pattern = J2534Message::default();
            pattern.protocol_id = ISO15765_PROTOCOL_ID;
            pattern.data_size = 4;
            pattern.data[..4].copy_from_slice(&(0x7E8_u32 + index).to_be_bytes());

            let mut flow_control = J2534Message::default();
            flow_control.protocol_id = ISO15765_PROTOCOL_ID;
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

impl fmt::Display for J2534Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}: {}", self.kind, self.message)
    }
}

pub(crate) trait PassThruApi {
    fn open_device(&mut self) -> Result<u32, J2534Failure>;
    fn read_version(&mut self, device: u32) -> Result<J2534Identity, J2534Failure>;
    fn connect_iso15765(&mut self, device: u32) -> Result<u32, J2534Failure>;
    fn start_obd_filter(&mut self, channel: u32) -> Result<(), J2534Failure>;
    fn read_mode01(
        &mut self,
        channel: u32,
        pid: u8,
        timeout_ms: u32,
        expected_ecu: Option<u16>,
    ) -> Result<Vec<Mode01Payload>, J2534Failure>;
    fn read_mode09(
        &mut self,
        channel: u32,
        pid: u8,
        timeout_ms: u32,
    ) -> Result<Vec<Mode09Response>, J2534Failure>;
    fn disconnect(&mut self, channel: u32) -> Result<(), J2534Failure>;
    fn close_device(&mut self, device: u32) -> Result<(), J2534Failure>;
}

pub(crate) fn decode_mode01_message(
    message: &J2534Message,
    requested_pid: u8,
    latency_us: u64,
) -> Result<Option<Mode01Payload>, WorkerErrorKind> {
    if message.rx_status & RX_STATUS_TX_MSG_TYPE != 0 {
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

fn decode_mode09_message(message: &J2534Message, latency_us: u64) -> Option<Mode09Response> {
    if message.rx_status & RX_STATUS_TX_MSG_TYPE != 0 {
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
        IDENTITY_READ_WINDOW, ISO15765_FRAME_PAD, ISO15765_PROTOCOL_ID, J2534Failure, J2534Message,
        MAX_READ_MESSAGES, PassThruApi, READ_WINDOW, ReadBatch, build_obd_flow_control_filters,
        classify_read_batch, decode_mode01_batch, decode_mode09_api_error_messages,
        decode_mode09_batch, matching_physical_reply, mode01_request_id, mode09_request_id,
        no_response_diagnostic, no_response_mode09_diagnostic, timeout_left_messages_unfilled,
    };
    use lognook_j2534_protocol::{
        J2534Identity, Mode01Payload, Mode09Response, WorkerErrorKind, is_supported_mode09_pid,
    };
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
            let started = Instant::now();
            let mut request = J2534Message::default();
            request.protocol_id = ISO15765_PROTOCOL_ID;
            request.tx_flags = ISO15765_FRAME_PAD;
            request.data_size = 6;
            request.data[..4].copy_from_slice(&mode09_request_id().to_be_bytes());
            request.data[4..6].copy_from_slice(&[0x09, pid]);
            let mut written = 1;
            let status =
                unsafe { (self.write_messages)(channel, &mut request, &mut written, timeout_ms) };
            self.check("PassThruWriteMsgs(Mode09)", status)?;
            if written != 1 {
                return Err(J2534Failure::new(
                    WorkerErrorKind::PassThruFailed,
                    "PassThruWriteMsgs did not accept the single fixed Mode 09 identity request",
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
                        responses.extend(decode_mode09_api_error_messages(
                            &messages, count, latency_us,
                        ));
                        read_failure = Some(J2534Failure::new(
                            kind,
                            "PassThruReadMsgs returned an invalid Mode 09 message count",
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
                        responses.extend(decode_mode09_api_error_messages(
                            &messages, count, latency_us,
                        ));
                        read_failure = Some(
                            self.check("PassThruReadMsgs(Mode09)", status)
                                .expect_err("ApiError always carries a failing J2534 status"),
                        );
                        break;
                    }
                };
                let latency_us = *first_latency.get_or_insert_with(|| {
                    started.elapsed().as_micros().min(u64::MAX as u128) as u64
                });
                responses.extend(decode_mode09_batch(&messages[..batch_count], latency_us));
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
                    no_response_mode09_diagnostic(pid, started.elapsed().as_millis()),
                ));
            }
            Ok(responses)
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
        ERR_BUFFER_EMPTY, ERR_TIMEOUT, ISO15765_PROTOCOL_ID, J2534Message, RX_STATUS_TX_MSG_TYPE,
        ReadBatch, build_obd_flow_control_filters, classify_read_batch, decode_mode01_batch,
        decode_mode01_message, decode_mode09_api_error_messages, decode_mode09_batch,
        decode_mode09_message, matching_physical_reply, mode01_request_id, mode09_request_id,
        no_response_diagnostic, no_response_mode09_diagnostic, retain_mode09_failure,
        timeout_left_messages_unfilled,
    };
    use lognook_j2534_protocol::{Mode09Payload, Mode09Response, WorkerErrorKind};

    fn rpm_message() -> J2534Message {
        let mut message = J2534Message::default();
        message.protocol_id = ISO15765_PROTOCOL_ID;
        message.data[..8].copy_from_slice(&[0x00, 0x00, 0x07, 0xE8, 0x41, 0x0C, 0x1A, 0xF8]);
        message.data_size = 8;
        message
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
