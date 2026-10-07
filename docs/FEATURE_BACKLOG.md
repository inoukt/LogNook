# Deferred feature backlog

Updated 2026-10-07. Saved for later; these items are not implementation commitments.

Current planning: [live CAN event correlation and visual signal editor](superpowers/plans/2026-10-07-live-can-investigation.md).

## Requested order

1. **6 — Continuous OEM logging.** Schedule explicitly enabled, verified definitions for the exact vehicle/ECU through the existing adapter owner. Preserve request identity separately from decoded channels, share a request when appropriate, expose polling rate/error feedback, and record the resulting samples through the common pipeline. Avoid competing Workbench and acquisition requests. Exact ECU eligibility depends on profiles in item 5; start with explicit session scope if profiles are still deferred.
2. **7 — Dedicated diagnostic views.** Present DTCs, VIN/ECU identity, and decoded Mode 06 results in readable views with timestamps and raw evidence access. Show supported actions per adapter. Keep unmapped monitor/test values explicit; add decoders incrementally from real captured replies.
3. **8 — Session organisation.** Add names, tags, notes, filtering/search, and deletion to the existing session browser. Preserve Open/Share/Save As. Confirm deletion, protect an active recording, and retain metadata when sessions are exported or copied.
4. **5 — Vehicle/ECU profiles.** Save adapter settings, exact ECU routes/identity, working definitions, and signal selections under a stable vehicle/ECU fingerprint. Revalidate cached knowledge when identity or catalog revision changes; retain the previous profile when refresh fails. A USB port or provider alone is not a vehicle identity.
5. **4 — DBC import.** Import licensed/user-supplied DBC definitions into existing CAN fields with ID width, bit numbering, byte order, signedness, scaling, units, and source attribution. Preview mappings before applying them. Report unsupported constructs explicitly; handle multiplexing deliberately rather than silently flatten it. Verify imported fields against captured frames and the Rust decoder.

Each item gets a focused implementation plan when selected. Choose dependencies at that point rather than scaffolding them now.
