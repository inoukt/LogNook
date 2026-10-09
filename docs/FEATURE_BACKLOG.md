# Deferred feature backlog

Updated 2026-10-09. Saved for later; these items are not implementation commitments.

Current feature bundle: [CAN discovery, vehicle diagnostics and simultaneous adapters](superpowers/plans/2026-10-09-discovery-diagnostics-dual-adapters.md). Earlier implementations include [continuous OEM logging](superpowers/plans/2026-10-07-continuous-oem-logging.md) and [live CAN investigation](superpowers/plans/2026-10-07-live-can-investigation.md).

## Requested order

1. **8 — Session organisation.** Add names, tags, notes, filtering/search, and deletion to the existing session browser. Preserve Open/Share/Save As. Confirm deletion, protect an active recording, and retain metadata when sessions are exported or copied.
2. **5 — Vehicle/ECU profiles.** Save adapter settings, exact ECU routes/identity, working definitions, and signal selections under a stable vehicle/ECU fingerprint. Revalidate cached knowledge when identity or catalog revision changes; retain the previous profile when refresh fails. A USB port or provider alone is not a vehicle identity.
3. **4 — DBC import.** Import licensed/user-supplied DBC definitions into existing CAN fields with ID width, bit numbering, byte order, signedness, scaling, units, and source attribution. Preview mappings before applying them. Report unsupported constructs explicitly; handle multiplexing deliberately rather than silently flatten it. Verify imported fields against captured frames and the Rust decoder.

## Recently implemented

- **Dedicated diagnostic views.** The top-bar vehicle view groups DTC categories, VIN/calibration/ECU name and Mode 06 support/results by responding ECU. Explicit SX/ELM/A30 reads yield between queries, save raw evidence, and are available through native MCP. Unmapped Mode 06 values remain raw. OpenPort, legacy/29-bit diagnostic routes and physical acceptance remain future work.
- **CAN discovery workflow.** Live event candidates link changed bytes/bits to the existing field editor, transition evidence, a bounded live/frozen scope and Save and graph. Saved-capture analysis remains available; this new interactive scope uses live recent frames.
- **Simultaneous OBD and CAN.** One OBD source and one RH02/Candlelight source share a session timeline and recording, with independent disconnect/failure handling. Adding a second source during recording still requires stopping first. Instructions: [Discovery and diagnostics](DISCOVERY_DIAGNOSTICS.md).

- **6 — Continuous OEM logging.** Runs only explicitly selected numeric candidates that are auto-requestable and physically verified for the selected catalog vehicle. Polls on the serialized adapter owner, groups same-route signals into one request, reports actual request/sample/error/latency data, and publishes raw response payloads and numeric samples through the common recording pipeline. Implementation plan: [continuous OEM logging](superpowers/plans/2026-10-07-continuous-oem-logging.md). The current bundled catalog has no eligible entries, so real vehicle polling remains unavailable until exact-profile validation evidence is added.

Each item gets a focused implementation plan when selected. Choose dependencies at that point rather than scaffolding them now.
