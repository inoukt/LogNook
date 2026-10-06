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
- Test: `crates/lognook-core/tests/can_signals.rs`
- Test: `crates/lognook-core/src/engine.rs`

**Interfaces:**
- Produces `CanFrame { elapsed_ns, wall_time_unix_ns, can_id, extended, direction, data, flags }`, `CanSignalDefinition { id, name, can_id, extended, start_bit, bit_length, byte_order, signed, factor, offset, unit }`, and `decode_can_signal(&CanFrame, &CanSignalDefinition, &str) -> Result<Option<Sample>, LogNookError>`.
- Defines `start_bit` using the DBC convention: Intel fields progress from the selected LSB0 bit; Motorola fields progress from the selected MSB0 bit with sawtooth byte transitions. `bit_length` is 1–64.
- Produces a cloneable `SamplePublisher` backed by the existing bounded pipeline, and `LogNookEngine::publish_timestamped_sample(Sample) -> Result<(), LogNookError>`. Both preserve a frame's elapsed timestamp and advance the pipeline marker clock monotonically.

- [ ] **Step 1: Write failing tests** for standard/extended ID bounds, payload/DLC validation, Intel and Motorola bit extraction, signed and unsigned scaling, short-frame rejection, non-finite scale rejection, and sample source/signal/raw-byte/timestamp preservation.
- [ ] **Step 2: Run** `cargo test -p lognook-core --test can_signals`; confirm the new APIs/behaviors fail.
- [ ] **Step 3: Implement** the small typed model/decoder and timestamp-preserving sample publication, reusing `Sample`, `SampleQuality`, and `SampleOrigin`.
- [ ] **Step 4: Run** `cargo test -p lognook-core --test can_signals` and the focused engine tests; require all assertions to pass.

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

- [ ] **Step 1: Write failing tests** for rate/inter-arrival and byte summaries, bounded history, ISO-TP request/reply pairing with NRC preservation, known synthetic OBD/byte correlation using explicit sample timestamps, and trigger windows that contain exactly the configured pre/post frames.
- [ ] **Step 2: Run** `cargo test -p lognook-core --test can_analysis`; confirm expected failures.
- [ ] **Step 3: Implement** streaming analyzers with bounded memory and no independent transport or sample owner.
- [ ] **Step 4: Run** `cargo test -p lognook-core --test can_analysis`; require all assertions to pass.

### Task 3: Streamed raw capture and replay

**Files:**
- Create: `crates/lognook-core/src/can_capture.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Test: `crates/lognook-core/tests/can_capture.rs`

**Interfaces:**
- Consumes Task 1 `CanFrame` and Task 2 analyzers.
- Produces `CanCaptureWriter::create_new(path)`, `write_frame(&CanFrame)`, `finish()`, and a row-streaming reader that invokes a callback for each valid frame while returning row-specific parse errors.
- CSV columns: receive Unix time, elapsed nanoseconds, direction, hexadecimal CAN ID, standard/extended format, DLC, payload hex, and flags. Raw capture replay feeds the same analyzers and configured field decoder.

- [ ] **Step 1: Write failing tests** for CSV round trips, create-new behavior, raw frame order/time/format retention, malformed rows followed by valid rows, replay analyzer totals, and surfaced write failures.
- [ ] **Step 2: Run** `cargo test -p lognook-core --test can_capture`; confirm expected failures.
- [ ] **Step 3: Implement** a streaming writer/reader using the existing `csv` dependency; do not buffer a complete live capture.
- [ ] **Step 4: Run** `cargo test -p lognook-core --test can_capture`; require all assertions to pass.

### Task 4: Candlelight GS_USB device transport

**Files:**
- Modify: `crates/lognook-core/Cargo.toml`
- Create: `crates/lognook-core/src/candlelight.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Test: `crates/lognook-core/tests/candlelight_protocol.rs`

**Interfaces:**
- Consumes Task 1 `CanFrame`.
- Produces Candlelight/GS_USB enumeration, identity, bitrate configuration, listen-only receive, and transmit operations with a single claimed interface and endpoint owner.
- Uses `nusb` 0.2.7; match VID/PID and interface descriptors as detection hints, not identity proof. Keep device operations on a worker thread using the crate's blocking `MaybeFuture` API.

- [ ] **Step 1: Write failing tests** for GS_USB control request/frame encoding, bitrate validation, standard/extended receive decoding, listen-only mode, frame-size/DLC checks, timeout/bus-off/device-removal diagnostics, and send echo direction.
- [ ] **Step 2: Run** `cargo test -p lognook-core --test candlelight_protocol`; confirm expected failures.
- [ ] **Step 3: Implement** the USB transport and pure protocol helpers against official GS_USB/Candlelight behavior; add only `nusb` as the needed dependency.
- [ ] **Step 4: Run** `cargo test -p lognook-core --test candlelight_protocol`; require all assertions to pass.

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
- `LogNookEngine::connect_candlelight(CanWorkbenchConfig)`, `start_can_capture(path)`, `stop_can_capture()`, `send_can_frame(frame, pacing, repeat)`, `start_can_scan(config)`, and `stop_can_scan()` keep CAN connection/recording state under the existing engine. `is_connected`, ordinary adapter connect, and disconnect account for the one active CAN session, so existing recording gates remain correct.
- Tauri commands expose discovery, connect, configure fields, capture, send, scan/stop, and capture replay/report; frame I/O never runs on the webview thread.

- [ ] **Step 1: Write failing tests** proving only the owner reads/writes the fake transport, stop cancels before the next scan request, ranges/pacing are honored, raw TX/RX are preserved, slow capture writes apply backpressure without silent frame loss, and selected field samples enter the common recorder with source timestamps.
- [ ] **Step 2: Run** `cargo test -p lognook-core --test can_workbench`; confirm expected failures.
- [ ] **Step 3: Implement** the workbench worker, sample path, Tauri state/commands, and event serialization; refuse a second adapter while another connection is active.
- [ ] **Step 4: Run** `cargo test -p lognook-core --test can_workbench` and Tauri crate tests; require all assertions to pass.

### Task 6: CAN Workbench UI and existing graph integration

**Files:**
- Create: `apps/desktop/src/CanWorkbenchPanel.svelte`
- Create: `apps/desktop/src/canWorkbench.ts`
- Modify: `apps/desktop/src/App.svelte`
- Modify: `apps/desktop/src/types.ts`
- Modify: `apps/desktop/src/connectionControls.ts`
- Test: `apps/desktop/tests/can-workbench.test.mjs`

**Interfaces:**
- Consumes Task 5 Tauri commands/events and Tasks 1–3 frame/scope/analyzer types.
- Produces Candlelight selection/status, bitrate and listen/active mode controls, raw-frame view and per-ID rates, multi-field scope definitions, census/byte/pair/correlation views, trigger capture, raw CSV replay/export, and visible scan/send results.
- Numeric CAN fields appear as normal signal options and use `SignalGraphs`; do not add a second graph or decode frames only in Svelte. Coalesce only UI refreshes, not capture writes.

- [ ] **Step 1: Write failing UI tests** for command argument shapes, ID/field validation, multiple graph traces, mode/state transitions, replay/trigger display, and stop behavior.
- [ ] **Step 2: Run** `npm run test:ui --prefix apps/desktop`; confirm the new cases fail.
- [ ] **Step 3: Implement** the panel, typed Tauri client helpers, and graph-option integration.
- [ ] **Step 4: Run** `npm run test:ui --prefix apps/desktop` and `npm run build --prefix apps/desktop`; require all assertions/type checks/build to pass.

### Task 7: Reports, DBC skeleton, docs, and full verification

**Files:**
- Modify: `crates/lognook-core/src/can_analysis.rs`
- Modify: `apps/desktop/src-tauri/src/commands/can.rs`
- Modify: `apps/desktop/src/CanWorkbenchPanel.svelte`
- Modify: `LogNook_Blueprint/03_HARDWARE_PROTOCOLS.md`
- Modify: `PROJECT_STATUS.md`
- Test: `crates/lognook-core/tests/can_analysis.rs`

**Interfaces:**
- Consumes Tasks 1–6.
- Produces saved-capture session report and ID/payload comparison, plus optional DBC skeleton with placeholder signal definitions clearly marked as unverified.

- [ ] **Step 1: Write failing tests** for deterministic report/census output, ID/byte differences between captures, and DBC placeholder labeling.
- [ ] **Step 2: Run** `cargo test -p lognook-core --test can_analysis`; confirm expected failures.
- [ ] **Step 3: Implement** these export conveniences and update architecture/progress documentation with physical-verification limits.
- [ ] **Step 4: Run** `cargo fmt --all -- --check`, `cargo test --workspace`, `npm run test:ui --prefix apps/desktop`, and `npm run build --prefix apps/desktop`.
- [ ] **Step 5: Report** physical limitations: the RH-02 is not connected in this environment; Windows device identity, live RX/TX, bitrate, and vehicle responses remain unverified until tested with the dongle.
