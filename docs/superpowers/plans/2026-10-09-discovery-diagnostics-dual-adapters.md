# CAN discovery, vehicle diagnostics, and simultaneous adapters

User-selected delivery order: CAN discovery (4), dedicated diagnostics (5),
simultaneous OBD and RH02/Candlelight capture (3). Implementation uses Luna
subagents at maximum reasoning; the parent reviews and integrates their changes.

## Scope

1. Connect the existing live event shortlist to the existing signal editor.
   Click a changed byte or bit to inspect its before/after evidence, prefill an
   extraction range, follow incoming RX frames, and preview a bounded scope.
   Save through the existing Rust field decoder and graph/recording pipeline.
   Correlation does not assign a signal meaning or validation state.
2. Add a compact vehicle diagnostics view for DTC categories, VIN/ECU identity,
   and Mode 06 supported monitors/results. Requests run through the existing
   serialized OBD acquisition owner. Preserve ECU identity, timestamps, raw
   replies and typed negative/malformed outcomes; display unmapped measurements
   as raw values. Retain evidence for support export and agent inspection.
3. Allow one OBD owner and one CAN owner simultaneously, in either connection
   order. Keep a shared session timeline, sample sources and ECU identities.
   Each adapter can disconnect or fail without ending the other adapter's
   capture or shared recording. Full disconnect/shutdown still finalizes both.
   Adding an adapter during recording continues to require stopping recording.

## Acceptance checks

- CAN seed/bit selection and scope tests cover null transitions, unrelated IDs,
  TX frames, short payloads and constant values. Rust preview stays authoritative.
- Diagnostic parser tests cover DTC categories, per-ECU results, Mode 09 text,
  Mode 06 records/support bitmaps, wrong identifiers, truncation, and negative
  responses. Unsupported adapters are explicit and do not transmit.
- Simulated OBD plus injected CAN transport tests cover both connection orders,
  staggered start times, a shared recording and individual disconnect/failure.
- UI helpers/wiring, Svelte checking, production frontend build and the relevant
  Rust tests must pass. Build a desktop binary embedding the new frontend.
- Physical ECU/adapter timing, Bluetooth interoperability and RH02 reception on
  a real bus remain hardware acceptance work, not inferred from simulation.

## Exclusions

No DBC import, profile subsystem, numeric scan on connection, automatic decoding
of unknown signal meanings, diagnostic clears, or firmware/programming workflow.
Preserve existing user captures and unrelated working-tree files.
