use lognook_j2534_protocol::{J2534Identity, Mode01Payload, WorkerErrorKind};
use std::fmt;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
use std::time::Duration;

pub(crate) const ISO15765_PROTOCOL_ID: u32 = 6;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
const ISO15765_FRAME_PAD: u32 = 0x40;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
const PASS_FILTER: u32 = 0x01;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
const ERR_TIMEOUT: u32 = 0x09;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
const ERR_BUFFER_EMPTY: u32 = 0x10;
const MESSAGE_DATA_SIZE: usize = 4128;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
const MAX_READ_MESSAGES: usize = 8;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
const READ_WINDOW: Duration = Duration::from_millis(250);

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
    ) -> Result<Vec<Mode01Payload>, J2534Failure>;
    fn disconnect(&mut self, channel: u32) -> Result<(), J2534Failure>;
    fn close_device(&mut self, device: u32) -> Result<(), J2534Failure>;
}

pub(crate) fn decode_mode01_message(
    message: &J2534Message,
    requested_pid: u8,
    latency_us: u64,
) -> Result<Mode01Payload, WorkerErrorKind> {
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

    Ok(Mode01Payload {
        ecu_id: format!("{can_id:03X}"),
        bytes: payload.to_vec(),
        latency_us,
    })
}

#[cfg(all(target_os = "windows", target_arch = "x86"))]
mod dynamic {
    use super::{
        ERR_BUFFER_EMPTY, ERR_TIMEOUT, ISO15765_FRAME_PAD, ISO15765_PROTOCOL_ID, J2534Failure,
        J2534Message, MAX_READ_MESSAGES, PASS_FILTER, PassThruApi, READ_WINDOW,
        decode_mode01_message,
    };
    use lognook_j2534_protocol::{J2534Identity, Mode01Payload, WorkerErrorKind};
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
            let mut mask = J2534Message::default();
            mask.protocol_id = ISO15765_PROTOCOL_ID;
            mask.data_size = 4;
            mask.data[..4].copy_from_slice(&[0x00, 0x00, 0x07, 0xF8]);

            let mut pattern = J2534Message::default();
            pattern.protocol_id = ISO15765_PROTOCOL_ID;
            pattern.data_size = 4;
            pattern.data[..4].copy_from_slice(&[0x00, 0x00, 0x07, 0xE8]);

            let mut filter_id = 0;
            let status = unsafe {
                (self.start_filter)(
                    channel,
                    PASS_FILTER,
                    &mut mask,
                    &mut pattern,
                    ptr::null_mut(),
                    &mut filter_id,
                )
            };
            self.check_with_kind(
                WorkerErrorKind::ChannelFailed,
                "PassThruStartMsgFilter(PASS)",
                status,
            )
        }

        fn read_mode01(
            &mut self,
            channel: u32,
            pid: u8,
            timeout_ms: u32,
        ) -> Result<Vec<Mode01Payload>, J2534Failure> {
            let started = Instant::now();
            let mut request = J2534Message::default();
            request.protocol_id = ISO15765_PROTOCOL_ID;
            request.tx_flags = ISO15765_FRAME_PAD;
            request.data_size = 6;
            request.data[..6].copy_from_slice(&[0x00, 0x00, 0x07, 0xDF, 0x01, pid]);
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
                if status == ERR_TIMEOUT {
                    break;
                }
                if status == ERR_BUFFER_EMPTY {
                    if Instant::now() >= deadline {
                        break;
                    }
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                self.check("PassThruReadMsgs(Mode01)", status)?;
                if count == 0 || count as usize > MAX_READ_MESSAGES {
                    return Err(J2534Failure::new(
                        WorkerErrorKind::MalformedResponse,
                        "PassThruReadMsgs returned an invalid message count",
                    ));
                }
                let latency_us = first_latency.get_or_insert_with(|| {
                    started.elapsed().as_micros().min(u64::MAX as u128) as u64
                });
                for message in messages.iter().take(count as usize) {
                    let response =
                        decode_mode01_message(message, pid, *latency_us).map_err(|kind| {
                            J2534Failure::new(
                                kind,
                                "J2534 returned an invalid ISO15765 OBD response",
                            )
                        })?;
                    responses.push(response);
                }
            }
            if responses.is_empty() {
                return Err(J2534Failure::new(
                    WorkerErrorKind::NoResponse,
                    format!("No response for standard Mode 01 PID {pid:02X}"),
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
    use super::{ISO15765_PROTOCOL_ID, J2534Message, decode_mode01_message};
    use lognook_j2534_protocol::WorkerErrorKind;

    fn rpm_message() -> J2534Message {
        let mut message = J2534Message::default();
        message.protocol_id = ISO15765_PROTOCOL_ID;
        message.data[..8].copy_from_slice(&[0x00, 0x00, 0x07, 0xE8, 0x41, 0x0C, 0x1A, 0xF8]);
        message.data_size = 8;
        message
    }

    #[test]
    fn j2534_message_matches_the_v0404_buffer_layout() {
        assert_eq!(std::mem::size_of::<J2534Message>(), 4152);
    }

    #[test]
    fn parses_iso15765_mode01_response_with_ecu_identity() {
        let payload = decode_mode01_message(&rpm_message(), 0x0C, 11_300).unwrap();
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
