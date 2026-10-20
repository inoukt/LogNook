//! CandleLight GS_USB classic CAN transport. USB IDs are hints; identity requires
//! valid interface descriptors and device/timing control replies. All USB I/O is
//! confined to one worker. Opening never starts CAN or sends a CAN frame.
//!
//! Wire reference: Linux drivers/net/can/usb/gs_usb.c and CandleLight
//! include/gs_usb.h, src/usbd_gs_can.c. Fields are little endian; host-format
//! negotiation is intentionally omitted for CandleLight firmware.
use crate::{CanDirection, CanFrame};
use nusb::{
    DeviceInfo, Endpoint, Interface, MaybeFuture,
    transfer::{
        Buffer, Bulk, ControlIn, ControlOut, ControlType, In, Out, Recipient, TransferError,
    },
};
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    sync::{Mutex, OnceLock, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const RX_ECHO_ID: u32 = u32::MAX;
const CAN_EFF_FLAG: u32 = 0x8000_0000;
const CAN_RTR_FLAG: u32 = 0x4000_0000;
const CAN_ERR_FLAG: u32 = 0x2000_0000;
const FEATURE_LISTEN_ONLY: u32 = 1;
const FEATURE_BERR_REPORTING: u32 = 1 << 12;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandlelightError {
    Timeout,
    Disconnected,
    ListenOnlyUnavailable,
    NotStarted,
    PassiveTransmit,
    BusOff(Vec<u8>),
    ControllerError(Vec<u8>),
    FrameError {
        raw_usb: Vec<u8>,
        diagnostic: Box<CandlelightError>,
    },
    UnsupportedFrame(String),
    Protocol(String),
    Usb(String),
    ChannelBusy,
    Worker(String),
}
impl fmt::Display for CandlelightError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => f.write_str("Candlelight USB timeout"),
            Self::Disconnected => f.write_str("Candlelight USB device disconnected"),
            Self::ListenOnlyUnavailable => {
                f.write_str("Candlelight does not advertise listen-only mode")
            }
            Self::NotStarted => f.write_str("Candlelight CAN channel is stopped"),
            Self::PassiveTransmit => {
                f.write_str("Candlelight transmit is unavailable in listen-only mode")
            }
            Self::BusOff(_) => f.write_str("Candlelight CAN bus-off"),
            Self::ControllerError(_) => f.write_str("Candlelight CAN controller error"),
            Self::FrameError { diagnostic, .. } => diagnostic.fmt(f),
            Self::UnsupportedFrame(s) | Self::Protocol(s) | Self::Usb(s) | Self::Worker(s) => {
                f.write_str(s)
            }
            Self::ChannelBusy => f.write_str(
                "Candlelight USB interface/channel is already owned by another LogNook source. Disconnect that source before identifying or connecting this channel.",
            ),
        }
    }
}
impl CandlelightError {
    /// Exact received USB evidence, including malformed packets and CAN errors.
    pub fn raw_usb(&self) -> Option<&[u8]> {
        match self {
            Self::BusOff(raw)
            | Self::ControllerError(raw)
            | Self::FrameError { raw_usb: raw, .. } => Some(raw),
            _ => None,
        }
    }
    /// Underlying typed outcome, independent of retained packet evidence.
    pub fn diagnostic(&self) -> &Self {
        match self {
            Self::FrameError { diagnostic, .. } => diagnostic.diagnostic(),
            other => other,
        }
    }
    fn with_raw(self, raw: &[u8]) -> Self {
        if self.raw_usb().is_some() {
            self
        } else {
            Self::FrameError {
                raw_usb: raw.to_vec(),
                diagnostic: Box::new(self),
            }
        }
    }
}
impl std::error::Error for CandlelightError {}
impl From<TransferError> for CandlelightError {
    fn from(error: TransferError) -> Self {
        match error {
            // Only bounded blocking transfers are used; no external cancellation.
            TransferError::Cancelled => Self::Timeout,
            TransferError::Disconnected => Self::Disconnected,
            other => Self::Usb(format!("Candlelight USB: {other}")),
        }
    }
}
fn usb_error(error: nusb::Error) -> CandlelightError {
    if error.kind() == nusb::ErrorKind::Disconnected {
        CandlelightError::Disconnected
    } else {
        CandlelightError::Usb(format!("Candlelight USB: {error}"))
    }
}
fn protocol(message: &str) -> CandlelightError {
    CandlelightError::Protocol(message.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GsUsbRequest {
    HostFormat = 0,
    BitTiming = 1,
    Mode = 2,
    BusError = 3,
    TimingConstants = 4,
    DeviceConfig = 5,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlSetup {
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub vendor: bool,
    pub interface_recipient: bool,
}
pub fn control_setup(request: GsUsbRequest, interface: u8, channel: u8) -> ControlSetup {
    ControlSetup {
        request: request as u8,
        value: if matches!(
            request,
            GsUsbRequest::HostFormat | GsUsbRequest::DeviceConfig
        ) {
            1
        } else {
            u16::from(channel)
        },
        index: u16::from(interface),
        vendor: true,
        interface_recipient: true,
    }
}
impl ControlSetup {
    pub fn input(self, length: u16) -> ControlIn {
        ControlIn {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: self.request,
            value: self.value,
            index: self.index,
            length,
        }
    }
    pub fn output(self, data: &[u8]) -> ControlOut<'_> {
        ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: self.request,
            value: self.value,
            index: self.index,
            data,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EndpointDescriptor {
    pub address: u8,
    pub transfer_type: u8,
    pub max_packet_size: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BulkEndpoints {
    pub input: u8,
    pub output: u8,
}
pub fn select_bulk_endpoints(
    descriptors: &[EndpointDescriptor],
) -> Result<BulkEndpoints, CandlelightError> {
    let mut input = None;
    let mut output = None;
    for ep in descriptors.iter().filter(|ep| ep.transfer_type == 2) {
        if ep.address & 0x0F == 0 || ep.address & 0x70 != 0 || ep.max_packet_size == 0 {
            return Err(protocol("Invalid GS_USB bulk endpoint descriptor"));
        }
        let slot = if ep.address & 0x80 != 0 {
            &mut input
        } else {
            &mut output
        };
        if slot.replace(ep.address).is_some() {
            return Err(protocol("Ambiguous GS_USB bulk endpoints"));
        }
    }
    Ok(BulkEndpoints {
        input: input.ok_or_else(|| protocol("GS_USB bulk IN endpoint missing"))?,
        output: output.ok_or_else(|| protocol("GS_USB bulk OUT endpoint missing"))?,
    })
}
pub fn is_candlelight_hint(vendor: u16, product: u16) -> bool {
    matches!((vendor, product), (0x1D50, 0x606F) | (0x1209, 0x2323))
}
fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated wire length"),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GsUsbDeviceConfig {
    pub channel_count: u16,
    pub software_version: u32,
    pub hardware_version: u32,
}
impl GsUsbDeviceConfig {
    pub fn decode(bytes: &[u8]) -> Result<Self, CandlelightError> {
        if bytes.len() != 12 {
            return Err(protocol("GS_USB device config must contain 12 bytes"));
        }
        Ok(Self {
            channel_count: u16::from(bytes[3]) + 1,
            software_version: word(bytes, 4),
            hardware_version: word(bytes, 8),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GsUsbTimingConstants {
    pub features: u32,
    pub clock_hz: u32,
    pub tseg1_min: u32,
    pub tseg1_max: u32,
    pub tseg2_min: u32,
    pub tseg2_max: u32,
    pub sjw_max: u32,
    pub brp_min: u32,
    pub brp_max: u32,
    pub brp_inc: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GsUsbBitTiming {
    pub prop_seg: u32,
    pub phase_seg1: u32,
    pub phase_seg2: u32,
    pub sjw: u32,
    pub brp: u32,
}
impl GsUsbBitTiming {
    pub fn encode(self) -> Vec<u8> {
        [
            self.prop_seg,
            self.phase_seg1,
            self.phase_seg2,
            self.sjw,
            self.brp,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect()
    }
}
impl GsUsbTimingConstants {
    pub fn decode(bytes: &[u8]) -> Result<Self, CandlelightError> {
        if bytes.len() != 40 {
            return Err(protocol("GS_USB timing constants must contain 40 bytes"));
        }
        let result = Self {
            features: word(bytes, 0),
            clock_hz: word(bytes, 4),
            tseg1_min: word(bytes, 8),
            tseg1_max: word(bytes, 12),
            tseg2_min: word(bytes, 16),
            tseg2_max: word(bytes, 20),
            sjw_max: word(bytes, 24),
            brp_min: word(bytes, 28),
            brp_max: word(bytes, 32),
            brp_inc: word(bytes, 36),
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), CandlelightError> {
        if self.clock_hz == 0 || self.tseg1_min == 0 || self.tseg2_min == 0 || self.sjw_max == 0 || self.brp_min == 0 || self.brp_inc == 0
            || self.tseg1_min > self.tseg1_max || self.tseg2_min > self.tseg2_max || self.brp_min > self.brp_max
            // Bound work on untrusted control replies. These exceed classic CAN controller limits.
            || self.tseg1_max > 65_535 || self.tseg2_max > 65_535 || self.brp_max > 65_535
        {
            return Err(protocol("Invalid GS_USB timing limits"));
        }
        Ok(())
    }
    /// Exact bitrate only; choose the closest sample point (87.5% <=500 kbit/s,
    /// 80% <=800 kbit/s, 75% above), preferring more time quanta on ties.
    pub fn for_bitrate(&self, bitrate: u32) -> Result<GsUsbBitTiming, CandlelightError> {
        self.validate()?;
        if bitrate == 0 || bitrate > 1_000_000 {
            return Err(protocol("Classic CAN bitrate must be 1-1000000 bit/s"));
        }
        let sample_per_mille = if bitrate <= 500_000 {
            875u64
        } else if bitrate <= 800_000 {
            800
        } else {
            750
        };
        let mut best: Option<(u64, u32, GsUsbBitTiming)> = None;
        for brp in (self.brp_min..=self.brp_max).step_by(self.brp_inc as usize) {
            let divisor = u64::from(brp) * u64::from(bitrate);
            if u64::from(self.clock_hz) % divisor != 0 {
                continue;
            }
            let tq = (u64::from(self.clock_hz) / divisor) as u32;
            if tq < 1 + self.tseg1_min + self.tseg2_min || tq > 1 + self.tseg1_max + self.tseg2_max
            {
                continue;
            }
            let low = self.tseg2_min.max(tq.saturating_sub(1 + self.tseg1_max));
            let high = self.tseg2_max.min(tq - 1 - self.tseg1_min);
            let ideal = (u64::from(tq) * (1000 - sample_per_mille)) / 1000;
            for tseg2 in [ideal as u32, ideal as u32 + 1].map(|n| n.clamp(low, high)) {
                let tseg1 = tq - 1 - tseg2;
                let error = (u64::from(tq - tseg2) * 1_000_000 / u64::from(tq))
                    .abs_diff(sample_per_mille * 1000);
                let timing = GsUsbBitTiming {
                    prop_seg: tseg1 / 2,
                    phase_seg1: tseg1 - tseg1 / 2,
                    phase_seg2: tseg2,
                    sjw: 1,
                    brp,
                };
                if best.is_none_or(|(e, quanta, _)| error < e || (error == e && tq > quanta)) {
                    best = Some((error, tq, timing));
                }
            }
        }
        best.map(|(_, _, timing)| timing).ok_or_else(|| protocol("Requested bitrate cannot be represented exactly by advertised GS_USB timing limits"))
    }
}
pub fn reset_mode_bytes() -> [u8; 8] {
    [0; 8]
}
pub fn mode_bytes(features: u32, listen_only: bool) -> Result<[u8; 8], CandlelightError> {
    if listen_only && features & FEATURE_LISTEN_ONLY == 0 {
        return Err(CandlelightError::ListenOnlyUnavailable);
    }
    let flags = u32::from(listen_only) | (features & FEATURE_BERR_REPORTING);
    let mut bytes = [0; 8];
    bytes[..4].copy_from_slice(&1u32.to_le_bytes());
    bytes[4..].copy_from_slice(&flags.to_le_bytes());
    Ok(bytes)
}

#[derive(Clone, Debug, PartialEq)]
pub struct GsUsbFrame {
    pub frame: CanFrame,
    pub channel: u8,
    pub echo_id: u32,
    pub raw_usb: Vec<u8>,
}
pub fn encode_frame(
    frame: &CanFrame,
    echo_id: u32,
    channel: u8,
) -> Result<Vec<u8>, CandlelightError> {
    frame.validate().map_err(|e| protocol(&e.to_string()))?;
    if echo_id == RX_ECHO_ID {
        return Err(protocol("TX echo ID must differ from GS_USB RX marker"));
    }
    if frame.flags != 0 {
        return Err(CandlelightError::UnsupportedFrame(
            "Only classic CAN data transmission is supported".into(),
        ));
    }
    let mut bytes = vec![0; 20];
    bytes[..4].copy_from_slice(&echo_id.to_le_bytes());
    bytes[4..8].copy_from_slice(
        &(frame.can_id | if frame.extended { CAN_EFF_FLAG } else { 0 }).to_le_bytes(),
    );
    bytes[8] = frame.data.len() as u8;
    bytes[9] = channel;
    bytes[12..12 + frame.data.len()].copy_from_slice(&frame.data);
    Ok(bytes)
}
pub fn decode_frame(
    bytes: &[u8],
    elapsed_ns: i64,
    wall_time_unix_ns: i64,
) -> Result<GsUsbFrame, CandlelightError> {
    decode_frame_inner(bytes, elapsed_ns, wall_time_unix_ns).map_err(|error| error.with_raw(bytes))
}
fn decode_frame_inner(
    bytes: &[u8],
    elapsed_ns: i64,
    wall_time_unix_ns: i64,
) -> Result<GsUsbFrame, CandlelightError> {
    // No hardware timestamp, padding or CAN FD mode is enabled by this transport.
    if !(12..=20).contains(&bytes.len()) {
        return Err(protocol(
            "GS_USB classic frame must contain a 12-byte header and at most 8 data bytes",
        ));
    }
    let echo_id = word(bytes, 0);
    if echo_id != RX_ECHO_ID {
        // Only echo_id and channel identify a TX confirmation. CAN ID, DLC,
        // flags and payload cannot describe an RX error. Overflow applies to
        // both RX and TX packets; retain only this recognized indication.
        // The USB owner checks the assigned ID/channel and restores its TX frame.
        let frame = CanFrame {
            elapsed_ns,
            wall_time_unix_ns,
            can_id: 0,
            extended: false,
            direction: CanDirection::Tx,
            data: Vec::new(),
            flags: u32::from(bytes[10] & 1),
        };
        frame.validate().map_err(|e| protocol(&e.to_string()))?;
        return Ok(GsUsbFrame {
            frame,
            channel: bytes[9],
            echo_id,
            raw_usb: bytes.to_vec(),
        });
    }
    let raw_id = word(bytes, 4);
    let dlc = usize::from(bytes[8]);
    if dlc > 8 {
        return Err(protocol("GS_USB classic CAN DLC exceeds 8"));
    }
    if bytes[10] & !1 != 0 {
        return Err(CandlelightError::UnsupportedFrame(
            "GS_USB CAN FD/unknown frame flags are unsupported".into(),
        ));
    }
    if raw_id & CAN_ERR_FLAG != 0 {
        if bytes.len() < 20 || dlc != 8 {
            return Err(protocol("Malformed GS_USB CAN error frame"));
        }
        return Err(if raw_id & 0x40 != 0 {
            CandlelightError::BusOff(bytes.to_vec())
        } else {
            CandlelightError::ControllerError(bytes.to_vec())
        });
    }
    if raw_id & CAN_RTR_FLAG != 0 {
        return Err(CandlelightError::UnsupportedFrame(
            "GS_USB remote frames cannot be represented by classic data CanFrame".into(),
        ));
    }
    let data_len = dlc;
    if bytes.len() < 12 + data_len {
        return Err(protocol("GS_USB frame payload is shorter than DLC"));
    }
    let frame = CanFrame {
        elapsed_ns,
        wall_time_unix_ns,
        can_id: raw_id & 0x1FFF_FFFF,
        extended: raw_id & CAN_EFF_FLAG != 0,
        direction: CanDirection::Rx,
        data: bytes[12..12 + data_len].to_vec(),
        flags: u32::from(bytes[10]),
    };
    frame.validate().map_err(|e| protocol(&e.to_string()))?;
    Ok(GsUsbFrame {
        frame,
        channel: bytes[9],
        echo_id,
        raw_usb: bytes.to_vec(),
    })
}

/// Unverified USB candidate; opening performs descriptor/control identity checks.
#[derive(Clone, Debug)]
pub struct CandlelightDevice {
    pub usb: DeviceInfo,
}

static CLAIMED_CHANNELS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

#[derive(Debug)]
struct CandlelightChannelLease {
    key: String,
}

impl CandlelightChannelLease {
    fn claim(key: String) -> Result<Self, CandlelightError> {
        let claimed = CLAIMED_CHANNELS.get_or_init(|| Mutex::new(HashSet::new()));
        let mut claimed = claimed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !claimed.insert(key.clone()) {
            return Err(CandlelightError::ChannelBusy);
        }
        Ok(Self { key })
    }
}

impl Drop for CandlelightChannelLease {
    fn drop(&mut self) {
        if let Some(claimed) = CLAIMED_CHANNELS.get() {
            claimed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&self.key);
        }
    }
}

fn channel_lease_key(device: &DeviceInfo, interface_number: u8, channel: u8) -> String {
    let device_path = device.serial_number().map_or_else(
        || format!("{}:{}", device.bus_id(), device.device_address()),
        |serial| format!("serial:{serial}:bus:{}", device.bus_id()),
    );
    format!(
        "{:04X}:{:04X}:{device_path}:{interface_number}:{channel}",
        device.vendor_id(),
        device.product_id()
    )
}
pub fn available_candlelight_devices() -> Result<Vec<CandlelightDevice>, CandlelightError> {
    thread::Builder::new()
        .name("candlelight-enumerate".into())
        .spawn(|| {
            Ok(nusb::list_devices()
                .wait()
                .map_err(usb_error)?
                .filter(|d| is_candlelight_hint(d.vendor_id(), d.product_id()))
                .map(|usb| CandlelightDevice { usb })
                .collect())
        })
        .map_err(|e| CandlelightError::Worker(e.to_string()))?
        .join()
        .map_err(|_| CandlelightError::Worker("Candlelight enumeration worker panicked".into()))?
}
#[derive(Clone, Debug)]
pub struct CandlelightIdentity {
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial_number: Option<String>,
    pub product: Option<String>,
    pub interface_number: u8,
    pub channel: u8,
    pub endpoints: BulkEndpoints,
    pub config: GsUsbDeviceConfig,
    pub timing: GsUsbTimingConstants,
}

enum Command {
    Start {
        bitrate: u32,
        listen_only: bool,
        session_origin: Instant,
        reply: mpsc::Sender<Result<(), CandlelightError>>,
    },
    Stop(mpsc::Sender<Result<(), CandlelightError>>),
    Receive {
        timeout: Duration,
        reply: mpsc::Sender<Result<GsUsbFrame, CandlelightError>>,
    },
    Transmit {
        frame: CanFrame,
        timeout: Duration,
        reply: mpsc::Sender<Result<u32, CandlelightError>>,
    },
    Shutdown,
}
/// A non-cloneable client for a single claimed interface/endpoint worker.
/// The later Workbench manager must ensure one transport per physical adapter.
pub struct CandlelightTransport {
    identity: CandlelightIdentity,
    _channel_lease: CandlelightChannelLease,
    commands: mpsc::Sender<Command>,
    worker: Option<JoinHandle<()>>,
    communication_sink: Option<crate::communication_trace::CommunicationSink>,
}
impl CandlelightTransport {
    pub(crate) fn set_communication_sink(
        &mut self,
        sink: crate::communication_trace::CommunicationSink,
    ) {
        self.communication_sink = Some(sink);
    }

    pub(crate) fn communication_sink(
        &self,
    ) -> Option<&crate::communication_trace::CommunicationSink> {
        self.communication_sink.as_ref()
    }

    /// Claim and identify only. `start` explicitly configures bitrate and CAN mode.
    pub fn open(
        device: CandlelightDevice,
        interface_number: u8,
        channel: u8,
    ) -> Result<Self, CandlelightError> {
        let channel_lease = CandlelightChannelLease::claim(channel_lease_key(
            &device.usb,
            interface_number,
            channel,
        ))?;
        let (commands, requests) = mpsc::channel();
        let (ready, result) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("candlelight-usb".into())
            .spawn(
                move || match UsbOwner::open(device.usb, interface_number, channel) {
                    Ok(mut owner) => {
                        if ready.send(Ok(owner.identity.clone())).is_err() {
                            return;
                        }
                        while let Ok(command) = requests.recv() {
                            match command {
                                Command::Start {
                                    bitrate,
                                    listen_only,
                                    session_origin,
                                    reply,
                                } => {
                                    let _ = reply.send(owner.start(
                                        bitrate,
                                        listen_only,
                                        session_origin,
                                    ));
                                }
                                Command::Stop(reply) => {
                                    let _ = reply.send(owner.stop());
                                }
                                Command::Receive { timeout, reply } => {
                                    let _ = reply.send(owner.receive(timeout));
                                }
                                Command::Transmit {
                                    frame,
                                    timeout,
                                    reply,
                                } => {
                                    let _ = reply.send(owner.transmit(frame, timeout));
                                }
                                Command::Shutdown => break,
                            }
                        }
                        if owner.needs_reset {
                            let _ = owner.stop();
                        }
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                    }
                },
            )
            .map_err(|e| CandlelightError::Worker(e.to_string()))?;
        match result.recv() {
            Ok(Ok(identity)) => Ok(Self {
                identity,
                _channel_lease: channel_lease,
                commands,
                worker: Some(worker),
                communication_sink: None,
            }),
            other => {
                let _ = worker.join();
                Err(other
                    .unwrap_or(Err(CandlelightError::Disconnected))
                    .unwrap_err())
            }
        }
    }
    pub fn identity(&self) -> &CandlelightIdentity {
        &self.identity
    }
    pub fn start(&mut self, bitrate: u32, listen_only: bool) -> Result<(), CandlelightError> {
        self.start_at(bitrate, listen_only, Instant::now())
    }
    /// Start using a caller-owned monotonic origin shared with its acquisition worker.
    pub fn start_at(
        &mut self,
        bitrate: u32,
        listen_only: bool,
        session_origin: Instant,
    ) -> Result<(), CandlelightError> {
        self.call(|reply| Command::Start {
            bitrate,
            listen_only,
            session_origin,
            reply,
        })
    }
    pub fn stop(&mut self) -> Result<(), CandlelightError> {
        self.call(Command::Stop)
    }
    pub fn receive(&mut self, timeout: Duration) -> Result<GsUsbFrame, CandlelightError> {
        self.call(|reply| Command::Receive { timeout, reply })
    }
    /// USB completion confirms submission; CAN transmission is confirmed only by
    /// a later TX echo from `receive`, carrying this assigned echo ID.
    pub fn transmit(
        &mut self,
        frame: CanFrame,
        timeout: Duration,
    ) -> Result<u32, CandlelightError> {
        self.call(|reply| Command::Transmit {
            frame,
            timeout,
            reply,
        })
    }
    fn call<T>(
        &mut self,
        command: impl FnOnce(mpsc::Sender<Result<T, CandlelightError>>) -> Command,
    ) -> Result<T, CandlelightError> {
        let (reply, result) = mpsc::channel();
        self.commands
            .send(command(reply))
            .map_err(|_| CandlelightError::Disconnected)?;
        result.recv().map_err(|_| CandlelightError::Disconnected)?
    }
}
impl Drop for CandlelightTransport {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
struct UsbOwner {
    interface: Interface,
    input: Endpoint<Bulk, In>,
    output: Endpoint<Bulk, Out>,
    identity: CandlelightIdentity,
    started: bool,
    needs_reset: bool,
    listen_only: bool,
    pending_tx: BTreeMap<u32, CanFrame>,
    next_echo: u32,
    started_at: Instant,
}
impl UsbOwner {
    fn open(usb: DeviceInfo, interface_number: u8, channel: u8) -> Result<Self, CandlelightError> {
        let device = usb.open().wait().map_err(usb_error)?;
        let interface = device
            .claim_interface(interface_number)
            .wait()
            .map_err(usb_error)?;
        let descriptor = interface
            .descriptor()
            .ok_or_else(|| protocol("GS_USB interface descriptor missing"))?;
        if descriptor.class() != 0xFF {
            return Err(protocol("GS_USB interface must be vendor-specific"));
        }
        let descriptors: Vec<_> = descriptor
            .endpoints()
            .map(|ep| EndpointDescriptor {
                address: ep.address(),
                transfer_type: ep.transfer_type() as u8,
                max_packet_size: ep.max_packet_size(),
            })
            .collect();
        let endpoints = select_bulk_endpoints(&descriptors)?;
        let config = GsUsbDeviceConfig::decode(
            &interface
                .control_in(
                    control_setup(GsUsbRequest::DeviceConfig, interface_number, channel).input(12),
                    CONTROL_TIMEOUT,
                )
                .wait()
                .map_err(CandlelightError::from)?,
        )?;
        if u16::from(channel) >= config.channel_count {
            return Err(protocol(
                "Selected GS_USB channel is outside device configuration",
            ));
        }
        let timing = GsUsbTimingConstants::decode(
            &interface
                .control_in(
                    control_setup(GsUsbRequest::TimingConstants, interface_number, channel)
                        .input(40),
                    CONTROL_TIMEOUT,
                )
                .wait()
                .map_err(CandlelightError::from)?,
        )?;
        // These firmware families require other packet layouts; reject instead of guessing.
        if timing.features & ((1 << 9) | (1 << 14)) != 0 {
            return Err(protocol(
                "GS_USB firmware packet quirk/protocol extension is unsupported",
            ));
        }
        let input = interface
            .endpoint::<Bulk, In>(endpoints.input)
            .map_err(usb_error)?;
        let output = interface
            .endpoint::<Bulk, Out>(endpoints.output)
            .map_err(usb_error)?;
        let identity = CandlelightIdentity {
            vendor_id: usb.vendor_id(),
            product_id: usb.product_id(),
            serial_number: usb.serial_number().map(str::to_owned),
            product: usb.product_string().map(str::to_owned),
            interface_number,
            channel,
            endpoints,
            config,
            timing,
        };
        Ok(Self {
            interface,
            input,
            output,
            identity,
            started: false,
            needs_reset: false,
            listen_only: false,
            pending_tx: BTreeMap::new(),
            next_echo: 0,
            started_at: Instant::now(),
        })
    }
    fn control_out(&self, request: GsUsbRequest, bytes: &[u8]) -> Result<(), CandlelightError> {
        self.interface
            .control_out(
                control_setup(
                    request,
                    self.identity.interface_number,
                    self.identity.channel,
                )
                .output(bytes),
                CONTROL_TIMEOUT,
            )
            .wait()
            .map_err(CandlelightError::from)
    }
    fn start(
        &mut self,
        bitrate: u32,
        listen_only: bool,
        session_origin: Instant,
    ) -> Result<(), CandlelightError> {
        let timing = self.identity.timing.for_bitrate(bitrate)?;
        let mode = mode_bytes(self.identity.timing.features, listen_only)?;
        self.stop()?;
        self.control_out(GsUsbRequest::BitTiming, &timing.encode())?;
        self.needs_reset = true;
        self.control_out(GsUsbRequest::Mode, &mode)?;
        self.listen_only = listen_only;
        self.started = true;
        self.started_at = session_origin;
        Ok(())
    }
    fn stop(&mut self) -> Result<(), CandlelightError> {
        self.started = false;
        self.control_out(GsUsbRequest::Mode, &reset_mode_bytes())?;
        self.needs_reset = false;
        self.pending_tx.clear();
        Ok(())
    }
    fn transmit(
        &mut self,
        mut frame: CanFrame,
        timeout: Duration,
    ) -> Result<u32, CandlelightError> {
        if !self.started {
            return Err(CandlelightError::NotStarted);
        }
        if self.listen_only {
            return Err(CandlelightError::PassiveTransmit);
        }
        if self.pending_tx.len() >= 10 {
            return Err(protocol(
                "GS_USB TX echo queue is full; receive pending confirmations first",
            ));
        }
        while self.next_echo == RX_ECHO_ID || self.pending_tx.contains_key(&self.next_echo) {
            self.next_echo = self.next_echo.wrapping_add(1);
        }
        let echo_id = self.next_echo;
        self.next_echo = self.next_echo.wrapping_add(1);
        let bytes = encode_frame(&frame, echo_id, self.identity.channel)?;
        frame.direction = CanDirection::Tx;
        // Keep uncertain submissions until an echo or reset; never automatically retry.
        self.pending_tx.insert(echo_id, frame);
        let completion = self.output.transfer_blocking(bytes.into(), timeout);
        completion.status.map_err(CandlelightError::from)?;
        if completion.actual_len != 20 {
            return Err(protocol("Short GS_USB CAN transmit"));
        }
        Ok(echo_id)
    }
    fn receive(&mut self, timeout: Duration) -> Result<GsUsbFrame, CandlelightError> {
        if !self.started {
            return Err(CandlelightError::NotStarted);
        }
        let completion = self.input.transfer_blocking(
            Buffer::new(
                20usize.div_ceil(self.input.max_packet_size()) * self.input.max_packet_size(),
            ),
            timeout,
        );
        completion.status.map_err(|error| {
            let error = CandlelightError::from(error);
            if completion.actual_len == 0 {
                error
            } else {
                error.with_raw(&completion.buffer[..completion.actual_len])
            }
        })?;
        let elapsed = self.started_at.elapsed().as_nanos().min(i64::MAX as u128) as i64;
        let wall_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| CandlelightError::Worker(e.to_string()))?
            .as_nanos()
            .min(i64::MAX as u128) as i64;
        let raw = &completion.buffer[..completion.actual_len];
        if raw.len() >= 12 && raw[9] != self.identity.channel {
            return Err(protocol("GS_USB reply belongs to another CAN channel").with_raw(raw));
        }
        let mut decoded = match decode_frame(raw, elapsed, wall_time) {
            Err(error @ CandlelightError::BusOff(_)) => {
                self.started = false;
                return Err(error);
            }
            other => other?,
        };
        if decoded.echo_id != RX_ECHO_ID {
            let original = self
                .pending_tx
                .remove(&decoded.echo_id)
                .ok_or_else(|| protocol("Unexpected GS_USB TX echo ID").with_raw(raw))?;
            // Only echo_id identifies a TX confirmation in GS_USB; Linux does not
            // require echoed CAN ID or payload. Restore the submitted frame.
            decoded.frame.can_id = original.can_id;
            decoded.frame.extended = original.extended;
            decoded.frame.data = original.data;
        }
        Ok(decoded)
    }
}

#[cfg(test)]
mod channel_ownership_tests {
    use super::CandlelightChannelLease;

    #[test]
    fn one_owner_per_interface_channel_and_drop_releases_the_claim() {
        let key = format!("lease-test:{}", std::thread::current().name().unwrap());
        let first = CandlelightChannelLease::claim(key.clone()).unwrap();

        let collision = CandlelightChannelLease::claim(key.clone()).unwrap_err();
        assert!(collision.to_string().contains("already owned"));
        let other_channel = CandlelightChannelLease::claim(format!("{key}:channel-1")).unwrap();
        let other_device = CandlelightChannelLease::claim(format!("{key}:other-device")).unwrap();

        drop(first);
        assert!(CandlelightChannelLease::claim(key).is_ok());
        drop((other_channel, other_device));
    }
}
