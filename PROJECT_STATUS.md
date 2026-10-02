# LogNook project status

Updated 2026-10-02 · **Milestone 1 simulator complete; the OBDLink SX made a read-only connection on the MSI test computer.** The captured physical run identified the SX, read the first supported-PID bitmap, returned one valid RPM sample, and disconnected. The merged adapter now follows the bitmap continuation flag with `0120`; that added request is fixture-tested but still needs a physical bench recheck.

This uploaded workspace has no Git metadata. Existing source and external-agent reports are preserved. The new hardware record is [OBDLink SX / Ford physical test](docs/hardware/2026-10-02-sx-ford.md).

## Delivered

- Rust workspace and shared `HardwareAdapter`, capability, signal, sample, quality, and marker types.
- Read-only simulated Mode 01 acquisition with timestamped RPM, coolant, intake temperature, throttle, and speed.
- `ObdLinkSxAdapter` over serial: rechecks USB VID/PID `0403:6015` before opening the selected COM port at 115200 8-N-1. It starts with the adapter-only `STI` query, validates its reply through `>`, then queries `STDI` and `STSN`. A queued startup banner/prompt is optional. Each subsequent command waits for a reply prompt.
- Live SX connection queries supported generic Mode 01 PID ranges, follows each bitmap continuation flag, filters the shared signal catalog, and reads only selected supported signals. The production adapter sends no reset, wake pulse, protocol-setting/NVM command, DTC-clear request, OEM/UDS request, or actuator/write request.
- Separate Tauri/UI Identify SX action sends identity queries only; live acquisition is a separate action. The simulator remains available.
- Synthetic fixtures plus actual Ford/SX identity and supported-PID captures. Tests cover no startup banner, optional startup frames, silent replies, wrong identity, exact USB matching, malformed/multiple ECU replies, supported-PID selection, and selected reads.
- Bounded acquisition-to-recording/UI pipeline, SQLite `.lognook` recording, marker insertion, incremental replay, CSV export/import helpers, raw-byte CSV field, and marker sidecar.
- Tauri 2 command/event bridge and Svelte/TypeScript UI with a compact toolbar, searchable PID picker, combined graph, separate draggable/resizable signal graphs, record, marker, export, and replay controls.
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

- User-supplied OBDwiz/FORScan screens showed successful connections. FORScan reported COM4/FTDI at 2000000 bps; the user reports faster PID refresh in OBDwiz USB mode. Those are observations from those applications, not measured LogNook performance.
- Passive banner waits and approved wake pulses did not establish communication. A complete `STI\r` query at 115200 answered immediately through both D2XX and VCP. The earlier assumption that the adapter must emit a fresh startup banner on every host-port open was incorrect.
- Actual replies confirmed CAN 11-bit/500 kbaud (`STPRS`), supported-PID bitmap `41 00 BF BE A8 93`, and 12.05 V (`STVR`, D2XX probe). All five existing LogNook Mode 01 signals are advertised as supported.
- The corrected production core adapter connected through `HardwareAdapter`, then requested one supported RPM read (`010C`): **632 rpm**, raw OBD bytes `41 0C 09 E0`, quality Valid, origin Live. It disconnected cleanly. The single read took 56.785 ms; no sustained rate is claimed.
- The captured physical run predates the merged continuation-range scan. It did not include a physical `0120` response; the new second-range path currently has synthetic fixture coverage only.
- ECU address is unknown because the replies have no header. Other advertised signal measurements and sustained recording/disconnect behavior have not yet been field-tested.
- The user separately authorized wake tests and one `ATSP 6\r` attempt at 2000000 through a temporary harness. That setting command received no reply, so whether it changed NVM remains unknown. It was not repeated and is not part of production code. No bare CR or ECU write was sent. Full traces and limits are in the hardware report.

## Verification

- Regression reproduced with the real no-banner fixture before the fix, then passed after the handshake correction.
- `cargo test --workspace`: **32 passed, 0 failed** after merging the adapter and adding continuation-range coverage.
- `cargo check --workspace`, `cargo fmt --all -- --check`, and `cargo build -p lognook-desktop --features tauri/custom-protocol`: passed. Cargo Clippy is not installed in this host's toolchain; the laptop report's Clippy run predates the continuation-range change.
- UI: **3 tests passed**, Svelte check reported **0 errors / 0 warnings**, and Vite production build passed.
- Actual core-adapter connection, first-range PID discovery, one RPM read, and disconnect passed on COM4. Logs are saved under `docs/hardware/`; recheck the new `0120` request on the adapter before treating that continuation path as physically verified.
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

1. Bench-check the merged adapter's new `0120` continuation request on COM4, then exercise Identify SX and live connection in the native UI with other OBD apps closed.
2. Validate all five measurements and a short recording/replay; measure actual per-PID rates and error counts over a defined interval.
3. Test unplug/timeouts and reconnect behavior, and preserve recording gaps.
4. Compare the generic-PID path on the 2014 Scion xB when it is physically available. Do not claim Ford MS-CAN/enhanced support for the SX.

## Remaining POC limits

The Ford test verifies one physical SX and one generic RPM sample, not every vehicle, signal, or sustained performance target. Multiple ECU responders are rejected rather than automatically selected. There is no scripted timeout/disconnect simulator, marker-sidecar importer, CSV import UI, session browser, installer, or Linux packaging. The CSV import helper round-trips sample rows only. The current COM4 transport remains the production implementation; direct D2XX was used only for temporary diagnostics.
