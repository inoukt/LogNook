# RH-02 Candlelight CAN Workbench

**Status:** Implemented in the Rust/Tauri/Svelte application; software verification is complete for the documented capture, scan, evidence, and graph pipeline. Physical RH-02/Candlelight USB and vehicle acceptance remains pending.

## Purpose

Add a developer-oriented CAN Workbench for the Jhoinrch RH-02 USB-CAN adapter. It supports passive frame sniffing and operator-directed active CAN tests through the same device. Numeric decoded measurements continue through LogNook's existing sample, graph, recording, and replay pipeline. Raw CAN frames remain distinguishable from decoded measurements and catalog evidence.

## Hardware and transport

The RH-02 manual identifies the factory firmware as Candlelight and the controller as STM32F072C8T6. Candlelight uses the GS_USB USB protocol. Known GS_USB/Candlelight VID/PID pairs include `1D50:606F` and `1209:2323`; treat either as a detection hint, verify the interface/endpoints, and confirm the RH-02 identity on the physical device. Windows uses WinUSB for Candlelight WCID descriptors. Sources: [RH-02 manual](https://manuals.plus/asin/B0CRB8KXWL), [CANable setup](https://www.canable.io/getting-started.html), and [upstream Linux GS_USB driver](https://github.com/torvalds/linux/blob/master/drivers/net/can/usb/gs_usb.c).

Use the existing Rust/Tauri architecture with a focused Candlelight GS_USB transport in `lognook-core`, using `nusb` for USB enumeration and endpoint transfers. Do not require reflashing to SLCAN, installing a separate Python runtime, or changing the device driver manually. Device discovery and interface opening are separate from starting CAN traffic. The physical adapter is not currently connected to the development PC, so USB identity and bus behavior still require hardware verification.

Implement the Candlelight profile from the upstream GS_USB layout: vendor/interface request numbers include host format 0, bitrate 1, mode 2, bus-error reporting 3, bitrate constants 4, and device configuration 5; frame fields are little-endian, receive frames use the GS_USB RX echo marker, and the USB interface's descriptors determine bulk endpoints. Candlelight does not implement the optional host-selected byte-order exchange, so use little-endian without sending host-format negotiation. On Windows, claim the interface and use its `control_in`/`control_out` methods for vendor control transfers; `nusb::Device::control_in/out` is unavailable there. Do not hard-code endpoint addresses. If the device does not advertise listen-only mode, refuse passive capture rather than start the bus in normal mode. References: [upstream Linux GS_USB driver](https://github.com/torvalds/linux/blob/master/drivers/net/can/usb/gs_usb.c), [CandleLight firmware](https://github.com/candle-usb/candleLight_fw), and [nusb Interface API](https://docs.rs/nusb/latest/nusb/struct.Interface.html).

One Rust-owned Candlelight session serializes USB access. The passive receiver, active diagnostic requests, and close path must not open competing handles or read the endpoint concurrently. Adapter removal, WinUSB errors, bus-off, and receive timeout are reported as distinct capture/connection errors.

Add a typed `CanFrame` model carrying host elapsed/UTC time, arbitration ID and width, DLC, bytes, direction (`rx`/`tx`), and available error/overflow flags. The owning session publishes frames to the CAN Workbench event stream and the capture writer. During active OBD reads, the same owner publishes the request/reply frames and the decoder separately emits numeric `Sample` events through the existing measurement pipeline. Source-backed Mode 21/22 CAN candidate reads use the shared OEM decoder, preserve raw scan evidence, and publish finite numeric results with the ECU and receive timestamps so the existing graph/recording/replay path can render them.

## Workbench modes

### Passive CAN capture

- User selects a supported nominal bitrate from the device's verified GS_USB timing capabilities. Default to 500 kbit/s; do not scan bitrates automatically.
- Start Candlelight in listen-only mode. Do not transmit CAN frames in this mode.
- Capture standard 11-bit and extended 29-bit CAN 2.0 frames with host monotonic and UTC receive timestamps, identifier format, DLC, data bytes, and frame/error flags when supplied by firmware.
- Show a scrolling raw-frame table and a compact per-ID summary with count, rate, first/last timestamp, and latest payload. The display may coalesce counters but must not silently drop frames from a saved capture.
- Stream saved captures to a user-selected, create-new `.candump.csv` file rather than buffering the entire bus in memory. Use explicit columns for receive time, elapsed nanoseconds, direction, CAN ID, standard/extended format, DLC, payload hex, and available flags. Keep this raw capture artifact separate from numeric `.lognook` recordings; saved-capture replay is described below.

### CAN analysis and signal scopes

- Run analysis from the same timestamped frame stream used by the raw capture; do not add a second reader or generate display-only samples.
- Provide a Bus Census with per-ID frame count, rate, observed DLC, inter-arrival timing, active/static payload bytes, and counter/checksum candidates. Candidate labels are statistical hints, not decoded or validated facts.
- Record repeatable, operator-labeled vehicle event marks during a raw capture, each with its own before/after window. On replay, group marks by label and rank CAN IDs and payload-byte transitions or rate changes that recur around the marked actions. Preserve timestamps and raw frames; present the ranking as research evidence, not decoded meaning.
- Provide a pinned-ID byte view with recent values and min/max/latest summaries for each observed payload byte.
- Provide live signal scopes for multiple selected traces on a shared time axis. A raw CAN field definition contains CAN ID and frame format, bit start/length, byte order, signedness, factor, offset, display name, and unit. Support fields up to the classic CAN payload width; reject definitions that do not fit the current frame. Offer byte-aligned integer shortcuts while keeping bit-level selection available.
- Scope OBD Mode 01 PIDs as decoded measurements alongside raw broadcast fields. Broadcast fields use the frame receive timestamp and enter the existing `Sample` pipeline with their own `source_id`/`signal_id`, unit, and original frame bytes. The existing graph, recording, CSV, and replay paths then render the actual acquired values. Missing frames remain gaps; do not interpolate fabricated values.
- Let an operator correlate selected broadcast fields with simultaneously acquired standard OBD values. Show sample count and correlation/slope/offset as a research hypothesis only; do not name or validate an OEM signal automatically.
- Show basic ISO-TP request/reply pairs, response latency, and negative-response codes when those frames are present. Preserve raw frames as the primary evidence.
- Provide configurable trigger capture with a bounded pre-trigger ring and post-trigger duration (for example: selected CAN ID appears or a payload byte crosses a threshold), plus unnamed/named timeline markers. Save the triggered frame window as a normal raw capture.
- Allow saved raw captures to be reopened and fed through the same census, byte, pair, correlation, and signal-scope analyzers. Add a session report and byte-level comparison after the basic live and replay path works.
- Offer DBC skeleton export only as a later convenience. It may carry observed IDs/DLC and clearly marked placeholder signal layouts; it must not imply discovered field semantics are correct.

**Signal-scope boundary:** Candlelight supplies digital CAN frames and payload bytes, not the analog CAN_H/CAN_L voltage waveform. These scopes decode timestamped payload fields into traces. Viewing physical differential voltage, edges, ringing, or bit timing requires a separate electrical oscilloscope or CAN analyzer with analog sampling; the RH-02 cannot provide that waveform.

### Active OBD-II diagnostics

- In active mode, configure the selected bitrate and normal CAN mode, then perform bounded standard OBD-II Mode 01 support-page discovery. Preserve each responding ECU separately; do not combine replies from different responders into one capability profile.
- Expose only the standard signals reported supported by the responding ECU. Poll selected signals through `HardwareAdapter` and publish timestamped numeric `Sample` values through the existing acquisition pipeline.
- Display the corresponding transmitted and received CAN frames in the CAN Workbench view. Numeric values continue to use existing graphs, `.lognook` recordings, CSV export, and replay.

### Catalog-backed OEM tests

- Add Candlelight as a supported adapter for the Test Workbench's existing source-backed service 21/22 candidate action and ordered queue.
- Reload candidates from the v0.3 read-only catalog before transmission. Use the exact source service, identifier width, request/response route, and current candidate decoder.
- Implement only the ISO-TP behavior needed by eligible catalog definitions; retain exact raw request/response frames, ECU identity, timestamps, and typed outcomes in the existing research results/evidence flow.
- Finite decoded numeric values use the existing research sample pipeline. They do not promote the candidate's research or vehicle-validation state.

### Manual CAN tests and scans

- Bench mode exposes an operator-configured raw CAN frame sender for standard or extended CAN 2.0 IDs, payload bytes, pacing, and a finite repeat count. It also exposes configurable CAN-ID, Mode 21, and Mode 22 scans; the user may select the full range supported by the chosen request form.
- Each scan requires explicit adapter/bitrate/route/range selection and a user Start action. It is cancellable and records its configuration, every transmitted frame, all received frames, and the typed result. No source or decoder is required to send a bench frame, and scan results remain raw research evidence unless separately decoded and validated.
- Active scans are not background tasks and never start on connect. Normal acquisition and polling remain separate. Manual frame traffic is labeled experimental and the UI does not classify an operator-entered frame as harmless or read-only.
- This feature does not implement adapter firmware updates, ECU flashing/programming sequences, or automatic state-changing routines.

## User interface

Add a CANable/Candlelight choice to the existing adapter controls. Show detected USB identity, selected bitrate, connection state, mode, receive/transmit counters, and errors. The CAN Workbench provides explicit `Listen` and `Active OBD` modes, `Start`/`Stop`, raw frames, per-ID rates, repeatable labeled event marks, and capture export. Disable switching adapters while acquisition, recording, replay, or a research queue is active. Do not make the CAN page a second graph or second request owner.

The firmware's support for hardware ID filtering is not assumed. Apply filters only to the displayed summary; retain every captured frame in an active saved capture. Keep capture ingestion and disk writes off the UI thread; coalesce view updates without dropping frames from the saved artifact.

## Verification

- Unit fixtures cover GS_USB control-transfer/frame encoding, standard and extended frame decoding, listen-only setup, raw frame transmission, bitrate validation, disconnect/errors, and frame timestamps.
- Fake-device tests prove only one owner reads the USB endpoint, that scans honor configured ranges/pacing/Stop, and that active OBD responses retain their ECU IDs and exact raw frames.
- Core tests cover Bus Census timing/byte summaries, repeated manual-event correlation, payload field extraction (bit positions, signedness, byte order, scaling, short frames), timestamp-preserving scope samples, pre/post trigger windows, and analyzer replay from saved raw CSV.
- Core tests prove Mode 01 samples and finite OEM values use the existing sample pipeline without mixing ECU identities or changing catalog validation.
- UI tests cover device selection, listen/active bench modes, bitrate and raw-frame controls, scan range/pacing/Stop, frame/rate display state, multiple signal traces, scope field configuration, trigger state, capture export, and replay.
- Acceptance on hardware: enumerate the RH-02, confirm Candlelight/GS_USB identity, verify passive capture without TX, verify a standard Mode 01 supported-PID response and selected live signal, then try one source-backed Toyota/Scion catalog candidate. Record the actual bitrate, ECU route, latency, raw request/response, and observed rate. A returned response is evidence, not automatic vehicle validation.

## Out of scope

SLCAN firmware support, automatic bitrate detection, importing full third-party DBC databases, analog CAN_H/CAN_L waveform sampling, adapter firmware updates, and ECU firmware flashing/programming workflows are separate work. Implement the factory Candlelight path and core raw/scope/replay path first; add census, correlation, triggers, reports, and DBC skeleton export as small slices on that shared frame stream.
