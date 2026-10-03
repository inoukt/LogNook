# LogNook project status

Updated 2026-10-02 · **Milestone 1 simulator complete; the OBDLink SX has completed live read-only acquisition and Ford transport/PID benchmarks on the MSI test computer. The production SX adapter now uses the measured grouped Mode 01 path on verified CAN 11/500. The CSV session player now imports mixed/legacy CSV, replays `.lognook` sessions with controls, and supports persistent markers and zoomable graph windows.** The five-signal UI CSV contains 1,095 valid live rows—219 per signal—over about 65.6 seconds. The Ford advertised 41 PIDs; all 35 public generic candidates marked auto-requestable in the PIDs seed returned valid responses, individually and in seven combined SAE PID request groups. Bare requests measured about 16.6 aggregate responses/s; single-PID STPX measured about 33.3 responses/s. Five-PID stuffing measured 80.5 PID values/s, and the seven groups returned 350/350 values at 81.42 values/s. OBDwiz 4.36 release notes document its “Combine SAE PID Requests” setting, consistent with the user's reported >=50 PID-values/s observation. Six advertised PIDs remain marked review-required because they need structured or multi-channel decoders.

This Git checkout preserves the existing source and external-agent reports. The hardware records are [OBDLink SX / Ford physical test](docs/hardware/2026-10-02-sx-ford.md), [the direct current-check log](docs/hardware/2026-10-02-sx-current-check.log), [the key-on retry log](docs/hardware/2026-10-02-sx-key-on-retry.log), and [the remaining-PID log](docs/hardware/2026-10-02-sx-remaining-pids.log).

## Delivered

- Rust workspace and shared `HardwareAdapter`, capability, signal, sample, quality, and marker types.
- Read-only simulated Mode 01 acquisition with timestamped values for all 35 seeded, auto-requestable generic signals.
- `ObdLinkSxAdapter` over serial: rechecks USB VID/PID `0403:6015` before opening the selected COM port at 115200 8-N-1. It starts with the adapter-only `STI` query, validates its reply through `>`, then queries `STDI` and `STSN`. A queued startup banner/prompt is optional. Each subsequent command waits for a reply prompt.
- Live SX connection queries supported generic Mode 01 PID ranges, follows each bitmap continuation flag, filters the shared 35-signal catalog by ECU support, and reads only selected supported signals. On ISO 15765-4 CAN 11/500, it enables headers and uses grouped `STPX` only when discovery identifies one standard `7E8`–`7EF` responder; it derives the matching physical request header, reassembles complete ISO-TP replies, and restores volatile echo/header settings on disconnect or failure. When a single standard responder ID is unavailable, it retains the per-PID path. The adapter sends no reset, wake pulse, saved-protocol/NVM command, DTC-clear request, OEM/UDS request, or actuator/write request.
- Separate Tauri/UI Identify SX action sends identity queries only; live acquisition is a separate action. The simulator remains available.
- Synthetic fixtures plus actual Ford/SX identity, supported-PID, and grouped-response captures. Tests cover no startup banner, optional startup frames, silent replies, wrong identity, exact USB matching, malformed/truncated/multiple-ECU replies, all 35 seed decoders, grouping all signals into seven responses of at most two CAN frames, supported-PID selection, and volatile setting restoration.
- Bounded acquisition-to-recording/UI pipeline, SQLite `.lognook` recording, marker insertion/edit/delete, controlled replay, bounded graph-window queries, and CSV sample/marker event import/export.
- Tauri 2 native CSV picker and command/event bridge; Svelte/TypeScript UI with a searchable PID picker, combined and separate draggable/resizable graphs, zoom/pan, playback speed/seek, marker controls, recording, and CSV export.
- Failure handling for adapter reads, invalid measurements, and recording writes. CSV exports refuse to overwrite existing files.
- External-agent tasks and completed reports remain in `Work to do by Ext Agent/` and `results/`.

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

## Remaining POC limits

The Ford bench advertised 41 generic PIDs. The shared scalar signal catalog and production picker support the 35 seed entries marked `public_reference` and `auto_request=True`; the six review-required entries (`01`, `15`, `19`, `34`, `38`, `41`) need structured or sub-channel decoders. The grouped production path is covered by fixtures and unit tests; application throughput has not yet been remeasured on the vehicle. Multiple ECU responders remain rejected, and Ford enhanced/MS-CAN support is not claimed. There is no scripted timeout/disconnect simulator, session browser, installer, or Linux packaging. The COM4 transport remains the production implementation; direct D2XX was used only for diagnostics.

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
- Added the native CSV picker, session-aware searchable signal list, combined/separate graph marker lines, zoom/pan/reset, player timeline, marker controls, and themed scrollbars.
- Verification on `feature/csv-session-player`: Rust workspace **65 passed, 0 failed**; `cargo check --workspace` passed; formatting check passed; custom-protocol desktop build passed; UI tests **9 passed**; Svelte check **0 errors / 0 warnings**; Vite build passed.
- A launch smoke check opened a native window titled `LogNook`. The imported CSV player was not manually operated in the native window, and this player change was not tested against a physical SX. Physical SX acquisition/throughput evidence remains in the hardware reports above.
- Build and launch from the repository root:

```bat
cargo test --workspace
cargo build -p lognook-desktop --features tauri/custom-protocol
cargo run -p lognook-desktop --features tauri/custom-protocol
```

For the frontend checks, run `npm run test:ui` and `npm run build` from `apps/desktop`.
