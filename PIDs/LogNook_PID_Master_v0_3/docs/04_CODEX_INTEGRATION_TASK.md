# Codex task — integrate versioned LogNook PID catalog

## Goal

Integrate `LogNook_PID_Master_v0_1/data` as an **inert source-backed research catalog** without rewrites or speculative enhanced PID polling. The application can safely expose supported generic PIDs and expert-reviewed OEM packs while preserving all raw data and ECU provenance.

## Required vertical slices

1. Import `lognook_pids.sqlite` in read-only mode and expose `searchSignals(query, brand, layer, module)` via a shared Rust service. Keep UI search asynchronous and debounced. Add a source + verification badge per result and hover descriptions when available.
2. Represent `RequestDefinition` and `DecodedChannel` separately. One DID can decode many channels; avoid duplicate bus requests when multiple channels of one DID are selected. Cache request execution and fan out subchannel decoding.
3. Implement Service 01 support-bitmap walking and per-responding-ECU feature detection. Show "Catalogued" versus "Available on this ECU" versus "Verified decoder" as distinct states. Never assume all SAE entries are usable on any specific car.
4. Introduce a **strict disabled-by-default OEM review queue**. `auto_request=False` must be enforced in the engine, not merely hidden in the UI. Normal acquisition must not probe unknown DIDs, enter sessions, or perform actions that alter the vehicle. Separately authorized physical tests may issue exact source-backed reads; any session/security step or state-changing operation requires the root `AGENTS.md` per-test gate.
5. Add an importer for user-supplied OBDb v3 packs using `tools/obdb_import.py` as a specification reference, preserving source pack URL/license/commit, year/ECU filters, raw `fmt`, `eax`/`rax`, bit-endian, null values and all source metadata. Reject unsupported combinations instead of approximating them.
6. Add independent SSM protocol support and model/ECU matching before any Subaru memory access. Read-only access follows the standing source-backed test policy; a required session/security step must pass the root per-test gate. Do not feed SSM offsets through UDS DID formatting.
7. Add editable user-defined signals and verified-preset packages in a **separate** user-owned storage layer so catalog upgrades do not delete custom definitions.
8. Add tests using recorded raw frames for common SAE decoders and synthetic OEM DID responses, import rejection, multi-channel batching and failure/reconnect behavior. Keep physical-test evidence and simulation states distinct.

## Initial profiles

The JSON examples include 2011 Ford Flex 3.5 EcoBoost, 2014 Scion xB 2.4 and generic Subaru. Standard PIDs in these files are only requested if the connected responding ECU advertises them. Enhanced Ford Mustang, Toyota-brand and Subaru SSM candidate rows **must not** be auto-assigned to the user's vehicles.

## Acceptance criteria

Search is fast, source/validation state visible, sample timestamps/raw data intact, no OEM candidates auto-polled, unsupported PIDs filtered, malformed responses fail safely, no cross-ECU data mixups, and user presets survive schema migrations. Report exact verified integration tests and unresolved adapter/vehicle-specific work.
