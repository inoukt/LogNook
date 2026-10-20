# Unified Vehicle Diagnostics and Explicit DTC Clear

**Status:** Owner-approved and implemented on 2026-10-15. Candlelight diagnostics and clear currently use the single physical ECU accepted by its existing Mode 01 connection flow; multi-ECU Candlelight diagnostics are not implemented.

## Goal

Extend LogNook's existing Vehicle Diagnostics panel to Candlelight CAN OBD and expose a clearly separated, explicit standard OBD Mode 04 action for all physical OBD adapter types supported by LogNook. Preserve per-ECU results, adapter ownership, diagnostic evidence and typed MCP access.

## Current behavior

The panel runs stored (Mode 03), pending (Mode 07), and permanent (Mode 0A) DTC reads, Mode 09 identity reads, and Mode 06 results. Its capability gate currently enables full diagnostics for OBDLink SX, ELM327 and Xtool A30/A30M. OpenPort supports only Mode 09 identity. The new Candlelight OBD adapter supports generic Mode 01 acquisition but no diagnostic snapshot. There is no Mode 04 operation. The MCP `read_vehicle_diagnostics` action is intentionally read-only.

## User experience

- Extend the existing Diagnostics panel and report model. The snapshot continues to perform read-only DTC, identity and Mode 06 requests selected for the connected adapter. For Candlelight CAN, use the single ECU accepted by the existing Mode 01 connection flow, preserve its physical request/response route, and do not claim multi-ECU support.
- Add an explicit **Clear DTCs** control for each connected physical OBD adapter that reports Mode 04 capability. Keep it outside the snapshot request and normal logging/polling path.
- Before sending Mode 04, show a confirmation naming the selected adapter and that the request targets emission-related diagnostic information on responding ECUs. Explain that clearing can erase stored diagnostic information and affect readiness state. Cancel leaves the vehicle untouched.
- Send one standard Mode 04 request through the currently connected adapter's serialized I/O owner. Do not retry a clear automatically. Preserve positive, negative/NRC, no-response, malformed, transport-error and per-ECU outcomes. Candlelight uses the single ECU's physical route; OpenPort uses its typed J2534 functional route. Report a positive `44` response as an ECU acknowledgement, not as proof that a later read will contain no DTCs.
- After the operation, let the user run the ordinary read-only diagnostic snapshot again. Do not automatically chain a clear into a scan or vice versa.
- Simulator behavior stays explicitly simulated and never sends a vehicle request. It may show a simulated result, but the UI must label it as simulated.

## Adapter capability matrix

Capability is explicit and checked in the shared command/UI path; unsupported operations return a typed unsupported result with a reason.

| Adapter | Read-only diagnostics | Explicit Mode 04 clear |
|---|---|---|
| OBDLink SX | Preserve existing DTC, identity and Mode 06 snapshot | Add one-shot Mode 04 on its serialized session |
| Bluetooth ELM327 | Preserve existing snapshot | Add one-shot Mode 04 on its serialized session |
| Xtool A30/A30M | Preserve existing snapshot | Add one-shot Mode 04 through its existing A30 owner |
| OpenPort 2.0 | Extend beyond current identity-only whitelist for standard OBD DTC reads; preserve its current Mode 06 limitation unless the worker already supports the request safely | Add Mode 04 to the typed J2534 worker protocol and preserve ECU responder outcomes |
| Candlelight OBD | Add Mode 03/07/0A, Mode 09 identity and supported Mode 06 reads to the current 11-bit/500 kbit/s owner; current scope is the single ECU accepted during Mode 01 connection | Add Mode 04 to the same serialized CAN OBD owner using that ECU's physical route |
| Simulator | Return simulated diagnostic data with an explicit simulated label | Simulate the operation locally; no physical adapter or ECU I/O |

Do not advertise a capability solely from a product label when connection negotiation can establish it more reliably. The common UI may disable a specific operation and show its reason when a connected adapter lacks the required protocol support.

## Command and evidence boundaries

- Introduce a typed `ClearVehicleDtcs` command/result separate from `DiagnosticRequest::queries()` and `read_vehicle_diagnostics`. The read-only snapshot cannot accept a clear flag or write-like payload.
- Reuse the existing `HardwareAdapter`/acquisition owner, `DiagnosticRead`/`DiagnosticReport` evidence types where compatible, and diagnostic progress/evidence services. Retain the adapter ID, connection generation, request service, exact response bytes, ECU ID, outcome and time.
- Each backend validates exactly service `0x04` with an empty identifier and fixed request payload. Do not accept caller-provided arbitrary bytes, OEM services, ECU-write commands or extra retries.
- All CAN/SX/ELM/J2534 exchanges remain serialized on their existing transport owner. OpenPort uses only its allow-listed typed worker operation; the desktop must not construct a raw J2534 frame.
- MCP gets typed read/status/capability operations plus one explicit clear operation under the existing Agent Access gate. The MCP clear action must be a separate request and return its per-ECU outcomes; it must never be invoked by `read_vehicle_diagnostics`, background polling, profile recognition, presets or session replay.
- The UI confirmation and MCP authorization describe the same connected adapter and connection generation. If either changes before transmission, reject the pending action.

## Tests and acceptance criteria

- Standard fixtures cover positive Mode 04 acknowledgement, negative/NRC response, no response, malformed reply, and transport failure. Transports that support multiple responders preserve them separately. Read-only requests and automatic acquisition never emit Mode 04.
- Each adapter's fixture verifies its exact service/route/response parsing, serialized owner, raw evidence and failure cleanup. OpenPort tests verify disallowed services are rejected before worker I/O. Candlelight fixtures verify the single accepted ECU and physical route; they do not establish multi-ECU support.
- UI tests cover confirmation/cancel, unsupported capability, disconnect or generation change before confirmation, pending result, and post-clear read. The simulator is labelled and causes no physical command.
- MCP schema tests reject unknown payloads and injected service/identifier values, enforce Agent Access, bind authorization to adapter/generation and return typed per-ECU results.
- Existing diagnostic history remains append-only. Clear actions are recorded as distinct explicit operations and never overwrite earlier DTC reports.
- Run targeted Rust and UI tests, full workspace tests where feasible, Tauri checks, production UI build and the standalone Windows release build. No physical adapter clear is part of automated validation.

## Out of scope

No UDS service `0x14`, manufacturer-specific clear routines, security access, ECU session changes, actuator controls, programming, erase/write operations, automatic clearing after repair, or OEM candidate catalog changes. No adapter starts a diagnostic or clear request at launch or connection without the explicit user-triggered operation above.

## References

- SAE's J1979 standard index identifies Mode 03 as emission-related powertrain DTC retrieval and Mode 04 as clearing/resetting emission-related diagnostic information: <https://saemobilus.sae.org/standards/j1979_199709-e-e-diagnostic-test-modes>.
- EPA OBD inspection guidance discusses readiness considerations around code clearing: <https://nepis.epa.gov/Exe/ZyPURL.cgi?Dockey=P1002KRN.TXT>.
