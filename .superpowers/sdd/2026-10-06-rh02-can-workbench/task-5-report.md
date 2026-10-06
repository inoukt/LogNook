# Task 5 report: Serialized CAN workbench session and commands

Status: implementation and review fixes complete. Original implementation commit: `92189447936f4d644bb53de0b66dfb4f4c5c8639`.
Branch: `codex/rh02-can-workbench`.
Workspace: `C:\Users\inouk\.codex\worktrees\rh02-can-workbench\LogNook`.

## Implemented

- Added an engine-owned `CanWorkbench` session and a private serialized worker/scan implementation. The workbench worker is the only caller of `CanTransport`; the Candlelight handle continues to serialize actual USB endpoint access on its Task 4 owner thread.
- Added Candlelight USB hint discovery, explicit bus/address/interface/channel selection, identity-checked open, 500 kbit/s-capable bitrate configuration, and listen-only or active mode. Open does not send a CAN frame or begin a scan.
- Added manually configured frame sends, finite standard/extended CAN-ID scans, cancellable Mode 21 and Mode 22 ranges, Mode 01 support-page discovery, and selected Mode 01 PID polling. Active operations are user-started, pacing and repeat bounds are validated, and `Stop` is checked between frames/scan requests.
- Mode 21/22 and Mode 01 requests use their configured request and response CAN IDs. ISO-TP single-frame and multi-frame replies are reassembled, with serialized flow control for multi-frame responses. Typed outcomes preserve positive, wrong echo, malformed, negative/NRC, response-pending, and no-response results. No session changes, security access, writes, or automatic polling were added.
- The transport and workbench now share one monotonic session origin for Candlelight RX and workbench TX timestamps. Raw RX is appended to capture and event batches before scan parsing can submit ISO-TP flow control, preserving wire order for replay.
- Each `.candump.csv` capture requires a paired create-new `.candump.jsonl` journal. Scan/send commands are refused without an active pair; partial creation rolls back the sidecar. The streaming journal stores session/scan configuration (including candidate provenance), typed results linked to exact TX/RX frames, TX attempt/submission separately from TX-echo confirmation, and structured transport errors with raw USB bytes. A persisted DTO keeps candidate provenance readable while command deserialization ignores forged `source_candidate_id` values.
- Mode 01 parser/decoder failures now produce `Malformed` outcomes and error events. Mode 01 sample sources use the connected adapter identity. Finite scans emit `CanScanCompleted`; operator cancellation remains `CanScanStopped`.
- Added source-backed OEM reads through the catalog's existing candidate validation. The exact catalog candidate ID is carried in the scan configuration/result events; request and response frames remain raw evidence and do not update decoder or vehicle-validation state.
- Added configurable CAN signal fields. Received frames decode through `SamplePublisher` into the existing live recording pipeline with source receive timestamps, CAN ECU ID, field unit, and raw frame bytes. Selected standard Mode 01 samples use the same pipeline and preserve received timestamps and frame bytes.
- Added create-new `.candump.csv` capture, synchronous worker-side writes, batched `EngineEvent::CanFrames`, scan-start/result/stop events, adapter status events, and bounded capture replay/report commands. The capture worker writes each frame before reading the next one, so slow storage applies backpressure instead of dropping frames in an application queue.
- Replay reports continue counting and decoding valid rows after the analyzer's 2048-ID census cap, separate analyzer issues from malformed CSV, decode only RX samples, and summarize the paired JSONL journal. CSV and JSONL readers recover after bounded row errors; diagnostic response retention is capped at 600 frames while every bus frame remains in raw CSV. Stopping capture cancels pending scans and repeated manual TX before another request is sent.
- Engine connected/recording gates now account for a live CAN session. Ordinary adapter connections are refused while CAN is active, and engine disconnect stops the CAN owner and finalizes capture/recording.
- Split responsibilities into `can_workbench.rs`, `can_workbench/worker.rs`, and `can_workbench/scan.rs` so the public engine API, worker lifecycle, and scan/ISO-TP logic remain focused.

## Tauri commands

- `available_can_adapters`
- `connect_candlelight`
- `configure_can_signals`
- `start_can_capture` / `stop_can_capture`
- `send_can_frame`
- `start_can_scan` / `stop_can_scan`
- `start_can_oem_candidate`
- `replay_can_capture_report`

USB enumeration/opening and frame reads/writes run in Rust-owned workers. Tauri commands only request work and receive acknowledgements; live frame events and measurement samples use the application event/sample pipelines.

## RED/GREEN and verification

The initial `cargo test -p lognook-core --test can_workbench` was RED on the not-yet-existing workbench APIs. A new 128-frame burst deadline test was also RED before the receive loop change: 128 ready frames took about 0.78 seconds because command `recv_timeout` imposed a 5 ms delay before every read. After replacing that per-frame wait with nonblocking command checks, the same burst test completed in under 500 ms (about 0.04 seconds in the focused run), while stop/cancellation checks still passed. In this review round, ordering and shared-clock regressions reproduced both issues; the sidecar provenance round trip caught that the command-safe `skip_deserializing` field also erased provenance when reopening evidence, leading to the persisted DTO fix.

Final commands used `$env:CARGO_TARGET_DIR='X:\LogNook\target'`:

- `cargo test -p lognook-core --test can_workbench`: **17 passed**. Covers serialized transport ownership, timestamp ordering, journal-required active operations, sidecar rollback and provenance/result round trips, ordered multi-frame capture, TX submission vs echo, raw USB error evidence, malformed Mode 01 handling, adapter-derived sample source, recording integration, natural completion, cancellation, bounded response retention with complete raw capture, ranges, pacing, and no-loss bursts.
- `cargo test -p lognook-core -- --quiet`: **239 passed, 0 failed** across unit and integration tests.
- `cargo test -p lognook-desktop --lib -- --quiet`: **18 passed, 0 failed**. Replay tests cover analyzer capacity, later-row RX decoding, journal summary, and oversized JSONL recovery.
- `cargo test -p lognook-core candlelight_error_adaptation_preserves_the_exact_usb_packet`: passed.
- `cargo check -p lognook-desktop`: passed.
- `cargo test -p lognook-desktop -- --quiet`: **18 passed, 0 failed**.
- `rustfmt --edition 2024 --check --config skip_children=true` on changed Task 5 source/test files: passed.
- `git diff --check`: passed.

One initial full core run hit the existing timing-sensitive playback test `deleting_late_marker_during_playback_clamps_without_restarting`; its isolated rerun passed, as did the subsequent full quiet core rerun.

## Hardware limits and next task

The RH-02/Candlelight adapter is not connected here. USB identity on the physical device, real receive throughput, bus-off/removal behavior, and vehicle replies remain unverified. The first hardware acceptance task is to enumerate and connect the RH-02 in listen-only mode, capture without transmitting, then try a selected Mode 01 PID and one source-backed Mode 21/22 candidate while retaining the raw capture.

Next planned implementation task: Task 6, build the Svelte Test Workbench UI against these Tauri commands/events and feed selected CAN field samples into existing graphs.
