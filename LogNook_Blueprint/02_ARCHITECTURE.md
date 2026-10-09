# Engineering architecture and repository

## Proposed stack
- Tauri 2 desktop shell; Rust acquisition/storage/processing engine; Svelte + TypeScript UI; CSS variables/theme tokens.
- Windows J2534 calls run in a separate architecture-matched worker. The implemented OpenPort 2 path uses the vendor-installed 32-bit J2534 v04.04 provider in `lognook-j2534-worker.exe`; the Tauri process never loads the vendor DLL. A bounded typed JSON-lines protocol and response deadlines isolate the x64 app from the x86 driver.
- Serial for OBDLink SX/ELM; native `gs_usb` USB for compatible candleLight devices using evaluated pure-Rust `nusb` or `rusb` and Windows WinUSB binding; SLCAN over serial. Linux backend later maps `gs_usb` to SocketCAN.
- A small, controlled renderer interface: uPlot for very fast time-series and Apache ECharts for scatter/heatmap/bar. Their two roles are explicit; don't implement two competing graph data pipelines.
- SQLite local state and per-session recording; migration-on-open with backup and schema-version checking. Credential secrets in OS credential storage, never in JSON config.

## Pipeline
`HardwareAdapter -> RawTransportEvent -> ProtocolSession -> DecodedSample -> EventBus -> Recorder / SchedulerFeedback / TriggerEngine / UIThrottler / AlertEngine`.
Raw CAN monitoring takes a parallel path to the raw-frame recorder/DBC decoder. Derived signals subscribe to decoded sample events; formulas do not alter source measurements. Recorders never rely on the chart's selected signals if acquisition is configured to retain more.
The live CAN monitor batches UI updates per animation frame and maintains cumulative RX/TX counts by 11-bit or 29-bit ID for the current connection. Each summary row shows the latest DLC/payload/ASCII and average RX inter-arrival period/rate; reconnect resets the counters. The raw frame view keeps only the latest 400 frames for display, while the paired capture writer retains the full traffic stream.
The pending CAN display queue is also capped at 400 frames when rendering is throttled; cumulative per-ID summaries consume every received batch before that display limit. Adapter-level PID parse/decode failures are signal-scoped: good samples from the same poll are retained and the bad PID is reported, while transport and ECU/session failures still end acquisition.
Live chart rendering keeps its recent display series separately from the live CSV ring (100,000 samples). On-screen CSV export includes currently graphed live signals, including graph-only research/CAN signals, and reports when the oldest samples were overwritten. A trace is marked stale after at least five seconds, or three recent sample intervals if longer; stale live traces stop extending the latest point across the chart.
When CAN Workbench is selected, the monitor fills the upper right pane and the decoded-signal graph remains below it. A draggable, keyboard-adjustable divider changes their height; adapter and scan controls stay in the left pane.

## Shared contracts
The sample worker also retains a bounded 100,000-sample live history for **Rec From Connect**, separate from the automatic trigger's 30-second buffer and the UI render/CSV buffers. A serialized start message writes retained samples with their original timestamps, source, ECU, units and raw bytes, adds a persistent backfill/eviction marker, and begins recording new samples without re-emitting the history to the UI. The last source disconnect clears this history; disconnecting only OBD or CAN while the other remains connected preserves it. Replay samples never enter this buffer. Both retrospective and automatic-trigger history are written in a single SQLite transaction using cached inserts; rollback restores rows and the writer's count/duration/signal summary.

Session management validates canonical native files within the recordings directory. Rename refuses invalid names and existing targets; rename/delete refuse loaded sessions, active recording and SQLite sidecars. Closing the viewer unloads its backend session and invalidates the old playback identity. Folder opening uses the platform file manager. Web sharing creates a temporary validated copy while holding the session lock, releases it before HTTP, streams the native recording to Catbox only after the user's Upload action, and removes its temporary copy on completion/error. Returned links require HTTPS on `files.catbox.moe`; response size, upload size and network time are bounded. No account, credentials, link shortener or automatic startup upload is introduced. Provider protocol/retention: [API](https://catbox.moe/tools.php), [FAQ](https://catbox.moe/faq.php), [terms](https://catbox.moe/legal.php).

`DeviceDescriptor {stable_id, hardware_family, firmware, path}`
`ConnectionCapability {protocols, bus_types, supports_passive, max_channels, supported_rates, timestamp_capability, uncertainties}`
`SignalKey {protocol, ecu_address, identifier, vehicle_profile_variant}`
`SignalDefinition {key, name, meaning, dimensions, decode_spec, source_definition_version}`
`Sample {key, t_monotonic_ns, t_wall_utc, raw_bytes_or_value, canonical_value, unit, quality, source_id}`
`Event {timestamp, source, kind, payload}` / `Marker` / `TriggerRule` / `Session`.
All domain models are defined in a single common crate and serialized through schema-versioned APIs. Adapter implementations depend on the common interfaces; the engine never imports vendor-specific types.

### Adapter progress diagnostics

`HardwareAdapter::set_diagnostic_sink` installs a synchronous, prompt callback on the serialized adapter owner. SX, ELM327, and A30 emit metadata-only started/completed stage events for Identify/Connect and selected PID reads, including duration, stable adapter identity when known, and signal IDs. Grouped SX responses report one shared duration and the full requested signal set; errors remain the original adapter errors with only command/payload fragments redacted in diagnostic detail. Identify wrappers preserve their adapter-only behavior. The desktop consumer owns persistence and coalescing of successful read timing; adapter instrumentation never starts requests or changes sample delivery.

### Persisted diagnostics and Diagnostics view

Tauri starts `DiagnosticsService` at `app_data_dir()/diagnostics/operations.jsonl`. A `DiagnosticRecord` contains a sequence ID, UTC Unix timestamp in milliseconds, operation correlation ID, optional adapter identity, operation/stage, snake-case phase (`started`, `progress`, `succeeded`, `failed`), duration, requested signal IDs, optional success/rate/latency summaries, error detail, transient disposition, and omitted-event count. Connect commands finish their own operation; ongoing acquisition has a separate correlation ID ending at Disconnect. Provider discovery, Identify, Connect, Disconnect, and one-shot research command boundaries record success/error and elapsed time. OpenPort worker progress crosses the bounded IPC channel for provider resolution, DLL loading, device open/version, channel/filter setup, support discovery, and PID reads without extending the existing absolute request deadline.

Per-signal reliability summaries include `requested`, `supported`, cumulative `attempt_count`, `success_count`, and attributable `error_count`, optional `first_response_delay_ms`, observed response rate, and average selected-read latency. An attempt means inclusion in an acquisition poll's requested signal set; it does not count CAN frames or assume every member was transmitted after an aborted grouped read. A singleton adapter `pid_read` failure increments that signal's error count. Grouped failures remain full-set batch records and do not create invented per-PID causes or inflate each signal's error count. Each failed acquisition poll now includes the core owner's `consecutive_failures` count, including nonfatal retry failures. First-response delay uses callback monotonic timestamps from the first acquisition poll attempt to the first valid acquired sample, excluding Connect time. Selected signals with no samples remain visible with zero successes and absent first-response timing, including a final zero-attempt summary if disconnect precedes the first poll. Statistics retain at most 500 signal entries per session and report that limit when additional requested IDs cannot be summarized. Unselected and explicit OEM/A30 research sample sources do not grow or inflate acquisition reliability statistics. Counts are best-effort observations; queue omissions remain visible through the existing omitted-event counter.

OpenPort provider discovery emits at most 64 inventory/explanation rows plus one totals/omissions row through the existing diagnostic sink. Eligible providers include their safe stable registry IDs and names; recognized worker explanations retain ignored-provider identity, unsupported-family reasons, missing FunctionLibrary/DLL reasons, and missing registry/eligible-provider states. Labels are bounded to 128 bytes and reject path/control characters; unrecognized explanation fragments are replaced with a payload-free omission message. The returned provider descriptors and explanation strings remain unchanged for existing callers. These observations do not start another discovery or vehicle request.

The serialized adapter/engine owner offers observations to a background writer with nonblocking `try_send` through a 1,024-item queue. Persistence and live `diagnostics-event` publication coalesce successful poll/read observations into per-signal summaries at most once per second, with a final nonempty summary on disconnect; individual failures retain immediate handling and grouped failures keep all requested signal IDs. The live history holds 500 records. JSONL rotates at 5 MiB to `operations.1.jsonl` and `operations.2.jsonl`. A full queue or failed disk writer increments the omitted counter and surfaces feedback through the live history; diagnostics loss never blocks acquisition or removes measurement samples. Event wall timestamps and rate windows currently use background processing time and can shift under backlog; first-response delay separately uses callback monotonic timestamps.

Settings opens the Diagnostics modal. App owns one Tauri diagnostics listener; the panel merges its snapshot with live events by sequence ID and retains the latest 500. Filters select adapter, operation, and failures. Rows show duration, progress/errors, requested/supported state, requested poll attempts, successes, attributable PID errors, first-response delay or no-response state, rates, average read latency, and failed-poll streaks. Provider inventory and ignored/missing-driver explanations appear in the same bounded log. The formatter explicitly projects metadata fields, so unexpected packet/sample fields are not rendered. Neither the diagnostic contract nor its JSONL includes sample values or raw request/response bytes; payload-bearing adapter error fragments are sanitized. Existing OEM evidence remains a separate raw/provenance record. Diagnostics never initiates discovery scans, connections, reconnects, or diagnostic requests.

### Native support bundles

The desktop `diagnostic_snapshot` command returns the diagnostics service's bounded recent window. `choose_diagnostics_recording` offers a native `.lognook` picker; `export_support_bundle` offers a native ZIP save dialog and creates a new destination on a blocking worker without acquiring the engine/adapter owner. Bundles contain application/OS metadata, existing `operations.jsonl` and its two rotations, and existing app-data `oem-validation/*.jsonl` evidence. They include exactly one recording under its basename only when explicitly selected, including external paths. Files are streamed with ZIP64 enabled for every source entry (including recordings above 4 GiB), sources are unchanged, and a failed write removes only its newly created partial ZIP. The panel defaults to no recording; Choose recording selects one `.lognook`, Remove recording clears it, and Export support bundle opens the native save dialog. Cancelling either dialog preserves the selected recording and retained diagnostics. Windows version fields (`ProductName`, `DisplayVersion`, `CurrentBuildNumber`, `UBR`) come from the OS registry; username and machine identity are not collected. Export reads a bounded prefix of each live source, so concurrent logging/rotation is not an atomic snapshot and a rotation race may require retrying the export. Non-Windows OS version metadata is currently null. Native dialog cancellation clears earlier error feedback, a deferred minor UI issue. Archive fixtures cover contents, explicit recording selection, create-new refusal, source preservation, partial cleanup, and ZIP64 headers; no multi-gigabyte physical export or native dialog acceptance run has been performed.

## Ownership and concurrency
- One writer/serialized command owner per physical hardware interface; bounded read and decode queues; bus/session scheduler controls requests and timeouts.
- Separate high-priority ingest and recorder workers from low-priority graph refresh, screenshot, indexing, sharing and AI tasks. Slow consumers receive pressure metrics and drop optional UI previews, NEVER silently drop saved samples.
- Explicit connection state machine: absent -> detected -> probing -> open -> bus-ready -> ECU-ready -> streaming -> degraded -> backoff -> recovering. Distinguish USB re-enumeration from ECU timeout. Resume only non-destructive session configuration; timestamps and gap markers expose each reconnect.

## Suggested small repository
```
lognook/
  Cargo.toml                    # workspace
  apps/desktop/                 # Tauri shell and thin commands
  ui/src/{routes,components,stores,charts,theme}/
  crates/model/                 # canonical contracts and errors
  crates/engine/                # scheduler, sessions, connection state
  crates/protocols/             # OBD, ISO-TP, UDS foundations
  crates/adapters/              # serial, gs_usb, slcan traits/impl
  crates/recording/             # SQLite, CSV, import/export, replay
  crates/analysis/              # triggers, formulas, comparison, alerts
  workers/lognook-j2534-x86/   # separate Windows crash/bitness boundary
  profiles/{generic,examples}/  # licensed/reviewed vehicle definitions
  tests/{fixtures,simulators}/
  docs/{decisions,protocols}/   # short ADRs only as decisions arise
```
Each module owns a single public contract. Avoid abstract 'plugin systems' and massive registries before at least two concrete implementations need an abstraction. Use a centralized typed settings registry with scoped overrides: global -> device -> vehicle -> workspace -> signal. Generic reusable components, not global variables.

## Compact **code** files and centralized reuse (the size requirement applies to source, not `.md` docs)
- Prefer cohesive, single-responsibility Rust/TypeScript source modules. Approximately **100–250 lines per hand-written source file is a review signal**, not a forced limit: split when responsibilities diverge, not simply when a line counter trips. Generated code, strict FFI definitions and mature algorithms may reasonably exceed this size.
- Keep a few intentional domain crates instead of hundreds of tiny crates. Split internally by operation (`probe.rs`, `poll.rs`, `reconnect.rs`, `csv_export.rs`, `trigger_rules.rs`) only when those are real, separately testable responsibilities. No file should be a dumping ground for unrelated helpers.
- The source of truth for **units, numeric transforms, timestamps, signal identifiers, device capabilities, errors, preferences and recording events** must be shared. Hardware implementations supply bytes/capabilities; they do not duplicate PID menus, trigger code, CSV encoding, conversion code or graph state.
- Put customization behind explicit typed configuration and well-named pure functions. Do **not** use global mutable state or copy-and-paste feature variants. Add an adapter implementation to the shared trait and give it the common test suite.
- Prefer one small public API per module with implementation helpers kept private. Add fixtures/property tests beside parser/decoder/scheduler functionality. Require formatting, lints, API-contract checks and regression tests in CI.
- Documentation has **no artificial small-file requirement**: clear in-depth protocol notes and long research documents are preferable to hiding information across many fragments. Keep brief ADRs for individual decisions where useful.

## Portability
No Windows APIs outside Windows adapter/worker and distribution modules. No frontend direct COM/USB or filesystem device access. Linux effort should consist mainly of platform adapters/packaging, not a second engine. Build Windows x64, separate x86 J2534 helper; reserve continuous integration for Linux engine tests from day one.

## Milestone 1 implementation

- The Cargo workspace has one domain crate, `crates/lognook-core`, and one Tauri app at `apps/desktop/src-tauri`. Svelte 5, TypeScript, and Vite live under `apps/desktop`; no empty subsystem crates were created.
- `HardwareAdapter` owns connect/read/disconnect. The simulator and SX use the same capability/signal/sample contracts. SX discovery and opening require exact USB VID/PID `0403:6015`; the selected COM port opens at 115200 8-N-1 with no flow control.
- The SX handshake begins with the complete adapter-only query `STI\r`. Its firmware reply and `>` synchronize the connection. The adapter then queries STDI and STSN, waiting for each reply prompt. An already-powered device is not required to emit a fresh startup banner on port open; a queued startup banner or empty prompt is tolerated before the STI reply. Missing or unexpected replies stop before further commands.
- Identify SX is a separate identity-only UI action. Live connection repeats identity, scans Mode 01 support ranges starting at `0100` and follows continuation flags (`0120`, etc.), then reads only selected supported signals from the shared catalog. The current production adapter sends no wake/reset, saved-protocol configuration, DTC-clear, OEM/UDS, actuator, or ECU-write commands. This describes current acquisition behavior, not the complete physical-test policy: separate source-backed read probes and state-changing test approval follow root `AGENTS.md`.
- SX serial ports are enumerated at launch, on adapter selection/focus, and every two seconds while SX is selected and disconnected. Refreshes preserve a still-present COM selection and clear identity when the selected port disappears.
- Tests include synthetic protocol fixtures and physical Ford/SX identity, supported-PID, and grouped-response captures. They cover continuation parsing, absent/optional startup frames, wrong firmware/device, silent replies, USB matching, malformed/truncated responses, multiple responders, PID capability filtering, all 35 seed decoders, and selected reads. Group construction is checked to cover each catalog signal exactly once in seven responses of at most two CAN frames. The live rate bench captured all three supported-PID ranges, including raw `0120` response `41 20 A0 07 F1 19`, and validated positive responses for the 35 tested auto-requestable generic candidates. The Ford advertised six additional structured or multi-channel PIDs that remain review-required. One earlier UI session showed a Mode 01 PID 05 timeout, which did not recur in the later five-signal recording.
- Acquisition serializes adapter reads at a 100 ms target and submits timestamped samples to a bounded 256-item synchronous queue. A slow recorder applies backpressure. The UI consumes pipeline events rather than synthesizing measurements.
- `LogNookError` carries an explicit transient/fatal disposition. SX no-prompt timeouts with no response payload (an optional command echo does not count as payload) and completed reads with no requested PID response are transient; acquisition retries after 250 ms, resets the streak on a successful cycle, and disconnects after three consecutive transient failures. Partial/malformed replies, serial writes, negative ECU responses, and decode/range failures remain fatal. OpenPort worker timeouts remain fatal because the isolated worker is killed.
- Mode 01 parsing preserves no-response separately from a `7F` negative response. The latter retains ECU, requested PID/service, echoed service, and NRC for diagnostics; failed reads never synthesize samples.
- Samples retain monotonic elapsed time, UTC receive time, source/ECU, units, quality, origin, and optional raw OBD bytes. Missing ECU-address headers remain unknown; no ECU address is fabricated.
- A recording is a new SQLite `.lognook` file with WAL during acquisition. Clean close checkpoints and verifies integrity. Original sample timestamps, raw bytes, quality, and separate markers are retained. Finalized files cache sample count, sample duration, and signal/unit pairs in metadata; graph lookups use time and `(signal_id, elapsed_ns, id)` indexes. Opening a legacy v1 file adds the indexes and summary once when writable; read-only files fall back to the original scan.
- An adapter disconnect disarms an untriggered automatic rule and clears its rolling pre-trigger samples. If an adapter read failure disconnects during an active recording, the partial recording stays open for an explicit stop; another adapter cannot connect until that recording closes, avoiding mixed elapsed-time origins.
- LogNook CSV is long-form with `raw_hex` and typed `event_type=sample|marker` rows. Export refuses overwrites. The importer accepts older sample-only CSV and imports legacy `*-markers.csv` sidecars only when inline marker events are absent. CSV import streams into a new SQLite session and preserves the selected source. Playback reads ordered session events incrementally and feeds samples through the same bounded pipeline with `origin=replay`.
- FORScan wide logs with a `time(ms)` column are also imported: comma or semicolon delimiters are detected, numeric PID columns and parenthesized units become samples, duplicate signal headings receive a suffix, `Yes`/`No` flags map to 1/0, and nonnumeric placeholders are skipped. Their elapsed millisecond timestamps are converted to nanoseconds; since these exports have no absolute time, the importer anchors wall-clock timestamps at import time.
- Live CSV snapshots serialize the UI's retained live samples (at most 180 per logged signal) through the shared CSV model without opening a recording. Saved sessions offer CSV sharing (samples plus markers) and a native `.lognook` Save As copy; export commands canonicalize source sessions under the app recordings directory before reading or copying them.
- Tauri owns app-local recording paths and bridges the engine to Svelte. The PID picker and combined/separate graphs share timestamped pipeline samples with recording and replay. Recording queries, session browser/file operations, adapter discovery and connection, OEM reads, and CSV exports are async Tauri commands so their blocking work does not run on the WebView thread. Full-session CSV export releases the engine mutex before writing.

### OpenPort 2.0 Mode 01 implementation — 2026-10-04

- `workers/lognook-j2534-x86` is a 32-bit Windows process. It enumerates only registered 32-bit J2534 04.04 providers named for Tactrix, OpenPort, or OPTA whose expanded DLL paths exist; it resolves the selected registry ID again before loading that DLL. The user selects the provider explicitly.
- The worker opens/closes providers for identity. Acquisition uses J2534 ISO 15765, 11-bit CAN at 500 kbit/s, fixed standard functional Mode 01 requests, and one serialized provider owner. It installs per-responder `FLOW_CONTROL_FILTER`s for `7E8`–`7EF` mapped to request IDs `7E0`–`7E7`, ignores transmit-confirmation indications, and retains partial receive batches returned with timeout. `PassThruWriteMsgs` cannot receive caller-supplied bytes or headers. Responses retain each ECU ID and raw Mode 01 payload.
- The core `OpenPort2Adapter` validates supported-PID pages from `00` through `E0`, advertises only supported signals in the shared catalog, rejects malformed replies or multiple ECU responders, and produces ordinary timestamped `Sample` values through `HardwareAdapter` and `LogNookEngine`. Its target cycle is 100 ms; ECU and transport latency can make the observed cycle slower.
- Provider identity/listing and selected PID reads have response deadlines. An unresponsive worker is killed and reported. Normal close sends a worker disconnect before process shutdown. IPC is capped at 16 KiB per line.
- Tauri exposes provider listing, identity, and connect commands. The top toolbar chooses Simulator, OBDLink SX, or OpenPort 2, displays controls for the selected device, and uses one Connect button. Simulator and SX startup do not build or launch the OpenPort worker.
- Provider discovery reports eligible 32-bit J2534 04.04 providers and explains ignored registry entries or missing DLLs; the UI can refresh that inventory. Identify and Connect remain manual. Errors name the failing operation, and a no-response result records the Mode 01 PID and elapsed wait. LogNook does not automatically switch providers or protocols after a failure.
- Build and test with `rustup target add i686-pc-windows-msvc`, `npm run build:j2534-worker` from `apps/desktop`, then `npm run tauri dev` from `apps/desktop`. The x86 MSVC linker and Windows SDK are required. Release builds look in `target/i686-pc-windows-msvc/release`; worker packaging is not enabled while Tauri bundling is disabled.
- Physical validation remains pending. The earlier OPTA passive CAN probe established identity and channel setup, then rejected an aggregated raw frame; no vehicle diagnostic request was sent in that probe. The current development host lists no matching OpenPort J2534 provider. Do not claim the new Mode 01 path works on a vehicle until it returns supported-PID replies and live samples through the app.

### CSV session player

- `LogNookEngine` loads an inspected `.lognook` session and owns one interruptible playback worker. Pause, resume, seek, speed changes, and stop do not wait for a long event gap. Playback state is serialized as position, duration, speed, and playing status and is sent through the pipeline event stream, with progress capped at 20 updates per second.
- Imported CSV sessions are streamed to a temporary SQLite file, integrity-checked, then published with a same-directory hard link so an existing target is never replaced. On filesystems without hard-link support, import fails safely and removes the temporary file.
- Marker IDs are session-local SQLite IDs. Add, edit, and delete operations pause playback until its read cursor closes, commit the marker change, then resume at the same position. CSV stores marker time, wall time, label, and note, but not the local ID.
- Elapsed times must be non-negative. CSV sample rows reject marker-only fields, marker rows reject measurement fields, and an absent marker note becomes an empty note. Errors include the CSV row number.
- Playback events and state carry a monotonically changing session ID. The UI ignores queued samples, markers, or state from a session that has since been replaced. A paused seek queries the latest stored value for each selected signal at or before the playhead.
- The UI lists the signals present in the session. Catalog metadata is reused for known generic PIDs; unknown signal keys remain identifiable without inventing a PID. Graph samples come from a bounded SQLite window query that keeps first/minimum/maximum/last points per signal bucket (up to 2,000 buckets); saved samples stay full resolution.
- Combined and separate graph layouts share one zoom/pan time window and draw marker positions plus the playhead. The player timeline, marker list, and PID list use the dark-slate scrollbar and teal accent styling.
- Playback follows a monotonic virtual clock and emits only the latest replay readout per signal at a 50 ms cadence; markers flush pending readouts in order. This avoids per-sample UI backpressure at high speeds while preserving recorded samples in SQLite.
- The graph viewport remains centered on the playhead at every replay update, including at session edges; its displayed range may extend beyond the recording while SQLite queries clamp to available samples. Dragging the recording-position slider and optional wheel scrubbing both update the cursor and viewport immediately, while backend seeks are coalesced. Graph-window and average refreshes are throttled to 100 ms.
- Replay graph queries use 360 time buckets and retain each bucket's first/minimum/maximum/last points, up to 1,440 per signal. Replay rendering plots the entire bounded query across the visible time window; the 180-sample cap remains for live graphs and saved references. The PID panel is hideable; graph paint is clipped to its column and kept below the PID panel's layer.
- The PID list toggle stays in a narrow rail beside the list across responsive layouts. Closing a loaded session stops replay, clears session readouts, and restores the live graph selection.
- PID colors and the optional wheel-scrub setting are local renderer preferences. The graph click seeks the shared playback cursor; adding a marker at that cursor accepts blank label and note. Averages use the visible graph range (live retained points or SQLite `AVG` for sessions).
- The v0.3 OEM SQLite catalog is resolved from a Tauri resource in packaged builds and from `PIDs/LogNook_PID_Master_v0_3/data/lognook_pids.sqlite` in development. Rust opens it read-only per paged search; candidate records retain service/DID, request/response headers, decoder, applicability, source SHA, and license. The Test Workbench's OEM picker is separate from the ordinary Mode 01 `SignalDefinition` list, so unvalidated entries cannot be automatically polled or added as ordinary PIDs. A finite source-backed one-shot can emit a graph sample when the decoder returns a finite numeric value; the sample retains research identity and never changes candidate validation.
- The legacy Scion xB backend probe re-reads the pinned Scion source definition and accepts only its reviewed 2014 profile, service `21`, identifier `01`, request `7E0`, response `7E8`, and five fixed byte-aligned decoder channels. It identifies the SX, requires CAN 11/500, then sends exactly `STPX H:7E0, D:2101, T:250`. It reassembles ISO-TP and returns raw frames plus candidate values; outcomes are appended to app-data `oem-validation/scion-xb-mode21.jsonl`, separate from the read-only catalog. Active acquisition or recording blocks this legacy command. Its dedicated OEM Research card has been removed; current candidate and standard-mode tests use the queued workflow below.
- The captured 2014 xB `21 01` reply is a 28-byte ISO-TP PDU (`61 01` plus 26 data bytes); the probe also accepts the earlier 29-byte form. It preserves the raw reply and decodes only its five selected candidate channels. Their source definitions remain unvalidated, and other captured identifiers are not automatically polled.
- Each eligible OEM catalog row has a **Try once** action for OBDLink SX, paired Bluetooth ELM327, and Xtool A30/A30M. The Tauri command accepts only a candidate ID, re-reads that row from SQLite read-only, checks source SHA/license and identifier width, and uses its exact request/response headers. SX uses STPX; ELM sends the source-backed service/PID or DID after selecting the exact CAN header; A30 wraps it in a captured FFEC record over SPP. Candlelight's explicit CAN read passes the trusted catalog spec to its serialized scan owner and uses the same OEM decoder on the received ISO-TP frames. Typed outcomes preserve negative NRC, response-pending, temporary unavailability, session/security requirements, no response, malformed data, and wrong responder. Raw response and provenance append to app-data `oem-validation/oem-candidate-tries.jsonl` for serial/Bluetooth attempts; CAN attempts remain in paired raw-frame and JSONL scan evidence. A finite decoded numeric value enters the common timestamped sample pipeline and graph immediately; its source and validation state remain research-only.

## Vehicle Test Workbench and queued reads — 2026-10-06

- The **Test Workbench** is a view of the existing acquisition workspace. It keeps the active adapter controls and graph, adds persistent OEM candidate selection and sequential runs, and places the result table below the graph. It does not create a second graph, connection, or acquisition worker.
- Every queued request reaches `LogNookEngine::execute_research_test`, then the connected `AcquisitionHandle` worker and the same mutable `HardwareAdapter` that owns live polling. SX, ELM327, and A30/A30M execute supported requests over their already-open serial/RFCOMM session. They do not open a competing connection. Simulator and OpenPort 2 retain the default unsupported research request; their OEM queue controls are unavailable.
- The same fixed quick-read selector is exposed for OBDLink SX and A30/A30M: Mode 01 engine RPM; Mode 02 freeze-frame RPM; Mode 03 stored DTCs; Mode 06 bitmap pages `00`, `20`, `40`, `60`, `80`, and `A0`, plus captured Scion-reference TIDs `35`, `A2`, and `A5`; Mode 07 pending DTCs; Mode 09 VIN; and Mode 0A permanent DTCs. SX uses its connected CAN 11/500 session and functional `7DF` requests with per-test waits; A30 retains its captured binary transport commands. ELM327 can run catalog-backed service 21/22 candidates and the dedicated diagnostic snapshot, but has no quick-test selector yet.
- `try_obd_read_test` dispatches the common quick-test contract through the existing adapter queue on a blocking worker, releasing the engine lock while the adapter completes the request. The historical `A30ReadTest` names and `a30ReadTest` report tags remain compatible with saved reports; returned adapter identity and exact command identify the actual transport. Graph values carry an optional originating `response_payload`, so a recorded channel retains the correct positive PDU even after an intermediate pending reply or when the ECU sends several PDUs. Older graph-value reports without that field still deserialize. A30 evidence retains `a30-read-tests.jsonl`; SX evidence uses the sibling `obd-read-tests.jsonl`, both included in support export. Existing MCP Mode 06 quick-test tools expose the same tests on SX and A30.
- A30 service 21/22 candidate reads are locally eligible only when the source route's request CAN ID is in `0x700–0x7FF`, matching the A30 record object field; SX and ELM retain other valid source-backed 11-bit routes. If a quick read returns distinct ECU response IDs, the result is typed `WrongResponder`: every raw payload is retained for review, and no ambiguous values are published to the graph or recording.
- OEM research results use a slim draggable divider above their table, styled like the CAN monitor divider. Pointer movement uses the visible panel geometry and supports cancellation; keyboard arrow keys resize the panel within the available-height limits. Collapse and export stay in the results header.
- Backend candidate commands accept an ID and run metadata, reload the candidate from the read-only catalog immediately before transmit, and use the catalog's exact service, identifier width, ECU route, source revision, license, and decoder. No numeric scan, arbitrary raw request, session/security change, write, clear, routine, or actuator action is exposed. A returned response or graph sample does not promote research or vehicle-validation state.
- The worker checks a due live poll before a waiting research request, serializes all adapter operations, and then resumes polling. It runs at most one queued research request per worker turn. An in-flight adapter operation is not preempted, so a research request may add its duration to observed poll latency; samples already submitted through the pipeline are retained. Fake-adapter tests cover poll priority, single-owner execution, progress after an overlong poll, request errors, and samples before and after research.
- Queue rows move through UI states `queued`, `running`, `completed`, `error`, and `skipped`. A typed ECU result (including negative/NRC, response-pending, temporary unavailability, no response, malformed/wrong-length, or wrong-responder outcomes) is a completed request and allows the next item while the adapter remains usable. A locally rejected candidate is shown as an error row and does not prevent later valid selections. A rejected active-session command, including transport loss, marks the current row `error`, halts the queue, and leaves unstarted rows `skipped`. Stop is cooperative: the current request completes or times out, then the remaining queued rows are skipped. Retries require a new user-started run.
- Every settled run can be exported through the native JSON save dialog. The report captures run ID, the adapter descriptor snapshot from run start, start/completion Unix milliseconds, and every selected row in original order with one-based `queue_index`, original `queue_size`, selection metadata, status, optional typed result, and optional error text. Serialized item statuses are `completed`, `error`, or `skipped`; a typed ECU negative is still `completed` with its negative outcome in `result`. Export uses create-new file semantics and refuses to overwrite. Backend JSONL evidence records returned typed adapter outcomes with run ID and queue order, separate from the read-only OEM catalog and exported summary; command/transport errors are represented by report rows and are not appended as typed probe records.
- Successful finite numeric graphable channels are published as `Sample` events through the existing engine pipeline. This includes finite decoded candidate and SX/A30 quick-read values and Mode 06 `raw count` channels, which are intentionally not decoded as vehicle metrics. These samples use the same graph, active recording, replay, and CSV contracts as live measurements; the UI does not synthesize samples. Out-of-range candidate values retain `Suspect` quality. DTC/VIN text and nonfinite or nonpositive results do not create numeric samples. Nonnumeric diagnostics stay in the result and returned-outcome evidence records.
- Graphable Workbench signal definitions and graphable IDs persist locally with workspace preferences (up to 500 each). After restart, their graph selections and graph-only controls are restored without adding them to automatic polling or logging. Only signal metadata and IDs are stored; measurement samples are not. Run another Workbench read or open a saved recording to see values again.
- Workbench verification is software/fixture verification only. It does not establish live SX/ELM/A30 compatibility, Scion vehicle support, physical A30 validation, or native save-dialog behavior. Physical acceptance still requires a real connected adapter and a separately recorded read-only exchange; a positive response is evidence, not vehicle validation.

## Bluetooth Classic SPP — 2026-10-05

- Windows lists already-paired Classic Bluetooth devices and connects through the standard SPP service-class UUID using Winsock RFCOMM; Windows service discovery resolves the RFCOMM channel. Pairing remains an OS action. BLE ELM clones and virtual-COM fallback are not implemented.
- ELM327 uses a prompt-delimited ASCII session, positively checks `ATI`, configures only standard adapter display/protocol settings, discovers Mode 01 support bitmaps, and emits timestamped selected PID samples through `HardwareAdapter -> LogNookEngine -> Sample`. Its direct CAN-header access is reserved for source-backed Mode 21/22 candidate tries; no raw command entry exists.
- A30 uses captured XOR-framed binary messages, SETUP/STATUS/poll/config startup, and serialized RFCOMM transactions. Its Mode 01 adapter follows supported PID bitmaps. The one-shot UI exposes captured read requests for Modes 01, 02, 03, 06, 07, 09, and 0A, plus source-backed candidate Modes 21/22. Numeric Mode 02/06 and candidate results use the shared sample pipeline; unmapped Mode 06 values are labeled `raw count`.
- Mode 04 clear, service 30 actuators, FFDA and other writes, routines, session/security changes, and arbitrary hex commands remain unavailable. The A30 Mode 21 field map and physical Bluetooth interoperability still need vehicle tests.
- Derived-signal IDs are checked for syntax, uniqueness, and collisions with base PIDs before the keyed signal list changes. Formula help uses the parser's ASCII arithmetic operators (`+ - * /`).
- The derived-signal editor is a native modal opened from the `ƒx` button on each base PID in Graph settings. New formulas start with that PID, its name, and its unit; the editor no longer occupies PID-list space.
- Derived traces default to a dotted line and a color distinct from their formula inputs. Same-unit formulas share an automatic vertical range with their input PIDs to show offsets; a **Keep source line** checkbox controls whether those source traces are drawn. Combined and separate graph footers show the formula in parentheses beside its value.

## Physical validation and transport decision

The corrected core adapter was verified on 2026-10-02 using SX r4.3.1, STN1130 v5.10.1,
serial 113011166292, COM4/115200 on the user's 2011 Ford Flex EcoBoost. Identity, CAN
11/500 protocol status, supported-PID bitmap `41 00 BF BE A8 93`, and a valid RPM
sample (632 rpm, `41 0C 09 E0`) were received. The adapter disconnected cleanly.

Both VCP and a temporary FTDI D2XX diagnostic program answered the same read-only
queries at 115200. A documented volatile `STSBR` transition also worked at
921600 and 2000000 on VCP and 2000000 on D2XX; each run restored 115200, verified
`STI`, and did not write NVM. At 2000000 baud, VCP and D2XX still measured about
16.6 aggregate Mode 01 responses/second across 5- and 24-PID mixes. `ATE0` at
2000000 made no material difference; `STSBR 3000000` was rejected. The raw rate
logs and per-PID results are in [the SX hardware report](../docs/hardware/2026-10-02-sx-ford.md).

The user reports at least 50 PID values per second in OBDwiz USB mode on this
same Ford. The installed OBDwiz 4.36 release notes identify “Combine SAE PID
Requests” (PID stuffing), which combines multiple generic Mode 01 PIDs into one
request. A five-PID request returned a 12-byte response in two ISO-TP CAN frames;
`R:1` truncated it after the first frame, while `R:2` returned the full payload.
Five-PID stuffing measured 80.51 values/s versus 32.63/s for individual reads.
Six PIDs with one 2-byte PID crossed into a third CAN frame and measured 63.60/s.
Grouping all 35 auto-requestable Ford candidates into seven response-size-aware
requests returned 350/350 values at 81.42/s. A physical `7E0` header gave no
rate gain over functional `7DF`. Production now checks CAN 11/500, enables CAN
headers and uses the fast path only when discovery identifies one standard
`7E8`–`7EF` responder; it derives its physical request header and sends selected
signals in groups whose positive payloads fit in two CAN frames. The parser
reassembles ISO-TP and rejects incomplete or multi-responder responses; `R:2`
is used for these tested groups. Volatile echo and header settings are restored
on disconnect or failed connection. If there is no standard responder ID, other
protocols fall back to the per-PID read path. Fixtures cover the five-PID
composite response and first-frame truncation, and tests verify grouping all 35
catalog signals into seven two-frame groups. The 81.42 values/s figure comes
from the diagnostic bench; the application has not yet been rerun on the vehicle
after this implementation. The SX firmware returned `?` to `STBC 0`, so STN
host-command batching is unavailable on this adapter; PID stuffing is a
different mechanism.

The STN periodic message command `STPPMA` was also tested with generic Mode 01
reads. At a 100 ms period, no unsolicited UART data appeared during idle capture;
the next manual RPM query returned two RPM lines in 208 ms, consistent with
interleaving but insufficient to establish the scheduler's response rate. A
20 ms schedule caused a manual read timeout. `STPPMC` cleanup, `ATE1` restoration,
and a normal 115200 RPM query succeeded afterward. Periodic polling is not part
of production acquisition until response delivery and bounded cancellation are
reliable. No official STMP transport is documented; `STPX` is a distinct ASCII
raw-message command.

Earlier passive waits failed because no unsolicited banner was observed. Separate
user-authorized UART BREAK wake tests also yielded no prompt. A temporary `ATSP 6`
at 2000000 timed out; its NVM effect remains unknown. Production acquisition does
not use those operations. The current protocol was later read successfully, but
that does not establish whether the earlier setting was accepted. Exact commands,
raw replies, provenance, and limits are in [the hardware record](../docs/hardware/2026-10-02-sx-ford.md).

Sources: [OBDLink FRPM Rev F](https://www.scantool.net/scantool/downloads/682/obdlink_frpm_f.pdf),
[OBDLink Windows setup](https://support.obdlink.com/support/solutions/articles/43000727094),
[FTDI direct/VCP interfaces](https://www.ftdichip.com/Support/Knowledgebase/d2xxintroduction.htm).

## Current POC boundaries

The typed `HardwareAdapter::execute_research_test` boundary now executes source-backed
service 21/22 candidates on the existing SX, ELM327, or A30 connection. A30 also
executes its fixed captured read tests on the same session. Public one-shot
helpers share these request/decoder paths. SX retains the connected device
identity and requires its verified CAN 11/500 setup; ELM restores functional
`7DF` addressing after a candidate so normal Mode 01 acquisition can resume.
Neither research method reconnects or repeats acquisition discovery. Simulator
and OpenPort retain the unsupported trait default. Catalog provenance/route/echo
checks and A30 checksummed framing remain in the shared adapter read paths.
Acquisition-worker scheduling and the workbench UI are separate follow-up slices.

- The earlier OpenPort inventory and passive probe are recorded in [the OpenPort 2 report](../docs/hardware/2026-10-02-openport2.md). The signed x86 OPTA DLL returned firmware `1.17.4877`, DLL `0.1.0`, API `04.04`, and accepted passive CAN5 at 500 kbit/s with requested sniff flags `0x10000800`; its first raw J2534 read was rejected after 109 ms because the returned message combined frame data. That remains historical transport evidence, not decoded PID support. The x86 worker and read-only standard Mode 01 adapter are now implemented and tested with fixtures, but have not yet been tested on a vehicle. No vehicle diagnostic request was sent during the historical passive probe; no driver replacement or firmware update occurred.

- Physical validation covers one SX/Ford connection, 41 advertised generic PIDs, successful reads for all 35 auto-requestable seed candidates, live samples for all five original UI signals, and a roughly 66-second recording of those five. The diagnostic bench measured 81.42 aggregate PID values/second across width-aware grouped queries. A live LogNook application run is still needed to verify application-level rate and stability.
- Multiple ECU responders are rejected until explicit selection is implemented. The grouped CAN fast path first enables headers and identifies one responder; the app still does not support multiple ECU selection. Ford enhanced/MS-CAN support is not claimed.
- The simulator supplies plausible values, timestamps, and response delays; scripted disconnects, malformed replies, and variable ECU sets are not implemented.
- CSV import, a shared session player, and a recent-session browser for app-local `.lognook` files are available. Alternate CSV schemas are out of scope. Live graphs keep a 180-point display window; session graphs request a bounded window from SQLite, while persisted data remains full resolution.
- The updated SX controls were not exercised in a native Tauri window during the hardware follow-up. No installer or release package has been validated.

## Selected workspace, capture, and analysis features — 2026-10-03

- `SettingsPanel.svelte` owns persisted keyboard bindings and appearance controls and dispatches typed actions to `App.svelte`. The `midnight`, `slate-lift`, `steel`, and `mist` palettes, high-contrast state, and 80–120% UI scale persist in workspace preferences. Tauri applies scale using the native webview zoom API. Focus guards leave text fields and native controls usable. The graph exposes its PNG action to this shortcut system through a window event.
- The app header keeps Settings, saved sessions, and CSV file opening together. The acquisition controls remain on one row without a scroll container at the 760 px minimum width.
- The graph's waiting message is an HTML overlay rather than SVG text, so chart stretching does not distort its glyphs. The PID picker gives the name column more room and shows the full name in a hover tooltip when truncation remains necessary.
- During live acquisition, the time window follows the retained sample span up to 18 seconds, with a one-second minimum. This keeps available traces spread across the graph before the window reaches its maximum.
- Adapter polling uses each adapter's target interval (100 ms by default); the simulator targets 17 ms, while real SX polling remains constrained by measured ECU and transport response time.
- Acquisition controls group graph/data actions separately from connection/recording actions; compact labels and responsive spacing keep the toolbar on one row down to the app's 760 px minimum width.
- The Tauri session browser lists app-local recordings by metadata. The native picker opens CSV and `.lognook` session files from any folder; session files are canonicalized and schema-checked before playback. Share and Save As continue to use browser-listed recordings.
- `pipeline_worker` serializes acquisition samples, recording, automatic triggers, and derived outputs. `pipeline_worker/trigger.rs` detects threshold conditions; `live_samples.rs` bounds the pre-trigger buffer and starts the recorder; `derived.rs` maintains formula inputs and emits separate output samples. Both use the same recording/event pipeline.
- Derived formulas compile to a small arithmetic expression tree over catalog signal IDs. The engine requests formula inputs from the adapter, emits the derived sample with the source timestamp, and never changes source rows. Tare updates only the derived offset and records a zero-valued sample.
- `GraphPreferences` are renderer-local preferences: per-signal manual min/max and line styling, one of line/scatter/bar/heatmap views, cursor readouts, and a stored reference snapshot. Reference samples are capped at 180 per signal and mapped to the current visible time span.
- Graph screenshots serialize the displayed SVG with its legend and guides, then use the browser canvas to download PNG. Full-window/native desktop capture is not implemented.
- Automatic recording currently supports one selected-signal threshold, above/below direction, hysteresis, hold duration, a 0–30 s pre-trigger window, and an automatic marker. The ring buffer is capped at 100,000 samples. Compound conditions, time/change events, automatic stop rules, notifications/audio, and physical trigger validation remain future work.


## OpenPort worker diagnostics progress — 2026-10-06

The J2534 JSON-line protocol allows zero or more `Progress` responses before one
terminal response. Each progress response carries only a stage, optional safe
detail, and elapsed milliseconds; the worker emits fixed stage names without
CAN/J2534 bytes or measurement values. `WorkerClient` forwards these events to the
core `DiagnosticSink`, retaining one overall request deadline and stopping the
worker on timeout, malformed output, or closed output. Progress never extends a
request timeout. The no-op `request` wrapper continues to return only the terminal
response, and existing fake sessions can retain their original request method.

Provider discovery and Identify have public `_with_diagnostics` wrappers.
`OpenPort2Adapter` supplies provider identity and operation context for Connect,
Mode 01 discovery, and selected PID reads on the same serialized worker owner.
Channel setup emits version-read, channel-connect, and filter-setup progress at
the corresponding existing calls; polling, ECU protocol, and teardown order are
unchanged. Diagnostic error redaction removes unexpected worker response debug
payloads while preserving the original returned error for callers.

## Diagnostics view and support export — 2026-10-06

`App.svelte` owns the single `diagnostics-event` subscription, its disposal, and native recording-picker/support-export invocation. It retains at most 500 live metadata records. `DiagnosticsPanel.svelte` is mounted once behind the Settings entry; its native modal preserves filters, optional recording selection, and export feedback while closed. On opening or Refresh, it merges `diagnostic_snapshot` with live records by sequence ID, preserving events that arrived during the snapshot request and bounding the merged view to 500.

The typed view formats operation/stage/status/duration, sanitized backend detail, omitted-event/storage failures, and per-signal success/rate/latency summaries. Its explicit field projection excludes packet and measurement fields. Adapter, operation, and errors-only filters affect only display. Opening, refreshing, and exporting diagnostics use no acquisition or research commands. Recording selection defaults to `null`, changes only after an explicit successful picker result, and can be removed. Picker/save cancellation preserves panel state. Native ZIP export receives Tauri's camelCase `recordingPath` argument for the backend's `recording_path` parameter.

## Discovery, vehicle diagnostics and dual adapters — 2026-10-09

- `CanHelp.svelte` supplies the small CAN-heading Help button and an accessible native dialog for the existing connection/capture/event/decoder/scan/replay workflow. `canHelp.ts` owns the shared boolean store for control explanations and persists it under `lognook-can-help-bubbles-v1` (default on; unavailable storage keeps the in-memory setting). CAN Workbench, traffic monitor, and field editor gate their existing native `title` explanations with this store; SVG scope help uses a conditional `<title>` child. The checkbox changes only display hints and opening Help issues no adapter request. The dialog supports Close/Escape and returns focus to its trigger, adding no permanent toolbar row.

The live CAN event analysis remains in Rust. `CanTrafficPanel` now maps an
operator-selected candidate byte/bit into the shared `CanSignalEditor` using
`canDiscovery.ts`; this selects an extraction range without assigning semantics.
The editor keeps the original evidence frame, can follow recent matching RX
frames, and displays a bounded 120-point scope. The Rust `preview_can_signal`
command verifies decoded values before the existing save/graph path configures
fields on the CAN owner. Graph selections and definitions retain their existing
workspace persistence.

Vehicle diagnostic requests are typed core research requests served by the
existing serialized OBD owner. The diagnostic model retains one result per
request and responding ECU, including transport and decoder outcomes, exact
request, raw response, timestamps, NRC and decoded fields. DTC, identity and
Mode 06 parsing is separate from the adapter envelopes. Unmapped unit/scaling
identifiers are displayed as raw values. The async desktop helper releases the
engine mutex before waiting on a queued research response, so independent CAN
controls remain available. The dedicated vehicle view complements the existing
application communication diagnostics view. Saved vehicle-read evidence is
included in support exports and returned through the native MCP tool.

The engine owns one OBD acquisition worker and one CAN workbench worker.
Connection checks distinguish OBD, CAN and any connection; a second adapter
does not replace the first adapter's capabilities or display history. Both
workers share the session origin while samples retain their source and ECU
identity. Individual disconnects affect their own owner; full disconnect and
shutdown finalize both. Recording/export/replay continue on the existing sample
pipeline and recording schema. Adding a second adapter requires recording to be
stopped. `get_status` reports both sources; `disconnect_source` operates on one.

User and agent operation instructions: [Discovery and diagnostics](../docs/DISCOVERY_DIAGNOSTICS.md).
