# OpenPort 2.0 Mode 01 Compatibility Design

## Goal

Let LogNook use the connected Tactrix OpenPort 2.0 for a bounded, read-only test of standard OBD-II Mode 01 signals, then send those measurements through the existing acquisition, recording, graphing, CSV, and replay pipeline.

## Current evidence

The existing [OpenPort test report](../../hardware/2026-10-02-openport2.md) establishes that the OpenPort was visible to Windows under a WinUSB binding. The Tactrix J2534 provider returned `ERR_DEVICE_NOT_CONNECTED` during its presence check. The registered OPTA provider opened the adapter and reported API 04.04, but its passive raw-CAN read path returned combined frame data that the probe correctly rejected. No vehicle diagnostic request or standard PID read was sent. Therefore this work must not treat passive raw-CAN reception as a verified data path.

The current development environment has no installed OpenPort DLL or J2534 sample files, and only the x64 Rust target is installed. Software tests can still cover discovery, worker protocol, parsing, cleanup, and pipeline integration; hardware compatibility requires a later run on a Windows machine with the OpenPort and a matching registered provider.

## Architecture

- Keep the desktop process and the shared Rust engine architecture-neutral. Load no vendor J2534 DLL in the Tauri process.
- Build a dedicated 32-bit Windows worker for J2534 API 04.04. It owns the selected provider DLL and all PassThru calls for the lifetime of a connection.
- Discover only registered 32-bit J2534 04.04 provider entries whose names identify Tactrix, OpenPort, or OPTA. Validate that the registered DLL path exists. The user chooses the provider explicitly; never scan arbitrary DLLs or silently fall back to another provider.
- Communicate with the worker through a small typed, bounded request/response protocol over its standard input/output. Keep stdout reserved for protocol messages, send diagnostics to stderr, and apply deadlines and size limits to every exchange. A single worker owns the adapter and serializes all requests.
- Implement an OpenPort adapter against the existing `HardwareAdapter` and `ConnectionCapabilities` contracts. Launch it from the Tauri command boundary and feed it to the existing `LogNookEngine::connect_adapter` path.

## Read-only acquisition path

- Initially support standard 11-bit CAN OBD-II using J2534 ISO 15765 at 500 kbit/s. Do not use the unreliable raw-CAN passive path as an acquisition fallback.
- Send only standard Mode 01 supported-PID discovery requests and selected Mode 01 PID reads from LogNook's existing signal catalog. Do not accept caller-supplied service bytes, identifiers, ECU addresses, or raw frames.
- Preserve responding ECU identity in the worker response. If discovery finds multiple responders and the current engine cannot represent a selected ECU, stop with a clear unsupported-multiple-ECU result rather than mixing responses.
- Reuse the existing Mode 01 decoder and shared `Sample` types. The adapter's timestamps and samples enter the same acquisition pipeline used by SX and simulation.
- Keep OEM service 21/22 probes, raw CAN monitoring, K-line, 29-bit CAN, programming, sessions, security access, writes, DTC clearing, routines, and actuator control outside this feature.

## Desktop behavior

Add a compact OpenPort provider selector, identity action, and connect action alongside the existing adapter controls. Identity opens and closes the chosen provider using standard identity calls; it does not open a vehicle channel. A connection opens one ISO 15765 channel and exposes the supported standard signals through the existing PID picker. Adapter/provider errors must be visible in the existing status area, and disconnect/worker failure must close the channel and provider.

## Failure handling

- Reject absent registry entries, missing DLLs, unsupported API versions, provider open failures, channel/filter failures, malformed messages, unexpected response IDs, timeouts, worker exit, and IPC framing/size errors with specific errors.
- On every post-open failure, attempt channel disconnect and provider close. On worker termination or protocol timeout, stop acquisition and report the failure; never retry by switching providers or protocols automatically.
- Do not infer decoded support from a raw frame, a transport connection, or a provider identity result.

## Verification

- Unit-test provider filtering/path validation and the typed worker message format, including unknown commands, malformed messages, oversized payloads, timeout/worker exit, and cleanup.
- Test Mode 01 support-page parsing, supported-signal selection, ECU identity handling, PID decoding, malformed/wrong-ECU responses, and multiple responders with fixtures.
- Test the adapter through a fake worker so the normal `HardwareAdapter` acquisition contract and emitted samples are verified without hardware.
- Run Rust formatting/tests, the x86 worker build, Svelte checks, and the desktop build where the installed toolchain allows. Report x86-target or native-provider limitations precisely.
- A later physical test must first identify the selected provider, then verify standard PID discovery and live samples with ignition/vehicle conditions appropriate to the target. Record the provider/API version, ECU response identity, request latency, sample rate, and errors. A provider opening is not evidence of vehicle communication.

## Acceptance criteria

1. The desktop lists only matching registered 32-bit J2534 04.04 providers and lets the user choose one explicitly.
2. The vendor DLL runs only in the 32-bit worker, which serializes bounded calls and cleans up after success, error, timeout, and shutdown.
3. The OpenPort adapter discovers and reads selected standard Mode 01 signals over ISO 15765, subject to supported ECU/address behavior, and emits common timestamped samples.
4. No unsupported services, arbitrary transmit fields, passive-raw fallback, or state-changing calls are reachable through the app.
5. Automated tests cover parsing, worker IPC, cleanup, and shared acquisition; physical vehicle validation is reported separately.
