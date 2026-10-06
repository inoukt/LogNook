# Bluetooth ELM327 and Xtool A30 Support

## Goal

Add Windows Bluetooth Classic SPP support for generic ELM327 adapters and Xtool A30/A30M. Test all captured read-only diagnostic request families in the first deliverable, and graph every numeric result that a source-backed decoder can decode, even while vehicle validation remains `not_tested`.

## Architecture

- Add one paired-device RFCOMM transport that connects through the standard SPP service UUID `00001101-0000-1000-8000-00805F9B34FB`; Windows resolves the remote RFCOMM channel through service discovery. Keep adapter protocols separate: ELM327 ASCII command/prompt protocol and A30 framed binary protocol.
- Feed continuous numeric measurements through `HardwareAdapter`, `LogNookEngine`, and the existing sample/recording/graph pipeline. Run bounded diagnostic tests through typed source-backed requests; emit graph samples only when a decoder returns a finite numeric channel. Keep request, responder, raw bytes, decoder provenance, and validation state attached to research results.
- Use capture-backed fixtures for the A30 wire/session layer and scripted stream fixtures for ELM327. No Python runtime or second worker process is required.

## First-release scope

- Bluetooth Classic SPP for paired ELM327 and A30/A30M devices on Windows. BLE ELM clones are a later transport because BLE GATT uses a different API and device-specific service profile.
- Test captured read-only modes/services 01, 02, 03, 06, 07, 09, 0A, 21, and 22. Mode 01 selected PIDs use continuous acquisition; the other modes use explicit, finite read tests. OEM identifiers come from the pinned catalog or exact A30 capture evidence; no numeric sweeps.
- Graph source-decoded numeric Mode 21/22 channels immediately while preserving their research and `not_tested` states. Graph Mode 06 raw numeric results only as explicitly labeled raw counts; never assign unknown engineering units. DTC lists and VINs are test results, not numeric graph signals.
- Keep Mode 04 clears, A30 service 30 actuator controls, FFDA/other writes, session/security changes, and arbitrary raw-command entry unavailable.

## Acceptance criteria

1. Pairing remains an OS/user action. LogNook lists paired Classic Bluetooth devices and connects to SPP by service UUID through Windows Winsock RFCOMM, without requiring a Bluetooth virtual COM port or hard-coded channel.
2. The ELM327 handshake confirms an ELM-compatible response; selected supported Mode 01 signals flow through the shared engine and create ordinary timestamped `Sample`s.
3. The A30 link checks frame XOR, completes the captured open sequence, and handles fragmented or coalesced RFCOMM reads and record/block replies.
4. The test view offers finite buttons for each in-scope captured mode, labels the exact request and ECU route, and preserves positive, negative, no-response, malformed, wrong-responder, and timeout outcomes distinctly.
5. A valid source-backed numeric decoder output is graphable immediately without setting vehicle validation. Unknown Mode 21 fields remain raw; Mode 06 readings can be graphed as `raw count` channels.
6. Tests prove captured/scoped requests are emitted and prohibited write, clear, routine, actuator, and arbitrary raw requests are rejected before transmission.

## Evidence and limits

- The checked-in A30 archive documents Classic SPP, channel 6, A30 XOR frames, mode 01/06/21 traffic, and source captures for modes 02/03/07/09/0A/22. The raw `.cfa` remains outside the repository at `X:\p1\btsnoop_hci_20261004130-.cfa`.
- The Scion 2014 A30 Mode 21 `21 01` reply is 28 bytes and has five catalog-backed candidate channels, but their field map is not yet verified. Its raw evidence and any source-decoded values must remain marked research/unvalidated.
- No physical adapter is assumed available during automated verification. The first test build will use fixtures; a later vehicle session is needed to establish hardware compatibility and measured rates.
