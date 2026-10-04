# OEM catalog and Scion xB one-shot probe plan

## Scope

Expose the authoritative v0.3 SQLite catalog through a read-only, paged desktop API and a searchable OEM Research view. Add one fixed, source-backed Scion xB 2014 Mode 21 PID 01 read through the OBDLink SX, plus a per-row one-shot action for eligible catalog-backed service 21/22 candidates. Do not add OEM candidates to normal signal selection, logging, graphing, presets, or polling. Per-candidate reads are SX-only; OpenPort 2 is excluded while its connection is unavailable.

## Design

- Resolve `PIDs/LogNook_PID_Master_v0_3/data/lognook_pids.sqlite` in development and configure it as a bundle resource at `pids/lognook_pids.sqlite`. Tauri bundling is currently disabled, so this change does not produce an installer.
- Open the catalog read-only per backend request. Search is parameterized and paginated; the UI receives candidate definitions, exact ECU addressing, decoder metadata, applicability, source revision and license.
- The test action obtains the Scion `21 01` group from that database, validates its exact source/addressing and the five channels used for comparison, verifies OBDLink SX identity and CAN 11/500, then sends only `STPX H:7E0, D:2101, T:250`.
- The per-row action accepts a candidate ID, re-reads the source-backed row from the read-only catalog, verifies source SHA/license, service-specific PID/DID width, and exact standard 11-bit ECU headers, then sends one `STPX` request for that candidate through OBDLink SX. It captures raw response, responder, latency, and any decoded value in a separate app-data JSONL evidence log. OpenPort 2 never receives this request.
- Reassemble ISO-TP and report positive, negative, incomplete, malformed, or no-response outcomes with adapter identity, raw response, latency, decoded reference channels and source SHA. Keep the research database immutable; save probe evidence in app data.
- Reject the probe while acquisition or recording is active. No raw command field, scan, session change, or automatic polling is added.

## Implementation steps

1. [x] Add Rust catalog models and read-only search with tests for pagination, filters and inert candidates.
2. [x] Add catalog resource resolution and Tauri commands for catalog search and the fixed probe.
3. [x] Add a bounded SX probe implementation and protocol/decoder tests for valid, negative, truncated and wrong-route responses.
4. [x] Add a compact OEM Research view with search, filters, candidate/source details, and the guarded one-shot action.
5. [x] Persist probe evidence outside the bundled catalog; update architecture and project status.
6. [x] Run focused Rust and frontend checks; no vehicle hardware test was performed.
7. [x] Add per-candidate service 21/22 Try once controls, typed response handling, and SX-only gating; verify standard CAN ID bounds and prevent unsafe decoder fallback.

## Review focus

- No candidate with `auto_request=0` becomes part of `SignalDefinition` or the acquisition queue.
- The fixed Scion action sends only source-backed service `21`, identifier `01`, request header `7E0`, response header `7E8`; per-row Try once sends at most one source-backed service `21` or `22` request with the exact catalog ID and route.
- The database stays read-only and source provenance remains visible.
- Failure evidence remains distinguishable from confirmed decoder/vehicle validation.
