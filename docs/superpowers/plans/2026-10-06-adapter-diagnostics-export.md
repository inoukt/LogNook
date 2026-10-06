# Adapter Diagnostics and Support Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record enough adapter progress, timing, PID outcomes, and errors to debug slow or failed connections, display that information in LogNook, and export a one-file support bundle.

**Architecture:** `lognook-core` emits typed, payload-free diagnostic progress from the existing serialized adapter/acquisition owner. Tauri persists a bounded JSONL event log, adds command and engine summaries, and exports it with environment and existing OEM evidence as a ZIP. Svelte displays progress/errors and lets the user optionally include one `.lognook` recording.

**Tech Stack:** Rust workspace, Tauri 2 commands/events, Svelte 5/TypeScript, existing native dialog plugin, `zip` 8.6.0 with only `deflate` enabled, Node built-in UI test runner.

**Spec:** `docs/superpowers/specs/2026-10-06-adapter-diagnostics-export-design.md`

## Global Constraints

- Persist the diagnostics JSONL at `app_data_dir()/diagnostics/operations.jsonl`.
- The new diagnostics stream contains progress, durations, IDs, and errors; it never contains measurement values or raw request/response payloads.
- Provider discovery, Identify, Connect, Disconnect, and one-shot research operations are always logged.
- Per-signal success/rate progress is aggregated at most once per second; individual errors remain immediate. Grouped poll failures include the full requested PID set.
- A failed or full diagnostics writer must not delay acquisition or drop measurement samples.
- Use a 1,024-event writer queue, a 500-event live snapshot, and rotate at 5 MiB while retaining two prior JSONL files.
- Support bundle export uses create-new semantics and includes app/OS metadata, diagnostics JSONL, and existing OEM evidence. A `.lognook` file is included only when the user selects one.
- Logging is observational; it never starts a scan, connection, reconnect, or request.
- Build the implementation in an isolated `codex/` worktree from the approved plan commit; preserve the existing modified spec and untracked laptop logs in `X:\LogNook`.
- Use `zip = { version = "8.6.0", default-features = false, features = ["deflate"] }` in the desktop Tauri crate. The workspace currently has no ZIP implementation; see [zip 8.6.0 documentation](https://docs.rs/zip/8.6.0/zip/).

## Review Focus

- A full diagnostics queue or disk-write failure must not delay or drop samples; test queue saturation and write errors in Task 4.
- Repeated polls must not create an unbounded live list or write one progress record per sample; test once-per-second aggregation and bounded history in Tasks 1 and 4.
- A failed Identify/Connect must record both its start and terminal error with elapsed time; test success and error paths in Tasks 2 and 4.
- OpenPort provider/library/J2534 stage and error detail must survive worker IPC; test progress followed by final success/error in Task 3.
- Cancelled export, an existing destination, or a failed ZIP write must preserve source recordings and remove partial output; test in Task 5.

---

### Task 1: Core diagnostic event contract and acquisition progress

**Files:**
- Create: `crates/lognook-core/src/diagnostics.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Modify: `crates/lognook-core/src/adapter.rs`
- Modify: `crates/lognook-core/src/engine.rs`
- Modify: `crates/lognook-core/src/acquisition.rs`
- Test: `crates/lognook-core/tests/diagnostics.rs`

**Interfaces:**
- `DiagnosticProgress { adapter_id: Option<String>, operation: String, stage: String, phase: DiagnosticPhase, duration_ms: Option<u64>, signal_ids: Vec<String>, detail: Option<String>, transient: Option<bool> }`, with `DiagnosticPhase::{Started, Progress, Succeeded, Failed}`.
- `DiagnosticSink = Arc<dyn Fn(DiagnosticProgress) + Send + Sync>`; adapters default to a no-op sink.
- `HardwareAdapter::set_diagnostic_sink(&mut self, sink: DiagnosticSink)` and `LogNookEngine::set_diagnostic_sink(&mut self, sink: DiagnosticSink)` install the optional event route before connecting.
- The acquisition owner emits poll start/finish events with the requested signal IDs, elapsed time, batch success/error, and transient status. Tauri derives per-signal success/rate summaries from ordinary `Sample` events.

- [ ] **Step 1: Write failing core tests** named `diagnostic_progress_round_trips_without_payload_fields`, `default_adapter_diagnostic_sink_is_noop`, `engine_installs_diagnostic_sink_before_adapter_connect`, and `acquisition_emits_poll_start_and_finish_without_changing_samples`.
- [ ] **Step 2: Run** `cargo test -p lognook-core --test diagnostics`; confirm the new cases fail.
- [ ] **Step 3: Implement** the typed progress contract and wire the sink through `LogNookEngine` into its existing adapter/acquisition owner.
- [ ] **Step 4: Run** the diagnostics integration test and focused engine/acquisition tests; require all to pass without changing sample delivery.
- [ ] **Step 5: Commit** the core contract and tests.

### Task 2: SX, ELM, and A30 Identify/Connect progress

**Files:**
- Modify: `crates/lognook-core/src/a30.rs`
- Modify: `crates/lognook-core/src/obdlink_sx.rs`
- Modify: `crates/lognook-core/src/elm327.rs`
- Test: the existing A30, SX, and ELM adapter tests in those modules

**Interfaces:**
- Connected adapter implementations receive Task 1’s `DiagnosticSink` and emit stage events with operation, phase, elapsed time, adapter ID, and relevant signal IDs. They do not include sample values or payload bytes.
- Add `identify_a30_with_diagnostics(address, sink)`, `identify_elm327_with_diagnostics(address, sink)`, and `identify_obdlink_sx_with_diagnostics(port, sink)`; existing Identify functions delegate to a no-op sink.
- A30 progress stages are Bluetooth session open, relay/identity handshake, announcement, and supported-PID discovery. SX/ELM stages are transport open, identity/initialization, protocol setup where applicable, and supported-PID discovery.
- Selected PID reads report signal ID and timing. Failed grouped reads report their signal set and exact adapter error; attach a single PID only when the adapter error identifies it.

- [ ] **Step 1: Write failing adapter tests** for stage order, elapsed timings, PID success/error association, and compatibility of existing no-op Identify wrappers.
- [ ] **Step 2: Run** focused A30, SX, and ELM test modules; confirm the new progress assertions fail.
- [ ] **Step 3: Implement** the progress sink in Identify, Connect, support discovery, and PID-read stages without changing protocol commands or polling behavior.
- [ ] **Step 4: Run** the focused adapter tests and core diagnostics tests.
- [ ] **Step 5: Commit** adapter progress and regression tests.

### Task 3: OpenPort/J2534 worker progress

**Files:**
- Modify: `crates/lognook-j2534-protocol/src/lib.rs`
- Modify: `crates/lognook-core/src/openport2.rs`
- Modify: `crates/lognook-core/src/openport2/worker_client.rs`
- Modify: `workers/lognook-j2534-x86/src/main.rs`
- Test: `crates/lognook-j2534-protocol/tests/wire.rs`
- Test: OpenPort worker/client tests in the listed source modules

**Interfaces:**
- Add `WorkerResponse::Progress { stage: String, detail: Option<String>, elapsed_ms: Option<u64> }`; the worker may send zero or more Progress responses before the final response for a request.
- `WorkerClient::request_with_progress(request, sink) -> Result<WorkerResponse, LogNookError>` forwards progress to Task 1’s sink and returns only the terminal response. Existing `request` remains a no-op-progress wrapper.
- Worker stages cover provider resolution, DLL loading, `PassThruOpen`, version read, channel connect, filter setup, Mode 01 discovery, and PID read. J2534 function/status and exact error text remain on terminal failures.

- [ ] **Step 1: Write failing wire/client tests** for Progress followed by success, Progress followed by typed error, multiple ordered progress stages, and rejection of a progress message that exceeds the worker protocol size limit.
- [ ] **Step 2: Run** `cargo test -p lognook-j2534-protocol --test wire` and focused OpenPort worker-client tests; confirm expected failures.
- [ ] **Step 3: Implement** progress message serialization, worker emission, and client filtering/forwarding while preserving the final response contract.
- [ ] **Step 4: Run** protocol, OpenPort, worker, and core diagnostics tests.
- [ ] **Step 5: Commit** J2534 progress IPC and tests.

### Task 4: Bounded Tauri diagnostics log and command outcomes

**Files:**
- Create: `apps/desktop/src-tauri/src/diagnostics.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/commands/bluetooth.rs`
- Modify: `apps/desktop/src-tauri/src/commands/openport2.rs`
- Modify: `apps/desktop/src-tauri/src/commands/oem_catalog.rs`
- Test: diagnostics and relevant command tests in the listed modules

**Interfaces:**
- `DiagnosticRecord` adds sequence ID, UTC timestamp, operation ID, adapter identity, operation/stage, phase, duration, signal IDs, optional per-signal success/rate/latency summaries, and exact error detail to Task 1’s progress fields.
- `DiagnosticsService::start(path, publish) -> Result<DiagnosticsService, String>`, `record(DiagnosticRecord)`, `snapshot() -> Vec<DiagnosticRecord>`, and `begin_operation(adapter_id, operation, signal_ids) -> DiagnosticOperation` provide append, live snapshot, and start tracking. `DiagnosticOperation::sink() -> DiagnosticSink` binds adapter stages to the operation ID; `finish(outcome, detail)` records its terminal result and elapsed time.
- The service writes through a 1,024-event `sync_channel` to a background JSONL writer, retains the latest 500 events in memory, rotates at 5 MiB with two prior files, and publishes `diagnostics-event` Tauri events. Queue or disk failures increment an omitted-event counter and surface in the live view.
- Tauri observes `EngineEvent::Connected`, `Sample`, `Error`, and `Disconnected` to maintain per-signal success/rate summaries and attach batch errors to the active adapter/signal selection without storing sample values.
- Instrument provider discovery, Identify, Connect, Disconnect, and one-shot research command success/error outcomes with operation duration and selected/supported signal IDs.

- [ ] **Step 1: Write failing Tauri tests** for JSONL ordering, operation start/end success and failure, ring/rotation bounds, queue saturation, and storage-write failure.
- [ ] **Step 2: Run** the focused Tauri diagnostics tests; confirm expected failures.
- [ ] **Step 3: Implement** the background writer, AppState wiring, live-event publishing, per-signal aggregation, and command operation instrumentation.
- [ ] **Step 4: Run** focused diagnostics, OpenPort command, OEM report, and existing desktop tests.
- [ ] **Step 5: Commit** diagnostics persistence and command logging.

### Task 5: Support bundle ZIP export

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `Cargo.lock`
- Add target-specific dependency: `windows-sys = { version = "0.61.2", features = ["Win32_Foundation", "Win32_System_Registry"] }` for OS metadata.
- Create: `apps/desktop/src-tauri/src/commands/diagnostics.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: `apps/desktop/src-tauri/src/commands/diagnostics.rs`

**Interfaces:**
- `diagnostic_snapshot() -> Result<Vec<DiagnosticRecord>, String>` returns the bounded recent-event window.
- `choose_diagnostics_recording() -> Result<Option<String>, String>` uses the native open dialog filtered to `.lognook`; cancellation means no recording is included.
- `export_support_bundle(recording_path: Option<String>) -> Result<Option<String>, String>` opens the native save dialog and writes a create-new ZIP.
- Register all three commands in `tauri::generate_handler!`; the frontend invokes them with the matching snake_case command names.
- The archive contains `metadata.json`, `diagnostics/operations.jsonl` and rotated files, and copied existing `oem-validation/*.jsonl` files. An optional user-selected `.lognook` is stored under `recordings/` by basename; it is not added unless selected.
- `metadata.json` includes app version and OS name/version/architecture. On Windows, read `ProductName`, `DisplayVersion`, `CurrentBuildNumber`, and `UBR` from `HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion`; do not collect username or machine name.
- A pure helper `write_support_bundle(destination, metadata, diagnostics_paths, evidence_paths, recording_path)` is testable without a Tauri window, refuses overwrite, validates the optional session file, and removes partial output on failure.
- Copy the selected `.lognook` into the archive as a stream; do not load the recording into memory.

- [ ] **Step 1: Write failing archive tests** named `bundle_contains_metadata_diagnostics_and_oem_evidence`, `bundle_omits_recording_by_default`, `bundle_includes_only_the_selected_recording`, `bundle_refuses_overwrite`, and `failed_bundle_write_removes_partial_file`.
- [ ] **Step 2: Run** the focused Tauri archive tests; confirm expected failures.
- [ ] **Step 3: Implement** the ZIP writer using `zip` 8.6.0 with `deflate` only and register the snapshot/choose/export commands.
- [ ] **Step 4: Run** the focused archive and command tests; verify the original recording and evidence files remain unchanged.
- [ ] **Step 5: Commit** support bundle export and tests.

### Task 6: Diagnostics view and export controls

**Files:**
- Create: `apps/desktop/src/DiagnosticsPanel.svelte`
- Create: `apps/desktop/src/diagnostics.ts`
- Modify: `apps/desktop/src/SettingsPanel.svelte`
- Modify: `apps/desktop/src/App.svelte`
- Modify: `apps/desktop/src/types.ts`
- Test: `apps/desktop/tests/diagnostics.test.mjs`

**Interfaces:**
- `diagnostics.ts` formats adapter/operation/stage/status/duration and filters by adapter, operation, and error state; it never formats packet payloads or sample values.
- `DiagnosticsPanel.svelte` loads the Tauri snapshot, listens for `diagnostics-event`, displays progress/errors and per-signal success/rate summaries, and offers **Export support bundle** plus optional recording selection.
- `SettingsPanel.svelte` opens/closes the Diagnostics view. `App.svelte` owns the Tauri listeners and export invocation; it does not duplicate the diagnostics logger.

- [ ] **Step 1: Write failing UI tests** named `diagnostics_panel_loads_snapshot_and_appends_live_events`, `diagnostics_filters_select_adapter_operation_and_errors`, `diagnostics_view_never_renders_packet_or_sample_values`, `export_cancellation_keeps_panel_state`, and `support_export_passes_optional_recording_path`.
- [ ] **Step 2: Run** `npm run test:ui --prefix apps/desktop`; confirm the new cases fail.
- [ ] **Step 3: Implement** the typed UI panel and Settings entry using the existing theme and control styles.
- [ ] **Step 4: Run** UI tests and `npm run build --prefix apps/desktop`; require Svelte check to report zero errors/warnings.
- [ ] **Step 5: Commit** the Diagnostics view and tests.

### Task 7: Architecture docs and full verification

**Files:**
- Modify: `LogNook_Blueprint/02_ARCHITECTURE.md`
- Modify: `PROJECT_STATUS.md`
- Test: full workspace and desktop verification

**Interfaces:**
- Documents the event schema/storage bounds, command/adapter instrumentation, export contents, and how to attach the optional session recording.
- Records the next physical acceptance test: repeat A30 Identify/Connect and an OpenPort/OPTA attempt on the laptop, then export one bundle for review.

- [ ] **Step 1: Update** architecture and project status to describe the diagnostics log, Diagnostics view, support ZIP, and physical test state.
- [ ] **Step 2: Run** `cargo test --workspace` and `cargo check --workspace`.
- [ ] **Step 3: Run** `npm run test:ui --prefix apps/desktop` and `npm run build --prefix apps/desktop`.
- [ ] **Step 4: Run** `cargo fmt --all -- --check`, targeted rustfmt for changed Rust files, and `git diff --check`; document any pre-existing workspace formatting failures.
- [ ] **Step 5: Commit** documentation and report exact verification results and hardware limitations.
