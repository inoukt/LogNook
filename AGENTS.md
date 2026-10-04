# LogNook project instructions

## Project purpose

LogNook is a desktop application for capturing, recording, graphing, exporting, and replaying vehicle measurements. Keep acquisition and replay on the shared application pipeline so the UI renders real recorded or acquired data.

## Architecture

- Desktop shell and commands: Tauri 2 under `apps/desktop/src-tauri`.
- Acquisition, protocol, recording, and shared models: Rust workspace, primarily `crates/lognook-core`.
- User interface: Svelte and TypeScript under `apps/desktop/src`.
- Prefer the existing `HardwareAdapter`, acquisition engine, event pipeline, and recording model. Add abstractions only when a concrete second use case needs them.
- Keep responsibilities clear, APIs typed, and handwritten modules focused. Preserve existing worktree changes.

## Catalog and data model

- The v0.3 SQLite catalog at `PIDs/LogNook_PID_Master_v0_3/data/lognook_pids.sqlite` is the authoritative OEM research catalog.
- `oem_candidates` stores researched service 21 and 22 definitions with source, addressing, decoder, applicability, and validation metadata.
- `vehicle_supported_commands` stores reported vehicle command codes and labels as supplied. Keep these reports distinct from decoded signal definitions.
- A diagnostic request may return multiple decoded signals. Preserve request identity separately from its decoded channels.
- Keep research definition, ECU response, decoder verification, and vehicle validation as distinct states.
- Preserve source attribution, revision, license, exact request/response addressing, response layout, units, and applicability whenever catalog data is changed.

## Enhanced diagnostics direction

- Treat service 21 and service 22 as separate request forms: service 21 uses a one-byte identifier; service 22 uses a two-byte DID.
- Separate request discovery from response decoding. A response can be recorded as raw evidence before a decoder exists.
- Prefer source-backed candidate discovery over numeric scans. Use ranked, relevant candidates and begin useful logging as soon as the configured signal set is available.
- Maintain capabilities and measurements per responding ECU. Include ECU addressing and available calibration or software identity in profile matching.
- Reuse cached knowledge when the ECU profile matches, and refresh it when the relevant identity or definition changes.
- Measure request latency and response rate on real adapters before tuning polling rates, timeouts, or request grouping.
- Keep measurement scheduling, catalog lookup, decoding, persistence, and rendering as separate responsibilities on the common pipeline.
- The long `ExtendedLoggingMode21-22.md` file is a design reference. Its examples are conceptual; implementation follows the Rust/Tauri/Svelte architecture above.

## Applying `ExtendedLoggingMode21-22.md`

- Treat the addon as a roadmap, not permission to transmit every example or implement every proposed phase. Its C++ structs, class names, file layout, and pseudocode are illustrative; map responsibilities onto the existing Rust core, Tauri command boundary, Svelte UI, v0.3 SQLite schema, and shared sample pipeline.
- Keep a request as the identity of a service, identifier, and ECU route. A request may decode into several channels; retain the exact raw response and keep each channel's decoder, unit, and validation state separate.
- Parse positive `61` and `62` replies only when they echo the requested local identifier or two-byte DID. Preserve negative-response service/NRC, response-pending, timeout, wrong-ECU, and wrong-length outcomes as distinct evidence; a negative reply is not always proof that an identifier is unsupported.
- Search the versioned, licensed v0.3 catalog and validate exact request/response addressing before considering an OEM candidate. Never infer `7E0/7E8` or another ECU route universally. Never promote a raw response directly to a decoded signal or vehicle-validated definition.
- For an explicitly authorized finite vehicle research run, review every relevant source-backed read definition in the repository, including older catalogs and plausible related-platform leads when exact-model coverage is missing. Label related-platform leads as lower confidence and report their outcomes separately; this does not authorize guessing or sweeping undocumented identifiers.
- In ordinary connection and logging workflows, do not sweep numeric identifiers. A future Mode 21 `00–FF` exploration may exist only as a separate, explicit, bounded research action; it must be paced, source/ECU scoped, cancellable, and keep evidence marked as research. Mode 22 discovery must use ranked candidates; full `0000–FFFF` exploration remains disabled by default and must never run during normal startup.
- Normal acquisition may poll only definitions explicitly enabled for automatic requests and cleared for the exact supported vehicle/ECU scope. Unvalidated candidates (`auto_request=0`) stay out of the ordinary picker, presets, and poll queue. Discovery and cache validation are lower priority than configured logging and must not delay or drop recorded measurements.
- Serialize exchanges through the existing adapter/ECU request owner. Do not launch independent discovery and logging workers that can transmit concurrently to the same ECU. Measure latency and error rates before changing timeouts, rates, or grouping; do not copy the addon's illustrative 15 ms timeout or request size as a product default.
- Use typed request outcomes rather than a supported/unsupported boolean. At minimum preserve raw support, decoded support, negative response/NRC, no response, malformed or wrong-length response, temporary unavailability, response-pending, and any session/security requirement as distinct states.
- Cache useful positive and negative research results per stable vehicle and ECU fingerprint, including source revision and timestamps. Revalidate a matching cache quickly, preserve the previous profile when a refresh fails, and keep cache evidence distinct from vehicle validation.
- Never automatically change diagnostic sessions, request security access, write data, clear codes, run routines, or operate actuators. Do not expose arbitrary raw-hex/STPX entry. Any exception requires a separately scoped procedure and the approval rules in the applicable safety instructions.

## Development workflow

1. Inspect the affected code, data, documentation, and existing tests before editing.
2. Implement a complete, small vertical slice before broadening the feature.
3. Add or update automated tests for parsing, decoding, storage, and UI behavior affected by the change.
4. Update the architecture notes and concise project status when behavior or data format changes.
5. Run the most relevant formatting, test, type-check, and build commands available in the current environment.
6. In completion reports, list implemented behavior, exact verification results, environment limitations, and the next concrete task.

## Feature delivery and performance

- Keep the graph responsive and prioritize displaying acquired samples over optional work such as indexing or background calculations.
- Avoid dropping recorded samples to keep UI rendering fast; throttle or coalesce display updates when needed.
- Measure real performance instead of assuming adapter throughput, ECU timing, or multi-request behavior.
- Keep CSV export, session replay, live acquisition, and simulated acquisition on consistent measurement and recording contracts.
- Prefer incremental capabilities with focused tests and clear data provenance over speculative subsystem frameworks.
