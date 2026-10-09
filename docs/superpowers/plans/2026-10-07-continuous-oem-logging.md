# Continuous OEM logging

## Goal

Poll source-backed Mode 21/22 measurements only when the read-only catalog says the definition is auto-requestable and a `PHYSICALLY_VERIFIED` result exists for the explicitly selected vehicle profile. Run requests through the active adapter owner and publish decoded samples through the existing recording, graph, export, and replay pipeline.

## Scope

- The session scope is an existing catalog vehicle profile selected by the operator. The profile ID is carried into the plan and is used to recheck validation evidence when the plan is created.
- Only catalog entries with `auto_request=1`, `vehicle_validation` of `vehicle_verified` or `ecu_variant_verified`, a matching physically verified validation result for that profile, and an exact 11-bit request/response route can enter a poll plan.
- The worker serializes OEM reads with standard PID reads and one-shot research requests. OEM requests target at most 20 starts per second (50 ms start-to-start), with no fixed wait after a completed response; slow transports naturally reduce the rate. Failed, undecodable, or out-of-range replies increase the interval up to one second, and valid replies ease it back to the target. Standard PID reads get another turn after each completed poll cycle so a slow live poll cannot starve OEM logging.
- Each adapter validates the selected request shape before enabling the plan (including the A30 0x7xx route requirement).
- Successful finite numeric decodes become timestamped live samples; the result keeps its candidate/request identity, ECU responder, and raw response payload.
- The UI exposes candidate selection and live sample/error/latency feedback under OEM Research. If no candidates meet the catalog gate, it explains why.

## Steps

1. Add read-only catalog APIs for vehicle profiles and candidates with exact physical-validation evidence. Test rejection of unverified, disabled, wrong-profile, and wrong-route entries.
2. Add a worker-owned polling plan and status. Schedule one OEM request at a time, keep standard polling responsive, convert successful outcomes to common-pipeline samples, and continue after request-level failures. Test serialization, sample delivery, cancellation, and failure isolation.
3. Add a compact OEM Research control for selecting a profile and eligible candidates, starting/stopping logging, and viewing request rate, latency, and errors. Do not change one-shot candidate research behavior.
4. Update architecture/status notes and run the focused Rust, UI, and build checks.

## Acceptance limits

- The shipped OEM catalog currently has 37,093 candidates and zero auto-requestable or verified definitions. Therefore, the real UI must show an empty eligible set until evidence is added through the catalog's validation workflow. No current OEM request should be sent automatically.
- A captured A30 session demonstrated 40–70 ms request/response latency and about 20 service-22 requests per second with eight selected DIDs. This is a measured target, not an application guarantee; LogNook reports the actual rate, and other adapters/ECUs may be slower. The A30 transport now returns early only after the echoed sequence and expected ECU reply are complete; silence and mismatches still use the configured timeout.
- The adapter owner is the only code allowed to perform polling I/O; Workbench and normal acquisition do not start competing workers.
