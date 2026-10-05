# LogNook project status

Updated 2026-10-05 · Current named beta: **OBDLinkSX-Beta1 (`0.1.0-beta.1`)**. Milestone 1 simulator complete; the OBDLink SX has completed live read-only acquisition and Ford transport/PID benchmarks on the MSI test computer. The production SX adapter now uses the measured grouped Mode 01 path on verified CAN 11/500. The CSV session player imports mixed/legacy LogNook CSV and numeric FORScan wide logs, replays `.lognook` sessions with controls, and supports persistent markers and zoomable graph windows. The supplied FORScan `edge2.csv` imported as 100,844 samples across 17 numeric signals over 178.148 seconds; FORScan `time(ms)` is elapsed-only, so imported wall-clock timestamps are anchored at import time. The five-signal UI CSV contains 1,095 valid live rows—219 per signal—over about 65.6 seconds. The Ford advertised 41 PIDs; all 35 public generic candidates marked auto-requestable in the PIDs seed returned valid responses, individually and in seven combined SAE PID request groups. Bare requests measured about 16.6 aggregate responses/s; single-PID STPX measured about 33.3 responses/s. Five-PID stuffing measured 80.5 PID values/s, and the seven groups returned 350/350 values at 81.42 values/s. OBDwiz 4.36 release notes document its “Combine SAE PID Requests” setting, consistent with the user's reported >=50 PID-values/s observation. Six advertised PIDs remain marked review-required because they need structured or multi-channel decoders.

This Git checkout preserves the existing source and external-agent reports. The hardware records are [OBDLink SX / Ford physical test](docs/hardware/2026-10-02-sx-ford.md), [the direct current-check log](docs/hardware/2026-10-02-sx-current-check.log), [the key-on retry log](docs/hardware/2026-10-02-sx-key-on-retry.log), and [the remaining-PID log](docs/hardware/2026-10-02-sx-remaining-pids.log).

## Delivered

- Rust workspace and shared `HardwareAdapter`, capability, signal, sample, quality, and marker types.
- Read-only simulated Mode 01 acquisition with timestamped values for all 35 seeded, auto-requestable generic signals. The simulator targets a 17 ms sample cycle (about 60 Hz) for the usual three logged PIDs; the physical SX retains a conservative 100 ms target.
- `ObdLinkSxAdapter` over serial: rechecks USB VID/PID `0403:6015` before opening the selected COM port at 115200 8-N-1. It starts with the adapter-only `STI` query, validates its reply through `>`, then queries `STDI` and `STSN`. A queued startup banner/prompt is optional. Each subsequent command waits for a reply prompt.
- Live SX connection queries supported generic Mode 01 PID ranges, follows each bitmap continuation flag, filters the shared 35-signal catalog by ECU support, and reads only selected supported signals. On ISO 15765-4 CAN 11/500, it enables headers and uses grouped `STPX` only when discovery identifies one standard `7E8`–`7EF` responder; it derives the matching physical request header, reassembles complete ISO-TP replies, and restores volatile echo/header settings on disconnect or failure. When a single standard responder ID is unavailable, it retains the per-PID path. The adapter sends no reset, wake pulse, saved-protocol/NVM command, DTC-clear request, OEM/UDS request, or actuator/write request.
- Separate Tauri/UI Identify SX action sends identity queries only; live acquisition is a separate action. The simulator remains available.
- OpenPort 2 software path: 32-bit J2534 04.04 worker, provider listing/identity, fixed standard Mode 01 reads, supported-PID filtering, ECU identity checks, bounded worker IPC/timeouts, and common-pipeline acquisition. The compact toolbar selects Simulator, OBDLink SX, or OpenPort 2 and gates physical connects on identity.
- Synthetic fixtures plus actual Ford/SX identity, supported-PID, and grouped-response captures. Tests cover no startup banner, optional startup frames, silent replies, wrong identity, exact USB matching, malformed/truncated/multiple-ECU replies, all 35 seed decoders, grouping all signals into seven responses of at most two CAN frames, supported-PID selection, and volatile setting restoration.
- Bounded acquisition-to-recording/UI pipeline, SQLite `.lognook` recording, marker insertion/edit/delete, controlled replay, bounded graph-window queries, and CSV sample/marker event import/export.
- Semicolon- or comma-delimited FORScan wide-log import keyed by `time(ms)`, retaining numeric PID columns, extracting units, distinguishing duplicate names, mapping Yes/No to 1/0, and skipping nonnumeric placeholders. Source files provide elapsed time only, so imported wall-clock timestamps are anchored at import time.
- Saved-session Share exports sample/marker CSV, Save As copies the native session file, and live acquisition can export the retained buffer (up to 180 samples per logged PID) without starting a recording.
- Tauri 2 native CSV/session picker and command/event bridge; Svelte/TypeScript UI with a compact single-row acquisition toolbar, saved-session and file-open actions in the top row, a searchable PID picker, combined and separate draggable/resizable graphs, zoom/pan, playback speed/seek, marker controls, recording, CSV export, four saved palettes, high-contrast mode, and persisted 80–120% UI scale.
- Failure handling for adapter reads, invalid measurements, and recording writes. CSV exports refuse to overwrite existing files.
- External-agent tasks and completed reports remain in `Work to do by Ext Agent/` and `results/`.

## OpenPort 2.0 next test

The worker and core/UI integration compile and pass fixture tests, including timeout cleanup and a shared `LogNookEngine` sample. The x86 executable builds for `i686-pc-windows-msvc`. A read-only check on this development PC found only DENSO `DST-010` in the 32-bit J2534 registry; LogNook filters it out because it is not an OpenPort/Tactrix/OPTA provider. No `0403:CC4D` OpenPort USB device was present during the check, so no physical Mode 01 request has been tested through LogNook. The earlier OPTA report verifies identity and passive CAN setup only; its raw frame result was aggregated and rejected.

On the machine with the registered OPTA J2534 provider, run `npm run build:j2534-worker` and `npm run tauri dev` from `apps/desktop`, select **OpenPort 2**, then identify and connect to OPTA. Start with ignition conditions appropriate to a read-only generic OBD test, confirm the supported PID list, and capture ECU ID, raw response, latency, observed sample rate, and errors. If no provider appears or any error occurs, stop and record it; do not replace drivers or try a different protocol automatically. Physical validation remains pending.

## Hardware and environment inventory

- The hardware inventory below was captured on the MSI Windows 11 Home test computer: 64-bit, full version `10.0.26200.9168`; display version `25H2`, build `26200`, UBR `9168`. The registry ProductName still says Windows 10 Home, while CIM reports Windows 11 Home.
- SX USB VID/PID `0403:6015`; hardware IDs `USB\VID_0403&PID_6015&REV_1000` and `USB\VID_0403&PID_6015`; USB instance `USB\VID_0403&PID_6015\113011166292`.
- Port COM4; COM hardware ID `FTDIBUS\COMPORT&VID_0403&PID_6015`; child instance `FTDIBUS\VID_0403+PID_6015+113011166292A\0000`. Device status OK.
- FTDI USB Serial Converter (`oem467.inf` / FTDIBUS) and USB Serial Port (`oem480.inf` / FTSER2K), both driver version `2.12.36.20`. D2XX DLL file version `3.2.21.1` is installed in System32 and SysWOW64.
- Firmware independently confirmed by `STI`: `STN1130 v5.10.1`; hardware reply `OBDLink SX r4.3.1`; serial `113011166292`. Windows properties alone did not expose STN firmware.
- MSI had Rust/Cargo `1.99.0`, stable `x86_64-pc-windows-msvc`, VS Build Tools `17.14.37710.0`, MSVC `14.44.35207`, Windows SDK `10.0.26100.0`, and Node `v24.19.0`; npm was unavailable there. This workspace is on `X:\LogNook`, with the staged Rust/MSVC environment and Node/npm available for local verification.
- OpenPort2 (`0403:CC4D`) is a separate interface. Its initial test is recorded in [the OpenPort2 report](docs/hardware/2026-10-02-openport2.md). Windows currently binds it to the OPTA WinUSB driver, alongside installed Tactrix and OPTA x86 J2534 DLL registrations. SX discovery checks PID as well as FTDI VID.
- The initial Windows inventory was performed with the vehicle connector unplugged and sent no serial/OBD/ECU commands. Exact commands, results, and denied/unavailable queries are recorded in the [hardware report](docs/hardware/2026-10-02-sx-ford.md).

## Physical Ford validation and corrected handshake

- User-supplied OBDwiz/FORScan screens showed successful connections. The user's OBDwiz figure is interpreted as aggregate PID-value rate: its installed 4.36.0 release notes describe PID stuffing. On this Ford, one combined request returns multiple Mode 01 PID values. Five-PID groups measured 80.51 values/s; seven groups covering all 35 auto-requestable generic candidates measured 81.42 values/s (350/350 valid). A six-PID group crossing to a third CAN frame measured 63.60 values/s. Individual bare requests measured about 16.6/s; individual STPX requests about 33/s. The production adapter now implements the verified two-frame grouping strategy.
- Passive banner waits and approved wake pulses did not establish communication. A complete `STI\r` query at 115200 answered immediately through both D2XX and VCP. The earlier assumption that the adapter must emit a fresh startup banner on every host-port open was incorrect.
- Actual replies confirmed ISO 15765-4, CAN 11-bit/500 kbit/s (`STPRS`), 41 PIDs across support ranges `0100`, `0120`, and `0140`, and 12.05 V (`STVR`, D2XX probe). All 35 public generic candidates marked auto-requestable returned a matching positive response and byte width. The other six advertised PIDs (`01`, `15`, `19`, `34`, `38`, `41`) are marked review-required in the seed: monitor-status records or multi-channel O2 values that are not represented safely by the current scalar signal picker.
- The corrected production core adapter connected through `HardwareAdapter`, then requested one supported RPM read (`010C`): **632 rpm**, raw OBD bytes `41 0C 09 E0`, quality Valid, origin Live. It disconnected cleanly. The single read took 56.785 ms; no sustained rate is claimed for that earlier one-PID implementation.
- The original physical transcript predates the merged continuation-range scan. The new raw rate-bench logs capture `0120` (`41 20 A0 07 F1 19`) and `0140`; see the [bench results](docs/hardware/2026-10-02-sx-ford.md). One earlier UI state reported no response for PID 05, which did not recur in the five-signal CSV.
- ECU address is unknown because the replies have no header. All five generic Mode 01 signals have returned valid live samples; throttle and vehicle speed have only one-off direct reads, while the CSV covers RPM, coolant, and intake temperature. Sustained reliability and disconnect recovery remain unverified.
- The user separately authorized wake tests and one `ATSP 6\r` attempt at 2000000 through a temporary harness. That setting command received no reply, so whether it changed NVM remains unknown. It was not repeated and is not part of production code. No bare CR or ECU write was sent. Full traces and limits are in the hardware report.

## Verification

### Git checkout run on the MSI — 2026-10-02

- Checkout: `C:\Users\inouk\LogNook`, branch `main`, initial commit `e294698`. Tracked source was clean before verification and remains unchanged.
- `cargo fmt --all -- --check`, `cargo test --workspace` (**32 passed**), and `cargo check --workspace` passed.
- UI: **3 tests passed**; Svelte check reported **0 errors / 0 warnings**; Vite production build passed.
- `cargo build -p lognook-desktop --features tauri/custom-protocol` passed. The standalone executable at `target\debug\lognook-desktop.exe` launched with window title `LogNook`.
- This Codex shell has Node `v24.19.0` but no `npm` command. `package.json` and `package-lock.json` matched the existing dependency tree, so `apps/desktop/node_modules` temporarily junctioned to that ignored tree for the UI checks. No manifest or source changes were made. The command output is in [the MSI verification log](docs/msi-build-verification.log).
- The native window completed a five-signal live SX recording; the exported CSV contains only valid live rows. A PID 05 timeout had appeared in an earlier UI state but did not recur in this recording. The later rate bench captured the raw `0120` reply bytes and validated the 41-PID bitmap chain.

- Regression reproduced with the real no-banner fixture before the fix, then passed after the handshake correction.
- `cargo test --workspace`: **32 passed, 0 failed** after merging the adapter and adding continuation-range coverage.
- `cargo check --workspace`, `cargo fmt --all -- --check`, and `cargo build -p lognook-desktop --features tauri/custom-protocol`: passed. Cargo Clippy is not installed in this host's toolchain; the laptop report's Clippy run predates the continuation-range change.
- UI: **3 tests passed**, Svelte check reported **0 errors / 0 warnings**, and Vite production build passed.
- The original pre-merge core-adapter check connected, discovered the first PID range, read RPM, and disconnected on COM4. The post-merge direct retry and native UI recording are documented in the current-check hardware record; raw `0120` bytes still need a transcript.
- The updated native Tauri SX controls were not exercised in a window during this follow-up.
- `cargo build -p lognook-desktop --features tauri/custom-protocol`: passed. The standalone debug executable embeds the built UI at `C:\LogNookR\target\debug\lognook-desktop.exe` (15,111,680 bytes); no npm dev server is required to run it.

## Build and launch on this Windows development machine

```bat
call X:\VS\Common7\Tools\VsDevCmd.bat -arch=x64 -host_arch=x64
set RUSTUP_HOME=X:\Rustup\LogNook
set CARGO_HOME=X:\Cargo\LogNook
set PATH=%CARGO_HOME%\bin;%PATH%
set TEMP=X:\BuildTemp\LogNook
set TMP=%TEMP%
cd /d X:\LogNook
cargo test --workspace
cargo check --workspace
cargo fmt --all -- --check
cd apps\desktop
npm run test:ui
npm run build
npm run tauri dev
```

The machine has Rustup, MSVC build tools, Node, and npm. Clippy is not installed in the current Rust toolchain.

## Next validation work

OpenPort 2: the user chose to retain the OPTA WinUSB binding. The matching signed x86 DLL passed Open/ReadVersion/Close (firmware `1.17.4877`, DLL `0.1.0`, API `04.04`) and a passive CAN connect at 500 kbit/s. The trace records 104 Normal CAN frames across 24 IDs, including frames drained during disconnect. The first returned J2534 message combined frame data; the probe rejected it after 109 ms and disconnected/closed cleanly. The old host log does not retain the rejected message's complete metadata, so DataSize and RxStatus remain unknown. The observed filter command used TxFlags `0x40` despite requested 0. Reliable raw J2534 frame delivery is unverified. No diagnostic request or vehicle transmit API was used; the driver and DLL remain unchanged, and no production J2534 backend is implemented. Captures, physical fixture, and a small implementation plan are in [the OpenPort 2 report](docs/hardware/2026-10-02-openport2.md).

1. Connect the OpenPort2 to USB and the Ford, then run the prepared passive metadata capture without replacing the OPTA driver; the latest PnP inventory showed only the SX.

2. Verify the grouped STPX path in a live LogNook session when the SX is next available, including long-run timing, disconnect/restore behavior, and the selected-signal sets users record. Existing rate evidence is from the read-only hardware harness, not a post-implementation application recording. `R:1` truncates multi-frame responses; the app uses `R:2` only after unique responder discovery and response-size grouping. The six review-required PIDs need structured decoders before they can be exposed. OBDLink `STBC` host-command batching returned `?` on this SX. The screenshot's `STMPA`/`STMPR`/`STMPC` are not documented; actual `STPPMA/MD/MC` periodic messaging did not produce a reliable stream.
3. Replay the five-signal recording, run longer stability/disconnect checks, and compare generic PIDs on the 2014 Scion xB when available. Do not claim Ford MS-CAN/enhanced support for the SX.

## Future feature ideas

These are backlog ideas from [Addons for later](Addons%20for%20later.txt), not yet designed or scheduled:

- An in-app notepad, either always on top or in a separate window, with notes optionally saved alongside CSV data.
- Drawing or annotations on stopped graphs, persisted with the session/export, plus a list of saved annotated screenshots.
- Phone-based remote control, with explicit authentication and safety boundaries to be designed before implementation.
- Phone sensor channels (for example GPS and altitude) alongside vehicle PIDs; microphone use needs a specific measurement definition.
- Video synchronized with session time from a phone camera.
- A feasibility investigation for a virtual dynamometer.

## Remaining POC limits

The Ford bench advertised 41 generic PIDs. The shared scalar signal catalog and production picker support the 35 seed entries marked `public_reference` and `auto_request=True`; the six review-required entries (`01`, `15`, `19`, `34`, `38`, `41`) need structured or sub-channel decoders. The grouped production path is covered by fixtures and unit tests; application throughput has not yet been remeasured on the vehicle. Multiple ECU responders remain rejected, and Ford enhanced/MS-CAN support is not claimed. Scripted timeout/disconnect simulation, installer work, Linux packaging, and alternate CSV schemas remain open. The COM4 transport remains the production implementation; direct D2XX was used only for diagnostics.

## Grouped SX polling implementation and verification — 2026-10-02

The adapter, protocol parser, and simulator now expose and decode all 35
auto-requestable seed signals. The CAN fast path packs selected signal PIDs by
response width, limits each positive payload to 13 bytes (one or two ISO-TP
frames), requests exactly that frame count with `STPX`, and rejects incomplete
or ambiguous replies. `read_samples` still receives only the user's selected
signals. The remaining six advertised PIDs (`01`, `15`, `19`, `34`, `38`, `41`) stay out
of the picker pending structured monitor-status and multi-channel O2 decoders.

Current checkout checks: `cargo fmt --all -- --check`, `cargo test --workspace`
(40 tests passed), `cargo check --workspace`, and
`cargo build -p lognook-desktop --features tauri/custom-protocol` passed. UI
tests: 3 passed; Svelte check: 0 errors / 0 warnings; Vite production build
passed. This host has no `npm` command, so the UI test and build script commands
were run through the bundled Node executable and `apps/desktop/node_modules/.bin`.
The 81.42 PID-values/s result is from the previously completed hardware bench;
the application has not yet been run against the Ford after this code change.

## CSV session player progress — 2026-10-02

- Added mixed sample/marker CSV export and import into a new `.lognook` session. Legacy sample-only CSV and old marker sidecars remain readable; round-trip coverage preserves timestamps, raw bytes, labels, and notes.
- Added streaming, interruptible playback with pause/resume, seek, speed, progress events, persistent marker editing, and a bounded SQLite graph window.
- Added the native CSV/`.lognook` picker, session-aware searchable signal list, combined/separate graph marker lines, zoom/pan/reset, player timeline, marker controls, and themed scrollbars.
- Verification on `feature/csv-session-player`: Rust workspace **65 passed, 0 failed**; `cargo check --workspace` passed; formatting check passed; custom-protocol desktop build passed; UI tests **9 passed**; Svelte check **0 errors / 0 warnings**; Vite build passed.
- A launch smoke check opened a native window titled `LogNook`. The imported CSV player was not manually operated in the native window, and this player change was not tested against a physical SX. Physical SX acquisition/throughput evidence remains in the hardware reports above.
- Fresh-clone build and launch (requires Rust/MSVC plus Node.js/npm):

```bat
cd apps\desktop
npm ci
npm run test:ui
npm run build
cd ..\..
cargo test --workspace
cargo build -p lognook-desktop --features tauri/custom-protocol
cargo run -p lognook-desktop --features tauri/custom-protocol
```

For a development launch instead, run `npm run tauri -- dev` from `apps/desktop`. The MSI machine had Node.js but no `npm` command in the earlier environment check, so install/enable npm there before building this branch.

## Replay and PID workspace refinements — 2026-10-02

- Added persistent per-PID color pickers, current values, and averages over the visible graph range; session averages are queried from SQLite.
- Replay keeps the playhead centered on each progress update, including at recording edges. Clicking the graph, dragging the recording-position slider, and optional wheel scrubbing update the cursor and viewport immediately, with backend seeks coalesced in order. Markers can have blank labels and notes.
- Replay uses absolute playback deadlines and coalesces UI readouts per signal at 50 ms, preserving all recorded samples and marker events.
- Verification: UI tests **18 passed**; `cargo test -p lognook-core` **66 passed**; Svelte check **0 errors / 0 warnings**; Vite build passed; Tauri development build launched. `cargo fmt` was unavailable because the rustfmt toolchain component is not installed.

## Workspace layout and graph rendering — 2026-10-03

- Compressed the session player by about 25%, removed the redundant measurements heading, and added a PID panel show/hide toggle.
- Clipped graph paint to its column and limited replay graph rendering to screen-resolution extrema; acquisition and recordings retain full-resolution samples.
- Reduced the branded top bar, made the acquisition toolbar wrap at narrow widths, moved the toggle to a left-side **Hide List / Show List** rail, and added a Close session action that stops replay and restores live graph selection.
- Verification: UI tests **19 passed**; Svelte check **0 errors / 0 warnings**; Vite production build passed; `cargo test --workspace` passed **67 tests**. Hide/Show List was checked in the 1280 px browser preview; the native CSV close flow and 760 px layout were not manually exercised.

## Selected requested features 1–7 — 2026-10-03

- Workspace preferences restore logged/graphed PIDs, graph mode, PID-list visibility, status-panel height, palette, and high-contrast state. Settings reset clears workspace, graph, color, wheel-scrub, shortcut, trigger, and derived-signal preferences.
- Customizable shortcuts cover recording, replay, markers and marker navigation, PID search, graph zoom, and graph PNG capture. A single Settings panel owns shortcut editing and focus guards.
- Automatic threshold recording supports above/below conditions, hysteresis, hold time, and 0–30 second pre-trigger capture. The Rust pipeline keeps at most 30 seconds/100,000 samples, writes the original sample timestamps, and inserts an automatic marker.
- Graph PNG export includes the rendered SVG, marker/playhead guides, current legend, and title; the shortcut is `Ctrl+Shift+S` by default.
- Derived signals use a safe arithmetic parser over acquired PID identifiers. Unit conversion is expressed in the formula; tare captures the current formula output as an offset. Derived samples are stored alongside unchanged source samples and remain available in CSV and replay.
- The derived-signal editor opens as a popout from the `ƒx` icon on a PID in Graph settings, and pre-fills the formula, name, and unit from that PID.
- Derived traces default to a dotted line and a distinct color. Same-unit formulas share a range with source PIDs to show offsets, and Graph settings can show or hide source lines; the formula appears in parentheses in graph readouts/legends.
- Graph settings persist manual ranges, line styles/thickness, line/scatter/bar/heatmap views, cursor readouts, and a dashed reference snapshot (up to 180 samples per signal).
- The session browser lists app-local `.lognook` recordings by modified time and sample count. The native file picker opens CSV files and `.lognook` sessions from any folder; direct open validates the session file and schema.
- Review fixes: failed or manual disconnects disarm automatic triggers and clear the pre-trigger ring; an open partial recording blocks reconnect so reset adapter clocks cannot mix into it. Session opening now awaits player activation, Space can insert a marker while the graph slider is focused, derived IDs reject duplicates/base PID collisions, and formula help matches supported operators.
- Verification on this checkout after review: `cargo test --workspace` **86 passed**; `cargo check --workspace` passed; `cargo build -p lognook-desktop --features tauri/custom-protocol` passed; UI tests **52 passed**; `npm run build` passed with **0 Svelte errors/warnings**; `git diff --check` passed. `cargo fmt --all -- --check` could not run because rustfmt is not installed in the available stable toolchain.
- This feature batch was verified by simulation/fixtures and compilation only; no physical vehicle or adapter test was performed. PNG capture covers the graph view rather than the whole desktop window. Automatic recording currently starts from one threshold rule; compound/time/change rules and automatic stop actions remain future work. Formula functions beyond arithmetic are not implemented.

## Live graph motion — 2026-10-03

- Live graphs share an animation-frame clock and advance their viewport between PID samples. Line charts extend the last measured point to the right edge for a continuous display; cursor readouts, acquisition samples, recordings, and replay data remain based on real measurements. The live clock resets on connection clock rollover and stops when live acquisition ends.
- UI verification: **53 tests passed**; Svelte check reported **0 errors / 0 warnings**; Vite production build passed. The smooth animation has not yet been manually checked in the native simulator window.

## Session sharing and live CSV snapshots — 2026-10-03

- Each saved-session row has **Share** (saves the complete session as portable CSV with markers) and **Save As** (copies the native `.lognook` file). Source paths are restricted to canonical recordings directly inside LogNook's recordings folder.
- **Save live CSV** exports the retained samples for currently logged PIDs, sorted by timestamp, even when recording is off. The snapshot uses each PID's existing 180-sample in-memory buffer and does not start or modify a recording.
- Verification: UI tests **54 passed**; Svelte check reported **0 errors / 0 warnings**; Vite production build passed; `cargo test --workspace` **86 passed** (including the new CSV snapshot and session export tests); `cargo check --workspace` and the Tauri custom-protocol build passed. Native save dialogs and the visual button flow have not yet been manually exercised.

## OEM PID catalog and Scion xB one-shot — 2026-10-03

- LogNook now reads the authoritative v0.3 catalog at `PIDs/LogNook_PID_Master_v0_3/data/lognook_pids.sqlite` through a paged Rust API opened with SQLite read-only flags. The new **OEM Research** view searches by signal text, brand, model, service, and `21 01`/`2101`; it displays addressing, decoder, applicability, source revision, and license. Results remain separate from the normal Mode 01 signal picker and acquisition queue.
- The SX research action loads the exact Scion candidate group from the catalog, checks the reviewed OBDb Scion-xB source revision, fixed `21 01`/`7E0 → 7E8` routing, `auto_request=0`, `not_tested` state, and the five fixed decoder channels. It verifies adapter identity and CAN 11/500, then sends only `STPX H:7E0, D:2101, T:250`. It captures positive, negative, timeout, malformed, wrong-responder, and wrong-length results with source SHA, SX identity/firmware, raw reply, latency, formulas, and values. Evidence is appended to `app_data_dir()/oem-validation/scion-xb-mode21.jsonl`, outside the read-only catalog.
- The real database regression check found **37,093** total OEM candidates, **352** Scion Service 21 candidates, and **18** Scion `21 01` channels. All queried candidates remain non-auto-polled. The v0.3 source is still a research lead; a response alone will not mark it vehicle-validated.
- Verification after implementation: `cargo test --workspace` **93 passed, 0 failed**; `cargo check -p lognook-desktop --features tauri/custom-protocol` passed; `cargo build -p lognook-desktop --features tauri/custom-protocol` passed; UI tests **54 passed**; Svelte check **0 errors / 0 warnings**; Vite build passed; `git diff --check` passed. `cargo fmt --all -- --check` remains unavailable because the installed Rust toolchain lacks the `rustfmt` component.
- Not verified: the OEM panel has not been manually operated in a native Tauri window, and no request has been sent to a vehicle. `tauri.conf.json` now maps the catalog as a resource, but `bundle.active` remains false; run the app from this checkout so development path resolution can find the database. No installer containing the database was built.
- Next physical step: from the checkout, run `cd apps\desktop` then `npm run tauri -- dev`. With the **2014 Scion xB 2.4L 2AZ-FE** stationary in Park, parking brake set, ignition on, engine off, and other diagnostic apps closed, open **OEM Research**, select the SX port, filter to Scion xB 2014, and press **Try once** on one enabled source-backed row, starting with the reviewed `21 01` definition. Review the raw reply and decoded value; stop and preserve the result if the adapter times out, the ECU replies negatively, or frames are incomplete. This read is not continuous polling and does not validate other Scion candidates.

## Laptop branch integration — 2026-10-04

- Integrated `codex/grouped-toolbar-themes`, including four appearance themes, high contrast, persisted 80–120% UI scale, compact grouped controls, faster graph refresh while scrubbing, and session opening from any folder. Settings, Browse saved sessions, and CSV opening share the app header; the acquisition toolbar fits one row without horizontal scrolling.
- `feature/csv-session-player` and `fix/mode01-pid60-discovery` had no commits missing from `main`; both were already included.
- Verification: Rust workspace **96 passed**; UI tests **57 passed**; Svelte check **0 errors / 0 warnings**; Vite and Tauri custom-protocol builds passed. The native app launched as `LogNook`; 760 px and 970 px browser layouts had no toolbar overflow, and Settings exposed the 80–120% scale control.

## OEM catalog per-candidate one-shot — 2026-10-04

- Each eligible service 21/22 OEM catalog row has a **Try once** action. It works only with a selected OBDLink SX and an idle workspace; OpenPort 2 is excluded while its connection is unavailable. The Tauri command accepts a candidate ID, re-reads that row from the read-only catalog, checks source SHA/license, identifier width, and standard 11-bit ECU headers, verifies SX identity and CAN 11/500, then sends one exact catalog-derived `STPX` read.
- The result shows the adapter, exact request, latency, responding ECU, raw reply, and any decoded selected candidate value. Positive replies must echo the requested service and PID/DID. Typed outcomes preserve NRC codes, response-pending, temporary unavailability, session/security requirements, no response, malformed response, and wrong ECU distinctly. Candidate and response evidence append to app-data `oem-validation/oem-candidate-tries.jsonl`; the OEM database and validation state remain unchanged.
- Review also rejects non-11-bit routes, avoids panics on malformed externally constructed candidates, and does not silently decode an unknown value format as unsigned.
- Verification: `cargo test --workspace -j 1` **146 passed, 0 failed**; `cargo check --workspace -j 1` passed (two existing dead-code warnings in the J2534 worker); `cargo build -p lognook-desktop --features tauri/custom-protocol -j 1` passed; UI tests **63 passed**; `npm run build` passed with **0 Svelte errors/warnings**; `git diff --check` passed. Rust formatting check passed on the new probe files and OEM command module. Workspace-wide `cargo fmt --all -- --check` reports formatting diffs across existing files, so those were not mass-reformatted.
- Not verified: no physical adapter request or native-window interaction was performed. This is an SX-only POC verified with fake serial/catalog fixtures; it has not established that any Scion/Toyota OEM PID is supported by the vehicle.
- Next physical task: with the 2014 Scion xB stationary in Park, parking brake set, ignition on, engine off, and other diagnostic apps closed, run the app from this checkout, select OBDLink SX, open OEM Research, filter to the vehicle, and press **Try once** on one enabled source-backed candidate. Save the displayed result and `oem-candidate-tries.jsonl`; stop if the adapter reports an error or the ECU returns a negative/incomplete response.

## Graph and PID label legibility — 2026-10-04

- The no-samples message is rendered outside the stretched SVG, with its 11 px display size compensated for the persisted webview scale. The Log/Graph checkbox columns are closer together; hovering a truncated PID name shows the complete name.
- The live graph's time span follows the retained samples, up to 18 seconds, so faster sample rates keep filling the chart instead of leaving blank time to the left.
- Verification: Svelte check reported **0 errors / 0 warnings**; Vite production build and Tauri custom-protocol build passed. The updated native app launched as `LogNook`.

## Replay graph window completeness — 2026-10-04

- Session graph rendering now keeps all 360 buckets of first/minimum/maximum/last samples (up to 1,440 points per signal), instead of limiting replay traces to the 180-point live buffer. Auto-ranging and cursor readouts use the complete visible session window too.
- Verification: UI tests **64 passed**; Svelte check **0 errors / 0 warnings**; Vite and Tauri custom-protocol builds passed. A regression test verifies full-window endpoints, auto-range extrema, and early cursor lookup.

## USB hot-plug and OpenPort 2 diagnostics — 2026-10-04

- OBDLink SX ports are enumerated at startup, when SX is selected, on window focus, and every two seconds while SX is selected and disconnected. Refresh keeps a still-connected COM selection; unplugging the selected adapter clears its stale identity. The app no longer needs a restart when the SX is plugged in after launch.
- OpenPort 2 now has a manual provider-list refresh. Its diagnostics say which 32-bit J2534 entries were ignored or lack a DLL. Errors identify the provider-list, identify, J2534 channel setup, PID discovery, or individual PID-read stage. A no-response result includes the PID and elapsed wait; a hung worker timeout names the request and deadline. Identify, Connect, and Refresh remain manual; there is no silent provider/protocol fallback.
- Read-only diagnosis on this development PC found DENSO `DST-010` in the 32-bit J2534 registry and no currently-present `0403:CC4D` OpenPort device. The current worker filters DENSO by provider name; that explains the empty OpenPort list but is not a compatibility verdict. DENSO documents DST-010 as a J2534 device with ISO 15765 support, so it remains a future unverified candidate for explicit manual testing. No DENSO DLL was loaded and no vehicle request was sent. Details are recorded in `docs/hardware/2026-10-02-openport2.md`.
- Verification: `cargo test --workspace -j 1` **151 passed, 0 failed**; UI tests **66 passed**; `npm run build` passed with **0 Svelte errors/warnings**; `cargo check --workspace -j 1` passed with two existing J2534 worker dead-code warnings; `npm run build:j2534-worker` and `cargo build -p lognook-desktop --features tauri/custom-protocol -j 1` passed. Provider-list output was also checked through the built x86 worker and reported the ignored DENSO entry.
- The updated native app was built and launched. Not verified: physical SX hot-plug, an active OpenPort device, or ECU communication. No alternate OpenPort provider/protocol path is implemented; retry stays manual on the selected eligible provider until a separate protocol strategy is justified and tested.
- Next check: with the app open, plug in the SX and select it; confirm its COM port appears within a few seconds. For OpenPort, press **Refresh providers**; after a compatible provider is registered, press **Identify**, then **Connect**. If it fails, capture the stage-specific status and raw error before retrying.

## Captured Scion Mode 21 reply — 2026-10-04

- The Scion xB `21 01` one-shot still sends `STPX H:7E0, D:2101, T:250` to request ECU `7E8`. Its ISO-TP decoder accepts the A30 capture's 28-byte PDU (`61 01` plus 26 data bytes) as well as the prior 29-byte PDU, preserves the raw response, and decodes only the five selected candidate channels. Other discovered identifiers remain outside automatic acquisition.
- The 18 catalog channels for `21 01` remain community references, unvalidated and not auto-requested. The capture confirms an ECU reply and repeatable stream, not the byte-field map or formulas. No A30 Bluetooth transport, service 30 actuation, or unknown write was added.
- Verification: `cargo test --workspace -j 1` **152 passed, 0 failed**; UI tests **66 passed**; `npm run build` passed with **0 Svelte errors/warnings**; `cargo build -p lognook-desktop --features tauri/custom-protocol -j 1` passed. The capture regression failed against the old 29-byte-only check and passed after the change; the test also rejects an unobserved 30-byte response.
- Not verified: no physical OBDLink SX request has been sent, so the candidate formulas remain unvalidated. The A30 `.cfa` was decoded separately on handle `0x13` / DLCI 12; that validates the captured-frame fixture, not an SX exchange.

## A30 protocol and Scion capture archived — 2026-10-04

- Saved the A30 protocol export and Scion xB session analysis under `docs/hardware/a30/`; the archive README records their source hashes and implementation boundary. The raw `.cfa` remains at `X:\p1` and is not checked into the repository.
- Direct extraction of handle `0x13` / DLCI 12 found 993 host frames, 1,979 dongle frames, and zero XOR mismatches. The Scion vehicle is VIN `JTLZE4FE1EJ049314`, ECU string `AZE151 2AZFE`, 2AZ-FE, with the engine at `7E0 → 7E8`.
- Mode 06 bitmap rows in the source Markdown had two incorrect response bytes and counted continuation markers as TIDs. The capture advertises 11 actual TIDs (`01`, `02`, `21`, `35`, `3D`, `42`, `A1`–`A5`); it read only `35`, `A2`, and `A5`. Their raw values are preserved, but MID meanings and UAS scaling remain unknown. LogNook has no Mode 06 implementation.
- A30 Mode 06 response latency ranged from about 96–532 ms. This is evidence for that adapter only, not an SX/J2534 timeout setting. The A30 archive includes state-changing service 30 and an unknown multi-frame write as historical capture evidence; LogNook must not reproduce them as acquisition commands.

## OEM research panel layout — 2026-10-05

- OEM search filters and the Scion one-shot stay above a single results scroll area. The list expands into the remaining sidebar height, reserves the right-side scrollbar gutter, and constrains candidate cards to one column so search results cannot create a horizontal scrollbar or push under the panel edge.
- Verification: `npm run test:ui` passed (67/67); `npm run build` reported 0 Svelte errors/warnings; `cargo build -p lognook-desktop --features tauri/custom-protocol -j 1` passed. The rebuilt app was launched for visual review.

## Transient acquisition recovery and Mode 01 diagnostics — 2026-10-05

- Read errors now carry an explicit transient/fatal disposition. The SX retries only a `>` prompt timeout with no response payload (an optional command echo is ignored) or a completed command with no selected-PID reply. It waits 250 ms, resets the failure streak after a successful cycle, and disconnects after three consecutive transient failures. Malformed or partial replies, serial write failures, ECU negative responses, and decode/range errors stay fatal; OpenPort worker timeouts stay fatal because the isolated worker is terminated.
- Mode 01 `7F` replies now retain ECU, requested PID/service, echoed service, and NRC and display them as a negative response instead of “No response.” The parser covers ISO-TP text, legacy text (including command echo), and binary payloads. Failed reads never produce samples.
- Verification: `cargo test --workspace -j 1 --quiet` passed (156 tests); targeted `rustfmt --check` passed for the six changed Rust files; the custom-protocol desktop build passed and the rebuilt app was launched. No physical adapter timeout or ECU negative response was induced. The workspace-wide `cargo fmt --all -- --check` reports formatting differences in untouched files, so those files were left unchanged.
