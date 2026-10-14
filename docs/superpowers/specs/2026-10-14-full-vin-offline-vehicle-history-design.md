# Full VIN, Offline Decode, and Vehicle History — accepted design

Status: owner approved implementation on 2026-10-14. Prepared against e6439a53966a204515b2a5cec61332c74cb2752b plus existing uncommitted profile/replay work. Implementation is in the same uncommitted worktree; final verification/review notes are in the implementation plan and PROJECT_STATUS.md. This adds application-local identity and history; it does not alter the authoritative PID catalog.

## Owner decisions

- Decode VINs fully offline. Decoder, recognition and history operations make no online VIN requests.
- Show and copy the complete VIN in vehicle and report detail views. Keep the profile-match digest and masked VIN separate.
- Bundle a small, version-pinned WMI lookup built from official NHTSA WMI metadata. Record each source, revision or retrieval date, content hash, attribution and license. Unknown WMI entries remain unknown.
- Provide only structural and source-backed WMI results. Do not promise model, engine or trim without a trustworthy bundled manufacturer chart or an owner-entered detail.
- Keep the model-year cycle unresolved unless a source-backed class rule applies or the owner confirms a year. Never select a cycle from the current date.
- Store profiles, vehicle identity and the diagnostic-history index in the existing app_data_dir()/profiles.sqlite. Keep vehicle-reads.jsonl as immutable report evidence.
- Add optional vehicle-context metadata to native .lognook sessions. Old sessions remain readable with no inferred context.

## Existing behavior to extend

- VehicleDiagnosticsPanel.svelte explicitly reads VIN, calibration, ECU name, DTC categories and Mode 06. Full VIN values already display in its Identity tab. commands/vehicle_diagnostics.rs appends reports to vehicle-reads.jsonl.
- Profile recognition reads Mode 09 VIN/calibration/ECU name through the serialized acquisition owner, then retains only digest and mask in VehicleIdentityObservation. The profile store is schema v1 and has no full-VIN identity or report index.
- SessionBrowser.svelte lists .lognook recordings only. The diagnostics dialog keeps only its latest report in component state.
- RecordingWriter stores schema-v1 metadata in the recording SQLite metadata table. RecordingInfo has no vehicle context.
- The typed vehicle_profiles MCP operation uses the same profile service and requires unlocked Agent Access. read_vehicle_diagnostics returns a report but does not expose history.

## Local identity and decode contract

Add a VehicleIdentityRecord keyed by the existing ProfileScope plus vin_digest. Same VIN text in live, simulator and CAN-only scopes does not merge identity details, corrections or history. Store manual VINs and details under the profile's existing scope, with origin (ecu_report, ecu_recognition, legacy_report_import, owner_entered, or agent_entered) kept separate from ECU evidence; do not add a synthetic manual ProfileScope. Preserve each VIN evidence origin/time/ECU or report reference when evidence is added, rather than overwriting provenance. The canonical VIN digest and profile fingerprint algorithm remain unchanged. Full VIN, digest, mask, decode, manual details and vehicle/ECU validation are separate values; manual input never becomes ECU evidence.

History keys include ProfileScope and VIN digest, so simulator, live OBD and CAN-only contexts do not merge. Owner/agent origin and an explicit manual report-association row remain distinct provenance. Manual input never overwrites a different ECU-observed VIN; conflicting ECU VINs leave the report unassigned unless the owner separately creates a visibly manual association, which does not change the captured conflict or validation.

The offline decoder accepts a trimmed 17-character VIN with permitted characters. It returns shape/check-digit status, year code and possible years, an optional source-backed WMI assignment, and source/version provenance. A US §565.15 check-digit mismatch is a warning, not universal VIN rejection; where jurisdiction/scope is unknown, report the check-digit scheme as unknown. Do not infer assembly plant or assembly country from WMI. A WMI-supplied manufacturer country is labeled as the assignment country.

The WMI table may have three-character entries and explicitly sourced six-character low-volume identifiers. Use positions 1–3 plus 12–14 only when the bundled source declares that key; do not map every 9 in position 3 to an invented manufacturer. Apply model-year class rules only when the bundled source supports the relevant class/scope. Otherwise return unresolved candidate years and accept a separate owner-confirmed year.

Do not install or bundle the full vPIC server database. NHTSA's [vPIC download page](https://vpic.nhtsa.dot.gov/downloads/) describes standalone SQL Server/PostgreSQL databases; that dependency is outside this bounded desktop feature. Use official WMI assignment metadata during dataset preparation, never a VIN lookup at runtime. Structural rules follow [49 CFR §565.15](https://www.ecfr.gov/current/title-49/subtitle-B/chapter-V/part-565/subpart-B/section-565.15).

Owner-entered make/model/year/engine/trim fields are correction records with field, value, actor (owner_ui or agent_access), timestamp and note. Show decoded/source-backed and manual fields separately. Unknown values remain blank. Corrections do not change the offline decode, profile matching or OEM eligibility.

## Legacy profile recovery

Migrate profile-store schema v1 transactionally without dropping or rewriting selections, VIN digests/masks, ECU variants or observations. Create scoped identity rows for existing digests with full_vin = NULL and state legacy_hash_only. A SHA digest cannot be reversed. Enrich it only from a unique, valid, conflict-free VIN in that profile's historical evidence: either its decoded VIN value or a strictly decoded positive Mode 09 PID 02 raw payload, with outcome and ECU provenance retained. An owner may also supply a full VIN whose canonical digest exactly matches. A mismatch leaves the profile untouched and does not merge profiles. Manual input is labeled and cannot produce an exact ECU match by itself.

## Diagnostic reports and vehicle history

Keep vehicle-reads.jsonl append-only and authoritative for report evidence. Add a stable report ID and a serde-defaulted per-report context snapshot to new DiagnosticReport values. The snapshot distinguishes (a) a unique positive VIN in that report, (b) same-connection-generation recognition evidence when that report's VIN read is unavailable, (c) an explicitly selected manual profile context, (d) conflict and (e) unassigned. Preserve ECU variants, generation, profile revision, VIN origin/digest/full VIN, scope and decoded/manual summary as captured.

Add a diagnostic_history index to profiles.sqlite with report ID, managed relative JSONL path, byte offset/length, report SHA-256, timestamp, profile scope, VIN digest, identity/conflict state, ECU IDs, summary counts and immutable context. Listing uses this index; detail reads the indexed byte range and verifies its hash. Detail preserves per-ECU DTC categories, Mode 06 bitmap/monitor reads, typed outcomes, exact request/response bytes and routes, raw adapter response, latency, NRC and error detail. Missing or changed evidence is shown as unavailable/changed, never replaced by the current profile.

Backfill legacy JSONL incrementally and idempotently. Read at most 1 MiB per report line, continue past malformed/oversized lines while counting them, and leave JSONL untouched. Retry a trailing incomplete line. Legacy association uses only that report's own valid decoded VIN value or a strictly decoded positive Mode 09 PID 02 raw response payload. Preserve read outcome and ECU provenance; never infer from an arbitrary raw-response string. Conflicts stay unassigned. Never use a vehicle connected later. If a VIN is decoded after capture, label it a later offline derivation. A later owner assignment is a separate append-only association with actor/time/note and does not rewrite captured evidence.

History supports profile scope, vehicle/VIN, date range and ECU filters, ordered by timestamp and report ID descending, with a 50-item default and 100-item maximum page. Status exposes indexing progress and malformed-line count; indexing is cancellable without hardware commands. Reports remain local unless an existing export/share action is used.

## Native recording context

Add optional vehicle_context_v1 JSON to the existing .lognook metadata table without invalidating schema v1. It captures full VIN/origin, digest/mask, profile scope, connection generation, profile ID/revision, observed ECU variants, offline decoder/WMI revision and owner-detail snapshot available at recording start. RecordingInfo returns None for older sessions.

Capture context through normal Record, Record From Connect and automatic-trigger starts using the shared pipeline. Store a context_from_elapsed_ns boundary: samples backfilled before Record From Connect/trigger activation remain without the new context; post-activation samples use the frozen snapshot. A newly entered manual VIN is never stamped onto earlier unknown samples. Later recognition/corrections do not mutate the snapshot. Session listings group matching recordings and reports under the same scoped vehicle but keep separate recording/report types. Label whether a native session includes a full VIN in its metadata; existing explicit share/export actions may transfer/write that metadata, with no added approval prompt. CSV export does not gain VIN columns by default.

## Operator and MCP workflow

- Profiles or Vehicle Diagnostics shows a local summary with full VIN show/copy, WMI/source, decode status and separate owner-entered details. Decode VIN offline performs local computation only. Unknown WMI, unsupported class, ambiguous year and invalid VIN have distinct outcomes.
- Keep Read diagnostics explicit. Add OpenPort identity-only reads for service 09 PIDs 02, 04 and 0A through the serialized owner. Keep profile-identity capability separate from full vehicle-diagnostics support: service 03/07/0A DTC reads and Mode 06 remain unsupported on OpenPort.
- Add Vehicle History, usable disconnected, with Diagnostics and Recordings sections, scoped vehicle selector, date/ECU filters, paged report detail and links to the existing session player. Do not put history in the unrelated app-communication Diagnostics panel.
- Add typed vehicle_profiles MCP operations for identity detail, offline decode, manual corrections/legacy linking, report list/detail and history import status/cancellation. Use the existing Agent Access gate and same core service. Lists stay masked; return full VIN only on explicit detail. Manual agent input is agent_entered, never ECU-verified. read_vehicle_diagnostics persists its report and returns its ID.
- Decoder, recognition and history operations make no online VIN requests. Existing explicit share/export actions may include the frozen full VIN in native recording or diagnostic evidence; describe actual file contents and add no approval prompt. No operation begins automatic OEM polling or promotes validation.

## Non-goals

No online VIN decode or remote vehicle history; no full vPIC SQL Server/PostgreSQL runtime; no inferred trim/engine/model from unsupported positions; no automatic VIN-cycle guess; no change to data/lognook_pids.sqlite or OEM/validation gates; no report rewrite; no guessed identity for old sessions; no customer, billing, recall or repair-history database.
