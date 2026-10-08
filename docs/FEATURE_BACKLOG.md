# Deferred feature backlog

Updated 2026-10-07. Saved for later; these items are not implementation commitments.

Most recent implementation: [continuous OEM logging](superpowers/plans/2026-10-07-continuous-oem-logging.md). The earlier [live CAN investigation plan](superpowers/plans/2026-10-07-live-can-investigation.md) is also complete.

## Requested order

1. **7 — Dedicated diagnostic views.** Present DTCs, VIN/ECU identity, and decoded Mode 06 results in readable views with timestamps and raw evidence access. Show supported actions per adapter. Keep unmapped monitor/test values explicit; add decoders incrementally from real captured replies.
2. **8 — Session organisation.** Add names, tags, notes, filtering/search, and deletion to the existing session browser. Preserve Open/Share/Save As. Confirm deletion, protect an active recording, and retain metadata when sessions are exported or copied.
3. **5 — Vehicle/ECU profiles.** Save adapter settings, exact ECU routes/identity, working definitions, and signal selections under a stable vehicle/ECU fingerprint. Revalidate cached knowledge when identity or catalog revision changes; retain the previous profile when refresh fails. A USB port or provider alone is not a vehicle identity.
4. **4 — DBC import.** Import licensed/user-supplied DBC definitions into existing CAN fields with ID width, bit numbering, byte order, signedness, scaling, units, and source attribution. Preview mappings before applying them. Report unsupported constructs explicitly; handle multiplexing deliberately rather than silently flatten it. Verify imported fields against captured frames and the Rust decoder.

## Recently implemented

- **6 — Continuous OEM logging.** Runs only explicitly selected numeric candidates that are auto-requestable and physically verified for the selected catalog vehicle. Polls on the serialized adapter owner, groups same-route signals into one request, reports actual request/sample/error/latency data, and publishes raw response payloads and numeric samples through the common recording pipeline. Implementation plan: [continuous OEM logging](superpowers/plans/2026-10-07-continuous-oem-logging.md). The current bundled catalog has no eligible entries, so real vehicle polling remains unavailable until exact-profile validation evidence is added.

Each item gets a focused implementation plan when selected. Choose dependencies at that point rather than scaffolding them now.
