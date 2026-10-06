# RH-02 Candlelight CAN Workbench

**Status:** Proposed design for user review; implementation has not started.

## Purpose

Add a developer-oriented CAN Workbench for the Jhoinrch RH-02 USB-CAN adapter. It supports passive frame sniffing and active, read-only vehicle diagnostics through the same device. Numeric decoded measurements continue through LogNook's existing sample, graph, recording, and replay pipeline. Raw CAN frames remain distinguishable from decoded measurements.

## Hardware and transport

The RH-02 manual identifies the factory firmware as Candlelight and the controller as STM32F072C8T6. Candlelight uses the GS_USB USB protocol. The expected Candlelight USB identity is VID `1D50`, PID `606F`; verify this on the physical RH-02 before treating the match as device identity. Windows uses WinUSB for Candlelight WCID descriptors. Sources: [RH-02 manual](https://manuals.plus/asin/B0CRB8KXWL), [CANable setup](https://www.canable.io/getting-started.html), and [GS_USB platform notes](https://github.com/hardbyte/python-can/blob/main/doc/interfaces/gs_usb.rst).

Use the existing Rust/Tauri architecture with a focused Candlelight GS_USB transport in `lognook-core`, using `nusb` for USB enumeration and endpoint transfers. Do not require reflashing to SLCAN, installing a separate Python runtime, or changing the device driver manually. Device discovery and interface opening are separate from starting CAN traffic. The physical adapter is not currently connected to the development PC, so USB identity and bus behavior still require hardware verification.

One Rust-owned Candlelight session serializes USB access. The passive receiver, active diagnostic requests, and close path must not open competing handles or read the endpoint concurrently. Adapter removal, WinUSB errors, bus-off, and receive timeout are reported as distinct capture/connection errors.

Add a typed `CanFrame` model carrying host elapsed/UTC time, arbitration ID and width, DLC, bytes, direction (`rx`/`tx`), and available error/overflow flags. The owning session publishes frames to the CAN Workbench event stream and the capture writer. During active OBD reads, the same owner publishes the request/reply frames and the decoder separately emits numeric `Sample` events through the existing measurement pipeline.

## Workbench modes

### Passive CAN capture

- User selects a supported nominal bitrate from the device's verified GS_USB timing capabilities. Default to 500 kbit/s; do not scan bitrates automatically.
- Start Candlelight in listen-only mode. Do not transmit CAN frames in this mode.
- Capture standard 11-bit and extended 29-bit CAN 2.0 frames with host monotonic and UTC receive timestamps, identifier format, DLC, data bytes, and frame/error flags when supplied by firmware.
- Show a scrolling raw-frame table and a compact per-ID summary with count, rate, first/last timestamp, and latest payload. The display may coalesce counters but must not silently drop frames from a saved capture.
- Stream saved captures to a user-selected, create-new `.candump.csv` file rather than buffering the entire bus in memory. Use explicit columns for receive time, elapsed nanoseconds, direction, CAN ID, standard/extended format, DLC, payload hex, and available flags. Keep this raw capture artifact separate from numeric `.lognook` recordings. No raw-frame replay is part of this slice.

### Active OBD-II diagnostics

- In active mode, configure the selected bitrate and normal CAN mode, then perform bounded standard OBD-II Mode 01 support-page discovery. Preserve each responding ECU separately; do not combine replies from different responders into one capability profile.
- Expose only the standard signals reported supported by the responding ECU. Poll selected signals through `HardwareAdapter` and publish timestamped numeric `Sample` values through the existing acquisition pipeline.
- Display the corresponding transmitted and received CAN frames in the CAN Workbench view. Numeric values continue to use existing graphs, `.lognook` recordings, CSV export, and replay.

### Catalog-backed OEM tests

- Add Candlelight as a supported adapter for the Test Workbench's existing source-backed service 21/22 candidate action and ordered queue.
- Reload candidates from the v0.3 read-only catalog before transmission. Use the exact source service, identifier width, request/response route, and current candidate decoder.
- Implement only the ISO-TP behavior needed by eligible catalog definitions; retain exact raw request/response frames, ECU identity, timestamps, and typed outcomes in the existing research results/evidence flow.
- Finite decoded numeric values use the existing research sample pipeline. They do not promote the candidate's research or vehicle-validation state.

## Transmission boundary

The active Workbench supports standard OBD-II Mode 01 reads and source-backed service 21/22 candidates. Passive mode is strictly listen-only. Do not add arbitrary raw-hex frame entry, unrestricted CAN-ID sweeps, writes, DTC clears, session/security changes, routines, or actuator commands. A broader active exploration requires its own finite, source-scoped procedure and approval.

## User interface

Add a CANable/Candlelight choice to the existing adapter controls. Show detected USB identity, selected bitrate, connection state, mode, receive/transmit counters, and errors. The CAN Workbench provides explicit `Listen` and `Active OBD` modes, `Start`/`Stop`, raw frames, per-ID rates, and capture export. Disable switching adapters while acquisition, recording, replay, or a research queue is active. Do not make the CAN page a second graph or second request owner.

The firmware's support for hardware ID filtering is not assumed. Apply filters only to the displayed summary; retain every captured frame in an active saved capture.

## Verification

- Unit fixtures cover GS_USB control-transfer/frame encoding, standard and extended frame decoding, listen-only setup, bitrate validation, disconnect/errors, and frame timestamps.
- Fake-device tests prove only one owner reads the USB endpoint and that active OBD responses retain their ECU IDs and exact raw frames.
- Core tests prove Mode 01 samples and finite OEM values use the existing sample pipeline without mixing ECU identities or changing catalog validation.
- UI tests cover device selection, listen/active mode controls, bitrate selection, Start/Stop, frame/rate display state, and capture export.
- Acceptance on hardware: enumerate the RH-02, confirm Candlelight/GS_USB identity, verify passive capture without TX, verify a standard Mode 01 supported-PID response and selected live signal, then try one source-backed Toyota/Scion catalog candidate. Record the actual bitrate, ECU route, latency, raw request/response, and observed rate. A returned response is evidence, not automatic vehicle validation.

## Out of scope

SLCAN firmware support, automatic bitrate scanning, raw frame injection, broad identifier sweeps, DBC-based signal decoding, and replaying raw CAN captures are separate work. The first implementation focuses on the factory Candlelight path and the Workbench functions above.
