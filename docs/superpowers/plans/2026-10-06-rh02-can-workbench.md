# RH-02 Candlelight CAN Workbench Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Candlelight/GS_USB bench workbench that captures raw CAN, scopes configurable payload fields through LogNook's shared sample/recording path, and supports the approved diagnostic and analysis tools.

**Architecture:** `lognook-core` owns frame models, decoding, analysis, CSV capture, the GS_USB session, and serialized bench actions. A single Candlelight session publishes decoded numeric values through `LogNookEngine`'s existing sample pipeline and exposes raw-frame events separately; Tauri commands control that session and the Svelte workbench configures and displays it. Raw CAN captures remain `.candump.csv`; numeric measurements remain ordinary LogNook samples and recordings.

**Tech Stack:** Rust workspace, Tauri 2 commands/events, Svelte 5/TypeScript, existing `csv` crate, `nusb` 0.2.7 for Candlelight USB, Node's built-in UI test runner.

**Spec:** `docs/superpowers/specs/2026-10-06-rh02-candlelight-can-workbench-design.md`

## Global Constraints

- Capture classic CAN 2.0 standard 11-bit and extended 29-bit frames; the RH-02's STM32F072/Candlelight path has up to 8 payload bytes.
- Default to 500 kbit/s and do not scan bitrates automatically.
- One Rust-owned Candlelight session serializes USB reads, sends, scans, disconnect, and capture shutdown.
- Stream raw captures to a create-new `.candump.csv`; keep raw captures separate from numeric `.lognook` recordings.
- Selected field samples retain the CAN receive timestamp and actual frame bytes and use the existing sample, graph, recording, export, and replay paths.
- User-selected CAN-ID/Mode 21/Mode 22 scans are explicit, cancellable, and record all TX/RX evidence. They never run on startup or connect.
- Bench CAN sends/scans stay local to the explicit Test Workbench; no flashing or automatic multi-step ECU programming is included.
- Inferred payload semantics and correlation results remain research hypotheses, never automatic OEM decoder or vehicle validation.

## Review Focus

- Standard/extended-ID boundary, remote/error frames, and malformed DLC must not corrupt or mislabel later frames. Pin validation in Task 1/4.
- Bit fields at payload boundaries and Motorola/Intel bit numbering must decode consistently, including signed 64-bit values. Pin conventions in Task 1.
- Short, malformed, truncated, or duplicate-header capture CSV must fail per row without fabricating samples or aborting access to later valid rows. Pin in Task 3.
- USB removal and a stopped scan must release the single endpoint owner and prevent later queued transmissions. Pin in Task 5.
- A busy bus and slow disk must not silently lose frames from a saved capture or freeze the UI. Pin queue/backpressure and UI coalescing in Tasks 3/5/6.

---

### Task 1: CAN frame, signal, and timestamped sample primitives

**Files:**
- Create: `crates/lognook-core/src/can.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Modify: `crates/lognook-core/src/engine.rs`
- Modify: `crates/lognook-core/src/pipeline.rs`
- Modify: `crates/lognook-core/src/pipeline_worker/live_samples.rs`
- Test: `crates/lognook-core/tests/can_signals.rs`
- Test: `crates/lognook-core/src/engine.rs`

**Interfaces:**
- Produces `CanFrame { elapsed_ns, wall_time_unix_ns, can_id, extended, direction, data, flags }`, `CanSignalDefinition { id, name, can_id, extended, start_bit, bit_length, byte_order, signed, factor, offset, unit }`, and `decode_can_signal(&CanFrame, &CanSignalDefinition, &str) -> Result<Option<Sample>, LogNookError>`.
- Defines `start_bit` using the DBC convention: Intel fields progress from the selected LSB0 bit; Motorola fields progress from the selected MSB0 bit with sawtooth byte transitions. `bit_length` is 1–64.
- Produces a cloneable `SamplePublisher::publish_timestamped_sample(Sample) -> Result<(), LogNookError>` backed by the existing bounded pipeline, and `LogNookEngine::sample_publisher() -> SamplePublisher`. Both preserve a frame's elapsed timestamp and advance the pipeline marker clock monotonically, including when samples arrive out of timestamp order.

- [x] **Step 1: Write failing tests** for standard/extended ID bounds, payload/DLC validation, Intel and Motorola bit extraction, signed and unsigned scaling, short-frame rejection, non-finite scale rejection, sample source/signal/raw-byte/timestamp preservation, and marker time not moving backward when a late timestamped sample arrives.
- [x] **Step 2: Run** `cargo test -p lognook-core --test can_signals`; confirm the new APIs/behaviors fail.
- [x] **Step 3: Implement** the small typed model/decoder and timestamp-preserving sample publication, reusing `Sample`, `SampleQuality`, and `SampleOrigin`.
- [x] **Step 4: Run** `cargo test -p lognook-core --test can_signals` and the focused engine tests; require all assertions to pass.

### Task 2: Bus analysis and bounded trigger capture

**Files:**
- Create: `crates/lognook-core/src/can_analysis.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Test: `crates/lognook-core/tests/can_analysis.rs`

**Interfaces:**
- Consumes Task 1 `CanFrame` and `CanSignalDefinition`.
- Produces `CanAnalyzer::feed(&CanFrame)`, census rows (ID/count/rate/DLC/period/active/static/counter/checksum candidates), per-byte history, request/reply timing rows, OBD-to-field correlation hypotheses, and `CanTriggerRecorder` with configurable pre/post durations.
- Produces `CanAnalyzer::feed_obd_sample(&Sample)` so correlation uses timestamped OBD samples from the common pipeline alongside broadcast frames.
- Analyzer labels for counter/checksum/correlation remain explicitly tentative.

- [x] **Step 1: Write failing tests** for rate/inter-arrival and byte summaries, bounded history, ISO-TP request/reply pairing with NRC preservation, known synthetic OBD/byte correlation using explicit sample timestamps, and trigger windows that contain exactly the configured pre/post frames.
- [x] **Step 2: Run** `cargo test -p lognook-core --test can_analysis`; confirm expected failures.
- [x] **Step 3: Implement** streaming analyzers with bounded memory and no independent transport or sample owner.
- [x] **Step 4: Run** `cargo test -p lognook-core --test can_analysis`; require all assertions to pass.

### Task 3: Streamed raw capture and replay

**Files:**
- Create: `crates/lognook-core/src/can_capture.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Test: `crates/lognook-core/tests/can_capture.rs`

**Interfaces:**
- Consumes Task 1 `CanFrame` and Task 2 analyzers.
- Produces `CanCaptureWriter::create_new(path)`, `write_frame(&CanFrame)`, `finish()`, and a row-streaming reader that invokes a callback for each valid frame while returning row-specific parse errors.
- CSV columns: receive Unix time, elapsed nanoseconds, direction, hexadecimal CAN ID, standard/extended format, DLC, payload hex, and flags. Raw capture replay feeds the same analyzers and configured field decoder.

- [x] **Step 1: Write failing tests** for CSV round trips, create-new behavior, raw frame order/time/format retention, malformed rows followed by valid rows, replay analyzer totals, and surfaced write failures.
- [x] **Step 2: Run** `cargo test -p lognook-core --test can_capture`; confirm expected failures.
- [x] **Step 3: Implement** a streaming writer/reader using the existing `csv` dependency; do not buffer a complete live capture.
- [x] **Step 4: Run** `cargo test -p lognook-core --test can_capture`; require all assertions to pass.

### Task 4: Candlelight GS_USB device transport

**Files:**
- Modify: `Cargo.lock`
- Modify: `crates/lognook-core/Cargo.toml`
- Create: `crates/lognook-core/src/candlelight.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Test: `crates/lognook-core/tests/candlelight_protocol.rs`

**Interfaces:**
- Consumes Task 1 `CanFrame`.
- Produces Candlelight/GS_USB enumeration, identity, bitrate configuration, listen-only receive, and transmit operations with a single claimed interface and endpoint owner.
- Uses `nusb` 0.2.7; recognize known GS_USB/Candlelight IDs `1D50:606F` and `1209:2323`, then verify interface/endpoints and treat USB IDs as hints rather than identity proof. Keep device operations on a worker thread using the crate's blocking `MaybeFuture` API.
- On Windows, claim the interface and use `Interface::control_in/out` for vendor/interface controls; do not use `Device::control_in/out`. Discover bulk IN/OUT endpoint addresses from descriptors. Encode Candlelight GS_USB fields little-endian without sending optional host-format negotiation; request IDs include host format 0, bit timing 1, mode 2, bus-error reporting 3, timing constants 4, and device configuration 5. Honor the advertised listen-only feature bit; the GS_USB RX echo marker is `0xFFFF_FFFF`.

- [x] **Step 1: Write failing tests** for request IDs/Windows interface-recipient setup, descriptor-derived endpoint selection, little-endian GS_USB header/control encoding, bitrate validation, standard/extended receive decoding, listen-only feature handling, frame-size/DLC checks, timeout/bus-off/device-removal diagnostics, and RX-marker versus TX-echo direction.
- [x] **Step 2: Run** `cargo test -p lognook-core --test candlelight_protocol`; confirm expected failures.
- [x] **Step 3: Implement** the USB transport and pure protocol helpers against official GS_USB/Candlelight behavior; add only `nusb` as the needed dependency.
- [x] **Step 4: Run** `cargo test -p lognook-core --test candlelight_protocol`; require all assertions to pass.

### Task 5: Serialized CAN workbench session and commands

**Files:**
- Create: `crates/lognook-core/src/can_workbench.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Modify: `crates/lognook-core/src/model.rs`
- Modify: `crates/lognook-core/src/engine.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/src-tauri/src/commands/can.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Test: `crates/lognook-core/tests/can_workbench.rs`

**Interfaces:**
- Consumes Tasks 1–4 and the existing OEM catalog/read pipeline.
- Produces one `CanWorkbench` worker owned by `LogNookEngine`, with explicit connect/listen/stop, raw capture start/stop, manual one-frame send, cancellable finite CAN-ID/Mode 21/Mode 22 scans, Mode 01 support discovery and selected-PID polling, and source-backed OEM candidate reads. The worker owns the adapter and serializes all USB actions; it emits batched `EngineEvent::CanFrames` and publishes decoded `Sample`s through the Task 1 `SamplePublisher`.
- `LogNookEngine::connect_candlelight(selection, config)`, `start_can_capture(path)`, `stop_can_capture()`, `send_can_frame(frame, pacing, repeat)`, `start_can_scan(config)`, and `stop_can_scan()` keep CAN connection/recording state under the existing engine. `is_connected`, ordinary adapter connect, and disconnect account for the one active CAN session, so existing recording gates remain correct.
- Active sends and scans require a paired create-new `.candump.csv` and `.candump.jsonl` capture. The streamed sidecar records scan configuration, source-candidate provenance, typed results, TX submission/echo status, structured errors, and clean-close state; replay reports incomplete or mismatched sessions. No transmission proceeds after evidence writing fails.
- Tauri commands expose discovery, connect, configure fields, capture, send, scan/stop, and capture replay/report; frame I/O never runs on the webview thread. RX/TX timestamps share one monotonic origin, RX frames are persisted before flow-control TX, and the worker distinguishes nonfatal unsupported frames from fatal transport errors.

- [x] **Step 1: Write failing tests** proving only the owner reads/writes the fake transport, stop cancels before the next scan request, ranges/pacing are honored, raw TX/RX are preserved, slow capture writes apply backpressure without silent frame loss, and selected field samples enter the common recorder with source timestamps.
- [x] **Step 2: Run** `cargo test -p lognook-core --test can_workbench`; confirm expected failures.
- [x] **Step 3: Implement** the workbench worker, sample path, Tauri state/commands, and event serialization; refuse a second adapter while another connection is active.
- [x] **Step 4: Run** `cargo test -p lognook-core --test can_workbench` and Tauri crate tests; require all assertions to pass.

### Task 6: CAN Workbench UI and existing graph integration

**Files:**
- Create: `apps/desktop/src/CanWorkbenchPanel.svelte`
- Create: `apps/desktop/src/canWorkbench.ts`
- Modify: `apps/desktop/src/App.svelte`
- Modify: `apps/desktop/src/types.ts`
- Modify: `apps/desktop/src/connectionControls.ts`
- Modify: `apps/desktop/src-tauri/src/commands/can.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: `apps/desktop/tests/can-workbench.test.mjs`

**Interfaces:**
- Consumes Task 5 Tauri commands/events and Tasks 1–3 frame/scope/analyzer types.
- Produces Candlelight selection/status, bitrate and listen/active mode controls, raw-frame view and bounded live per-ID count/rate, multi-field scope definitions, raw CSV plus JSONL evidence replay/report with Rust census/byte results, Mode 01/21/22 and CAN-ID scan controls, and visible send/TX-echo/error results. Active send and scan controls are disabled until the paired evidence capture is open and the adapter is in active mode.
- Numeric CAN fields appear as normal signal options and use `SignalGraphs`; do not add a second graph or decode frames only in Svelte. Coalesce only UI refreshes, not capture writes.
- Render raw frames and diagnostics without blocking acquisition; coalesce frame-list UI updates while Rust continues synchronous ordered capture writes. The common event pipeline publishes decoded values to the existing live graph.
- `CanWorkbenchStatus` also updates the application-level connection state so the existing Disconnect/Record controls and adapter-exclusivity behavior remain consistent while Candlelight is active.
- Keep pair-timing, correlation, and trigger-capture analysis in Task 7's Rust analysis/report slice; Task 6 must not approximate those protocols in TypeScript.

- [x] **Step 1: Write failing UI tests** for command argument shapes, ID/field validation, multiple graph traces, mode/state transitions, replay/sidecar display, and stop behavior.
- [x] **Step 2: Run** `npm run test:ui --prefix apps/desktop`; confirm the new cases fail.
- [x] **Step 3: Implement** the panel, typed Tauri client helpers, and graph-option integration.
- [x] **Step 4: Run** `npm run test:ui --prefix apps/desktop` and `npm run build --prefix apps/desktop`; require all assertions/type checks/build to pass.

### Task 7: Reports, DBC skeleton, docs, and full verification

**Files:**
- Modify: `crates/lognook-core/src/can_analysis.rs`
- Modify: `apps/desktop/src-tauri/src/commands/can.rs`
- Modify: `apps/desktop/src/CanWorkbenchPanel.svelte`
- Modify: `apps/desktop/src/canWorkbench.ts`
- Modify: `LogNook_Blueprint/03_HARDWARE_PROTOCOLS.md`
- Modify: `PROJECT_STATUS.md`
- Test: `crates/lognook-core/tests/can_analysis.rs`
- Test: `crates/lognook-core/tests/can_workbench.rs`
- Test: `apps/desktop/tests/can-workbench.test.mjs`

**Interfaces:**
- Consumes Tasks 1–6.
- Produces saved-capture session report and ID/payload comparison, request/reply timing and correlation hypotheses, bounded pre/post trigger capture controls, plus optional DBC skeleton with placeholder signal definitions clearly marked as unverified. Zero-DLC messages remain visible without an out-of-bounds placeholder signal.
- A trigger ended by disarm, capture stop, or session end is emitted and journaled as an incomplete window with its end reason; disarming before marking cancels the armed trigger without a window.
- Route keys and labels preserve standard 11-bit versus extended 29-bit identity. Correlation labels include OBD source and ECU, and hypotheses remain tied to timestamp overlap.

- [x] **Step 1: Write failing tests** for deterministic report/census output, ID/byte differences between captures, request/reply and correlation summaries, bounded trigger capture behavior, DBC placeholder labeling, zero-DLC messages, distinct route widths, correlation source/ECU labels, and interrupted trigger windows.
- [x] **Step 2: Run** `cargo test -p lognook-core --test can_analysis`; confirm expected failures.
- [x] **Step 3: Implement** these export conveniences and update architecture/progress documentation with physical-verification limits.
- [x] **Step 4: Run** `cargo fmt --all -- --check`, `cargo test --workspace`, `npm run test:ui --prefix apps/desktop`, and `npm run build --prefix apps/desktop`. Full workspace formatting reported unrelated existing diffs in untouched files; changed-file rustfmt passed.
- [x] **Step 5: Report** physical limitations: the RH-02 is not connected in this environment; Windows device identity, live RX/TX, bitrate, and vehicle responses remain unverified until tested with the dongle.
