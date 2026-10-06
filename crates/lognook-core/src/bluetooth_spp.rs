use crate::LogNookError;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub const SERIAL_PORT_SERVICE_CLASS_UUID: u128 = 0x0000_1101_0000_1000_8000_0080_5f9b_34fb;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct BluetoothSppDevice {
    pub address: String,
    pub name: String,
}

pub trait BluetoothByteStream: Read + Write + Send {}
impl<T: Read + Write + Send> BluetoothByteStream for T {}

pub fn parse_bluetooth_address(address: &str) -> Result<u64, LogNookError> {
    let digits = address.replace([':', '-'], "");
    if digits.len() != 12 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(LogNookError::new(
            "Bluetooth address must contain six hexadecimal bytes",
        ));
    }
    u64::from_str_radix(&digits, 16)
        .map_err(|error| LogNookError::new(format!("Invalid Bluetooth address: {error}")))
}

pub fn format_bluetooth_address(address: u64) -> String {
    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        (address >> 40) as u8,
        (address >> 32) as u8,
        (address >> 24) as u8,
        (address >> 16) as u8,
        (address >> 8) as u8,
        address as u8,
    )
}

#[cfg(windows)]
mod platform {
    use super::{
        BluetoothByteStream, BluetoothSppDevice, SERIAL_PORT_SERVICE_CLASS_UUID,
        format_bluetooth_address, parse_bluetooth_address,
    };
    use crate::LogNookError;
    use std::{
        io::{self, Read, Write},
        mem::size_of,
        sync::OnceLock,
    };
    use windows_sys::Win32::{
        Devices::Bluetooth::{
            AF_BTH, BLUETOOTH_DEVICE_INFO, BLUETOOTH_DEVICE_SEARCH_PARAMS, BTHPROTO_RFCOMM,
            BluetoothFindDeviceClose, BluetoothFindFirstDevice, BluetoothFindNextDevice,
            SOCKADDR_BTH,
        },
        Networking::WinSock::{
            INVALID_SOCKET, SO_RCVTIMEO, SOCK_STREAM, SOCKADDR, SOCKET, SOL_SOCKET, WSADATA,
            WSAGetLastError, WSAStartup, closesocket, connect, recv, send, setsockopt, socket,
        },
    };

    static WINSOCK: OnceLock<Result<(), String>> = OnceLock::new();

    fn initialize_winsock() -> Result<(), LogNookError> {
        WINSOCK
            .get_or_init(|| {
                let mut data = WSADATA::default();
                let result = unsafe { WSAStartup(0x0202, &mut data) };
                if result == 0 {
                    Ok(())
                } else {
                    Err(format!("Bluetooth socket initialization failed ({result})"))
                }
            })
            .as_ref()
            .map(|()| ())
            .map_err(|error| LogNookError::new(error.clone()))
    }

    pub(super) fn paired_spp_devices() -> Result<Vec<BluetoothSppDevice>, LogNookError> {
        let mut search = BLUETOOTH_DEVICE_SEARCH_PARAMS {
            dwSize: size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
            fReturnAuthenticated: 1,
            fReturnRemembered: 1,
            fReturnConnected: 1,
            ..Default::default()
        };
        search.fIssueInquiry = 0;
        let mut info = BLUETOOTH_DEVICE_INFO {
            dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
            ..Default::default()
        };
        let handle = unsafe { BluetoothFindFirstDevice(&search, &mut info) };
        if handle.is_null() {
            return Ok(Vec::new());
        }

        let mut devices = Vec::new();
        loop {
            let address = unsafe { info.Address.Anonymous.ullLong } & 0x0000_FFFF_FFFF_FFFF;
            let name_len = info
                .szName
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(info.szName.len());
            let name = String::from_utf16_lossy(&info.szName[..name_len]);
            if address != 0 && info.fAuthenticated != 0 {
                devices.push(BluetoothSppDevice {
                    address: format_bluetooth_address(address),
                    name,
                });
            }
            info = BLUETOOTH_DEVICE_INFO {
                dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
                ..Default::default()
            };
            if unsafe { BluetoothFindNextDevice(handle, &mut info) } == 0 {
                break;
            }
        }
        unsafe { BluetoothFindDeviceClose(handle) };
        devices.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.address.cmp(&right.address))
        });
        devices.dedup_by(|left, right| left.address == right.address);
        Ok(devices)
    }

    pub(super) fn connect_bluetooth_spp(
        address: &str,
    ) -> Result<Box<dyn BluetoothByteStream>, LogNookError> {
        initialize_winsock()?;
        let address = parse_bluetooth_address(address)?;
        let socket = unsafe { socket(AF_BTH as i32, SOCK_STREAM, BTHPROTO_RFCOMM as i32) };
        if socket == INVALID_SOCKET {
            return Err(last_socket_error(
                "Could not create Bluetooth RFCOMM socket",
            ));
        }
        let receive_timeout_ms = 100_u32;
        let timeout_result = unsafe {
            setsockopt(
                socket,
                SOL_SOCKET,
                SO_RCVTIMEO,
                (&receive_timeout_ms as *const u32).cast(),
                size_of::<u32>() as i32,
            )
        };
        if timeout_result == -1 {
            let error = last_socket_error("Could not configure Bluetooth receive timeout");
            unsafe { closesocket(socket) };
            return Err(error);
        }
        let target = SOCKADDR_BTH {
            addressFamily: AF_BTH,
            btAddr: address,
            serviceClassId: windows_sys::core::GUID::from_u128(SERIAL_PORT_SERVICE_CLASS_UUID),
            port: 0,
        };
        let result = unsafe {
            connect(
                socket,
                (&target as *const SOCKADDR_BTH).cast::<SOCKADDR>(),
                size_of::<SOCKADDR_BTH>() as i32,
            )
        };
        if result == -1 {
            let error = last_socket_error("Could not connect to paired Bluetooth SPP device");
            unsafe { closesocket(socket) };
            return Err(error);
        }
        Ok(Box::new(RfcommStream { socket }))
    }

    fn last_socket_error(context: &str) -> LogNookError {
        let code = unsafe { WSAGetLastError() };
        LogNookError::new(format!("{context}: Winsock error {code}"))
    }

    struct RfcommStream {
        socket: SOCKET,
    }

    impl Read for RfcommStream {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if buffer.is_empty() {
                return Ok(0);
            }
            let length = buffer.len().min(i32::MAX as usize) as i32;
            let read = unsafe { recv(self.socket, buffer.as_mut_ptr(), length, 0) };
            if read >= 0 {
                Ok(read as usize)
            } else {
                Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }))
            }
        }
    }

    impl Write for RfcommStream {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if buffer.is_empty() {
                return Ok(0);
            }
            let length = buffer.len().min(i32::MAX as usize) as i32;
            let written = unsafe { send(self.socket, buffer.as_ptr(), length, 0) };
            if written >= 0 {
                Ok(written as usize)
            } else {
                Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }))
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Drop for RfcommStream {
        fn drop(&mut self) {
            unsafe { closesocket(self.socket) };
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{BluetoothByteStream, BluetoothSppDevice};
    use crate::LogNookError;

    pub(super) fn paired_spp_devices() -> Result<Vec<BluetoothSppDevice>, LogNookError> {
        Err(LogNookError::new(
            "Bluetooth Classic SPP is currently supported on Windows only",
        ))
    }

    pub(super) fn connect_bluetooth_spp(
        _address: &str,
    ) -> Result<Box<dyn BluetoothByteStream>, LogNookError> {
        Err(LogNookError::new(
            "Bluetooth Classic SPP is currently supported on Windows only",
        ))
    }
}

pub fn paired_spp_devices() -> Result<Vec<BluetoothSppDevice>, LogNookError> {
    platform::paired_spp_devices()
}

pub fn connect_bluetooth_spp(address: &str) -> Result<Box<dyn BluetoothByteStream>, LogNookError> {
    platform::connect_bluetooth_spp(address)
}

#[cfg(test)]
mod tests {
    use super::{
        SERIAL_PORT_SERVICE_CLASS_UUID, format_bluetooth_address, parse_bluetooth_address,
    };

    #[test]
    fn bluetooth_address_round_trips_windows_and_capture_mac_formats() {
        let address = parse_bluetooth_address("AA:BB:CC:DD:EE:FF").unwrap();
        assert_eq!(address, 0xAABBCCDDEEFF);
        assert_eq!(format_bluetooth_address(address), "AA:BB:CC:DD:EE:FF");
        assert_eq!(parse_bluetooth_address("AABBCCDDEEFF").unwrap(), address);
        assert_eq!(
            parse_bluetooth_address("aa-bb-cc-dd-ee-ff").unwrap(),
            address
        );
    }

    #[test]
    fn bluetooth_address_rejects_bad_length_and_non_hex_input() {
        assert!(parse_bluetooth_address("AA:BB:CC").is_err());
        assert!(parse_bluetooth_address("AA:BB:CC:DD:EE:GG").is_err());
    }

    #[test]
    fn uses_the_standard_bluetooth_serial_port_service_uuid() {
        assert_eq!(
            SERIAL_PORT_SERVICE_CLASS_UUID,
            0x0000_1101_0000_1000_8000_0080_5f9b_34fb
        );
    }
}
