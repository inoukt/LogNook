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

## Shared contracts
`DeviceDescriptor {stable_id, hardware_family, firmware, path}`
`ConnectionCapability {protocols, bus_types, supports_passive, max_channels, supported_rates, timestamp_capability, uncertainties}`
`SignalKey {protocol, ecu_address, identifier, vehicle_profile_variant}`
`SignalDefinition {key, name, meaning, dimensions, decode_spec, source_definition_version}`
`Sample {key, t_monotonic_ns, t_wall_utc, raw_bytes_or_value, canonical_value, unit, quality, source_id}`
`Event {timestamp, source, kind, payload}` / `Marker` / `TriggerRule` / `Session`.
All domain models are defined in a single common crate and serialized through schema-versioned APIs. Adapter implementations depend on the common interfaces; the engine never imports vendor-specific types.

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
- Samples retain monotonic elapsed time, UTC receive time, source/ECU, units, quality, origin, and optional raw OBD bytes. Missing ECU-address headers remain unknown; no ECU address is fabricated.
- A recording is a new SQLite `.lognook` file with WAL during acquisition. Clean close checkpoints and verifies integrity. Original sample timestamps, raw bytes, quality, and separate markers are retained.
- An adapter disconnect disarms an untriggered automatic rule and clears its rolling pre-trigger samples. If an adapter read fails during an active recording, the partial recording stays open for an explicit stop; another adapter cannot connect until that recording closes, avoiding mixed elapsed-time origins.
- LogNook CSV is long-form with `raw_hex` and typed `event_type=sample|marker` rows. Export refuses overwrites. The importer accepts older sample-only CSV and imports legacy `*-markers.csv` sidecars only when inline marker events are absent. CSV import streams into a new SQLite session and preserves the selected source. Playback reads ordered session events incrementally and feeds samples through the same bounded pipeline with `origin=replay`.
- FORScan wide logs with a `time(ms)` column are also imported: comma or semicolon delimiters are detected, numeric PID columns and parenthesized units become samples, duplicate signal headings receive a suffix, `Yes`/`No` flags map to 1/0, and nonnumeric placeholders are skipped. Their elapsed millisecond timestamps are converted to nanoseconds; since these exports have no absolute time, the importer anchors wall-clock timestamps at import time.
- Live CSV snapshots serialize the UI's retained live samples (at most 180 per logged signal) through the shared CSV model without opening a recording. Saved sessions offer CSV sharing (samples plus markers) and a native `.lognook` Save As copy; export commands canonicalize source sessions under the app recordings directory before reading or copying them.
- Tauri owns app-local recording paths and bridges the engine to Svelte. The PID picker and combined/separate graphs share timestamped pipeline samples with recording and replay.

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
- The v0.3 OEM SQLite catalog is resolved from a Tauri resource in packaged builds and from `PIDs/LogNook_PID_Master_v0_3/data/lognook_pids.sqlite` in development. Rust opens it read-only per paged search; candidate records retain service/DID, request/response headers, decoder, applicability, source SHA, and license. The OEM Research picker is separate from the ordinary Mode 01 `SignalDefinition` list, so unvalidated entries cannot be logged, graphed, preset, or automatically polled.
- The Scion xB research action re-reads the pinned Scion source definition and accepts only its reviewed 2014 profile, service `21`, identifier `01`, request `7E0`, response `7E8`, and five fixed byte-aligned decoder channels. It identifies the SX, requires CAN 11/500, then sends exactly `STPX H:7E0, D:2101, T:250`. It reassembles ISO-TP and returns raw frames plus candidate values; outcomes are appended to app-data `oem-validation/scion-xb-mode21.jsonl`, separate from the read-only catalog. Active acquisition or recording blocks the action. No OEM polling or generic raw-command input is provided.
- The captured 2014 xB `21 01` reply is a 28-byte ISO-TP PDU (`61 01` plus 26 data bytes); the probe also accepts the earlier 29-byte form. It preserves the raw reply and decodes only its five selected candidate channels. Their source definitions remain unvalidated, and other captured identifiers are not automatically polled.
- Each eligible OEM catalog row also has a **Try once** action for the selected OBDLink SX. The Tauri command accepts only a candidate ID, re-reads that row from SQLite read-only, checks its source SHA/license and service-specific identifier width, and restricts both catalog ECU headers to standard 11-bit CAN IDs. It identifies the SX, requires CAN 11/500, then sends exactly one catalog-derived service 21 or 22 read. The response must echo the requested service and PID/DID and match the expected ECU before decoding. Typed outcomes preserve negative NRC (including the code), response-pending, temporary unavailability, session/security requirements, no response, malformed data, and wrong responder distinctly. Raw response and provenance append to app-data `oem-validation/oem-candidate-tries.jsonl`. This manual action does not alter candidate validation or add the candidate to logging/polling. It is unavailable for OpenPort 2.
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
