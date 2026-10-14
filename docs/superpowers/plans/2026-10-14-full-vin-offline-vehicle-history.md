# Full VIN Offline Decode and Vehicle History Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Store, display and copy full vehicle VINs with offline source-backed details, immutable diagnostic history and backward-compatible recording context.

**Architecture:** Keep vehicle identity, profile data and a byte-range diagnostic-history index in app_data_dir()/profiles.sqlite; vehicle-reads.jsonl remains the immutable report source. Decode locally from VIN structure and a small pinned WMI dataset. Add optional versioned context to native recording metadata through the existing recording pipeline. Route UI and MCP through the same services.

**Tech Stack:** Existing Rust 2024 workspace, rusqlite, serde/serde_json, sha2, Tauri 2, Svelte/TypeScript, Node UI tests and the existing x86 J2534 worker protocol. No new runtime network dependency or database server.

**Spec:** [Full VIN, Offline Decode, and Vehicle History design](../specs/2026-10-14-full-vin-offline-vehicle-history-design.md)

## Execution status — 2026-10-14

Implementation and verification are complete on the existing dirty `codex/can-event-freeze-frame` worktree based at `e6439a53966a204515b2a5cec61332c74cb2752b`. The requested Luna max implementation agents stopped at the account usage limit; integration was completed locally. Scoped full-VIN provenance, offline decode, immutable history indexing/import, identity-only OpenPort reads, native recording context, the Profiles dialog Vehicle History view, and typed MCP operations are implemented. History groups reports and recordings by scoped VIN and filters reports by scope/fingerprint/ECU/time. The view lives inside Profiles (instead of a SessionBrowser tab); SessionBrowser displays optional recording-context summaries. An independent Luna max review's two findings were fixed, and its follow-up found no remaining actionable issues.

Final verification on the current working tree: `cargo test --workspace -j 4`, `cargo check -p lognook-desktop --lib --locked -j 4`, `pnpm.cmd run test:ui` (211/211), and `pnpm.cmd run build` (zero Svelte errors/warnings) passed. The release app is `C:\Users\inouk\LogNook-can-event-freeze-frame\target\release\lognook-desktop.exe`; the 32-bit worker is `C:\Users\inouk\LogNook-can-event-freeze-frame\target\i686-pc-windows-msvc\release\lognook-j2534-worker.exe`. Tauri bundling is disabled, so no installer was produced. No vehicle or adapter was physically tested. No commit or push was made because this worktree contains earlier local CAN/profile changes. The original step checklist below records the accepted plan; completed items are checked, while commit steps were replaced with the deliberate shared-worktree choice.

## Global Constraints

- Runtime vehicle/profile/history data belongs in app_data_dir()/profiles.sqlite; the authoritative PID catalog is untouched.
- Decoder, recognition and history operations make no online VIN requests. Existing explicit share/export actions retain their current behavior and may contain the frozen full VIN.
- Keep the existing canonical VIN digest/fingerprint unchanged; store full VIN separately with an explicit origin.
- Key identity/history by existing ProfileScope plus VIN digest. Keep manual entry/association as provenance within that scope; never merge simulator, live or CAN-only history.
- Unknown WMI/model/engine/trim values stay blank unless source-backed or owner-entered and labeled.
- A model-year cycle is not selected from the current date; class-specific disambiguation requires backed scope.
- Manual/MCP-entered data never becomes ECU evidence, validation or OEM polling eligibility.
- vehicle-reads.jsonl is not modified during history import. Every indexed detail verifies its byte-range SHA-256.
- OpenPort reads only Mode 09 PIDs 02, 04 and 0A; DTC categories and Mode 06 remain unsupported there.
- Add only optional .lognook metadata key vehicle_context_v1; schema-v1 sessions without it remain readable.
- Owner approved implementation on 2026-10-14. Preserve the pre-existing uncommitted branch work; do not commit or push this handoff.

## Review Focus

- **Invalid or non-North-American VIN check digit:** invalid shape is rejected; an unknown check-digit scheme is not. Test the published §565.15 vector and unknown-scope mismatch in Task 1.
- **Unknown and low-volume WMI:** unknown WMI stays blank; six-character lookup requires an exact source-declared extension. Test exact/missing extension in Task 1.
- **Partial or conflicting ECU reads:** retain per-ECU outcomes/raw evidence and never choose one of two VINs. Test positive, negative, malformed, timeout and conflict cases in Tasks 4–5.
- **Legacy JSONL and evidence changes:** import is bounded/idempotent; malformed/oversized lines count and continue; missing/changed report bytes fail visibly. Test in Task 3.
- **Old identity/recording state:** hash-only profiles and sessions without context remain intact; old data is never assigned a later manual VIN. Test migration and from-connect/autotrigger context boundaries in Tasks 2 and 5.

## Execution and ownership

The original implementation sequence used exclusive file ownership for parallel tasks. The implementation agents stopped at the account usage limit, so integration was completed locally. One independent Luna max review and follow-up were completed after integration. No per-task commits or push were made; preserve this combined worktree.

---

### Task 1: Offline VIN decoder and pinned WMI data

**Files:**
- Create: crates/lognook-core/src/vehicle_profiles/vin_decode.rs
- Create: crates/lognook-core/data/vin_wmi.csv
- Create: crates/lognook-core/data/vin_wmi_sources.json
- Modify: crates/lognook-core/src/vehicle_profiles.rs, crates/lognook-core/src/lib.rs
- Test: crates/lognook-core/tests/vin_decode.rs

**Interfaces:**
- Produces decode_vin_offline(vin: &str) -> Result<VinDecodeResult, VinDecodeError>.
- VinDecodeResult includes normalized VIN, check-digit status, year code/candidate years/optional class-backed year, optional WMI assignment, decoder revision and WMI dataset SHA-256.
- WmiAssignment contains manufacturer, optional manufacturer-assignment country/type/class scope, source ID and source revision. It contains no assembly-country, model, engine or trim field.
- WMI rows contain an exact three-character key or a source-declared six-character low-volume key. Source metadata records URL, revision/retrieval, license and attribution.

- [x] Add tests for valid 17-character input, prohibited I/O/Q, a published §565.15 check-digit vector, unknown-scope checksum warning, missing/unknown WMI, exact six-character low-volume match, position-3 9 without a matching extension, and unresolved 30-year year codes.
- [x] Run `cargo test -p lognook-core --test vin_decode`; final decoder tests pass.
- [x] Implement the pure offline parser and static WMI lookup. Return structural results for unknown WMI. Apply check-digit/year class rules only where bundled source scope supports them.
- [x] Add a small static WMI dataset prepared from official NHTSA GetWMIsForManufacturer/DecodeWMI metadata. Pin source URL, revision or retrieval date, content hash, license and attribution. Keep data preparation outside runtime/build execution; do not include full vPIC.
- [x] Run cargo test -p lognook-core --test vin_decode; verify no VIN decoder path has a network client/request.
- [x] Preserve the changes in the shared working tree; no per-task commit or push was made.

### Task 2: Scoped local identity, profile migration and history index

**Files:**
- Modify: crates/lognook-core/src/vehicle_profiles.rs, crates/lognook-core/src/vehicle_profiles/store.rs
- Modify: apps/desktop/src-tauri/src/vehicle_profiles.rs
- Test: crates/lognook-core/tests/vehicle_profiles.rs
- Test: crates/lognook-core/tests/vehicle_history.rs

**Interfaces:**
- Reuses ProfileScope::{Live, Simulator, CanOnly}; each identity key is (scope, vin_digest). Manual origin and associations are provenance fields, not a new profile scope.
- Adds VinOrigin::{EcuReport, EcuRecognition, LegacyReportImport, OwnerEntered, AgentEntered}, ManualVehicleField::{Make, Model, ModelYear, Engine, Trim}, ManualDetailActor::{OwnerUi, AgentAccess}, and ManualVehicleDetail { field: ManualVehicleField, value: String, actor: ManualDetailActor, entered_at_unix_ms: u64, note: Option<String> }.
- Adds VehicleIdentityRecord { scope: ProfileScope, vin_digest: String, full_vin: Option<String>, origin_evidence: Vec<VinEvidenceSource>, decoder_snapshot: Option<VinDecodeResult>, owner_details: Vec<ManualVehicleDetail>, revision: u64 } and VehicleIdentitySnapshot { scope, connection_generation, profile_id/revision, full_vin/digest/mask/origin, observed_ecus, decoded_snapshot, owner_details }.
- VinEvidenceSource { origin: VinOrigin, recorded_at_unix_ms: u64, report_id: Option<String>, ecu_id: Option<String>, connection_generation: Option<u64>, actor: Option<ManualDetailActor> } retains source evidence. Repeated ECU evidence appends to provenance; it does not erase an earlier owner-entered source.
- ManualReportAssociation { report_id: String, scope: ProfileScope, vin_digest: String, actor: ManualDetailActor, associated_at_unix_ms: u64, note: Option<String> } records an explicit later grouping without changing the report snapshot, profile match or validation.
- Adds DiagnosticHistoryFilter { scope: Option<ProfileScope>, vin_digest: Option<String>, ecu_id: Option<String>, from_unix_ms: Option<u64>, to_unix_ms: Option<u64>, page_size: Option<u16>, cursor: Option<String> } and DiagnosticHistoryPage { rows: Vec<DiagnosticHistoryRow>, next_cursor: Option<String> }.
- DiagnosticHistoryRow { report_id, started_at_unix_ms, scope, vin_digest, masked_vin, identity_source, vin_conflict, ecu_ids, dtc_counts, mode06_read_count, evidence_status }; DiagnosticReportLocator { report_id, source_relative_path, byte_offset, byte_length, sha256, scope, vin_digest, identity_source, vin_conflict, context_snapshot }.
- HistoryIndexState::{Idle, Indexing, Complete, Cancelled, Failed}; HistoryIndexStatus { state, indexed_reports, scanned_bytes, total_bytes, malformed_lines, oversized_lines, error }; HistoryEvidenceStatus::{Available, Missing, Changed, Malformed}; DiagnosticReportDetail { index: DiagnosticHistoryRow, evidence_status: HistoryEvidenceStatus, report: Option<DiagnosticReport> }.
- VehicleProfileService exposes get_vehicle_identity(scope: ProfileScope, vin_digest: &str) -> Result<Option<VehicleIdentityRecord>, String>, save_vehicle_identity(record: VehicleIdentityRecord, expected_revision: Option<u64>) -> Result<VehicleIdentityRecord, String>, decode_vehicle_identity(scope: ProfileScope, vin_digest: &str) -> Result<VehicleIdentityRecord, String>, link_legacy_vehicle_vin(profile_id: &str, expected_revision: u64, full_vin: &str) -> Result<VehicleIdentityRecord, String>, and set_manual_vehicle_detail(scope: ProfileScope, vin_digest: &str, field: ManualVehicleField, value: String, actor: ManualDetailActor, note: Option<String>) -> Result<VehicleIdentityRecord, String>. History methods are add_diagnostic_locator(locator: DiagnosticReportLocator) -> Result<(), String>, get_diagnostic_locator(report_id: &str) -> Result<Option<DiagnosticReportLocator>, String>, list_diagnostic_history(filter: DiagnosticHistoryFilter) -> Result<DiagnosticHistoryPage, String>, and append_manual_report_association(report_id: String, scope: ProfileScope, vin_digest: String, actor: ManualDetailActor, note: Option<String>) -> Result<ManualReportAssociation, String>. Linking requires exact canonical-digest equality and does not change profile matching. Manual VINs remain under the existing profile scope with manual origin. UI/MCP callers cannot choose actor; Tauri assigns OwnerUi and the MCP boundary assigns AgentAccess.

- [x] Add regression tests for schema-v1 migration retaining every profile/selection/ECU observation and creating legacy_hash_only identity rows without digest inversion; also test unsupported newer DB remains untouched.
- [x] Run cargo test -p lognook-core --test vehicle_profiles --test vehicle_history. Final migration and identity tests pass.
- [x] Test same VIN remains separate across live/simulator/CAN-only scopes; test a manual-origin VIN under its existing profile scope cannot replace a different ECU-observed VIN; test explicit report association preserves a conflict, digest-preserving legacy link, mismatch rollback and owner_ui versus agent_access provenance.
- [x] Migrate transactionally to schema v2, adding scoped vehicle identity/evidence/detail tables and diagnostic_history, manual associations and import-state tables with indexes for scoped VIN/date and ECU/date.
- [x] Implement stable descending pagination by timestamp/report ID, 50 default and 100 maximum, date/ECU/scope/VIN filtering, and append-only manual association.
- [x] Test locator uniqueness, pagination, association provenance and migration v1-to-v2. Run cargo test -p lognook-core --test vehicle_profiles --test vehicle_history.
- [x] Preserve the changes in the shared working tree; no per-task commit or push was made.

### Task 3: Immutable report capture, indexing and legacy import

**Files:**
- Modify: crates/lognook-core/src/vehicle_diagnostics/model.rs
- Modify: apps/desktop/src-tauri/src/commands/vehicle_diagnostics.rs
- Modify: apps/desktop/src-tauri/src/commands/vehicle_profiles.rs
- Modify: apps/desktop/src-tauri/src/lib.rs
- Test: apps/desktop/src-tauri/src/commands/vehicle_diagnostics.rs
- Test: apps/desktop/src-tauri/src/commands/vehicle_profiles.rs

**Interfaces:**
- Add serde-defaulted DiagnosticReport.report_id: Option<String>, connection_generation: Option<u64>, vehicle_context: Option<VehicleIdentitySnapshot>.
- Add Tauri commands get_vehicle_identity(scope: ProfileScope, vin_digest: String) -> Result<Option<VehicleIdentityRecord>, String>, decode_vehicle_vin_offline(scope: ProfileScope, vin_digest: String) -> Result<VehicleIdentityRecord, String>, set_manual_vehicle_detail(scope: ProfileScope, vin_digest: String, field: ManualVehicleField, value: String, note: Option<String>) -> Result<VehicleIdentityRecord, String>, and link_legacy_vehicle_vin(profile_id: String, expected_revision: u64, full_vin: String) -> Result<VehicleIdentityRecord, String>. These wrappers call VehicleProfileService; the Tauri boundary sets owner_ui provenance.
- Add list_vehicle_diagnostic_history(filter: DiagnosticHistoryFilter) -> DiagnosticHistoryPage, get_vehicle_diagnostic_report(report_id: String) -> DiagnosticReportDetail, vehicle_history_import_status() -> HistoryIndexStatus, start_vehicle_history_import() -> HistoryIndexStatus and cancel_vehicle_history_import() -> HistoryIndexStatus.
- A new report's identity source is report_ecu, same_generation_recognition, manual_profile, conflict or unassigned. Scope is always retained.

- [x] Add regression tests for report ID creation before JSONL serialization, exact byte locator, and context priority: unique positive report VIN; otherwise same-generation recognition; otherwise explicitly selected manual profile; conflicting VINs stay unassigned.
- [x] Run cargo test -p lognook-desktop --lib. Final report ID, context, and index tests pass.
- [x] Update explicit diagnostics and profile-recognition paths to append JSONL first, then index its locator through VehicleProfileService. Return the same report ID to UI/MCP.
- [x] Register the identity/decode/manual-link Tauri wrappers in lib.rs and test each against the same shared service used by MCP.
- [x] Implement resumable import of app-data vehicle-reads.jsonl. Cap each line at 1 MiB; use bounded reads; dedupe locators; count and continue past malformed/oversized lines; retry an incomplete trailing line; never rewrite the file.
- [x] Recover old identity only from that report's own valid decoded VIN value or strictly decoded positive Mode 09 PID 02 raw response, retaining outcome and ECU provenance. Require exact digest. Never use the currently connected vehicle when importing a legacy report.
- [x] Test repeated import, malformed-line continuation/count, oversized line, incomplete final line, positive PID 02 raw-response decode, invalid/negative PID 02 ignored, report conflict, checksum mismatch, missing/changed source bytes, and cancellation/status transitions.
- [x] Run `cargo test -p lognook-desktop --lib` and changed command tests; the workspace suite passes. Preserve the changes in the shared tree without committing or pushing.

### Task 4: OpenPort identity-only Mode 09 path

**Files:**
- Modify: crates/lognook-j2534-protocol/src/lib.rs
- Modify: workers/lognook-j2534-x86/src/main.rs
- Modify: workers/lognook-j2534-x86/src/session.rs
- Modify: workers/lognook-j2534-x86/src/j2534.rs
- Modify: crates/lognook-core/src/openport2.rs
- Modify: crates/lognook-core/src/vehicle_diagnostics/model.rs
- Modify: crates/lognook-core/src/engine.rs
- Modify: crates/lognook-core/src/lib.rs
- Modify: apps/desktop/src-tauri/src/commands/vehicle_profiles.rs
- Test: crates/lognook-j2534-protocol/src/lib.rs
- Test: workers/lognook-j2534-x86/src/session.rs, workers/lognook-j2534-x86/src/j2534.rs
- Test: crates/lognook-core/src/openport2.rs, crates/lognook-core/src/vehicle_diagnostics/model.rs

**Interfaces:**
- Add protocol Mode09Payload { ecu_id, bytes, latency_us }, WorkerRequest::ReadMode09 { pid }, and WorkerResponse::Mode09Responses { responses }.
- Allow only service 09 PIDs 02, 04 and 0A. Preserve actual ECU responder, route, exact bytes, latency and typed outcomes.
- Reuse ResearchTestRequest::Diagnostic and existing acquisition ProfileIdentityQuery. Do not add full-diagnostics or scanner capability.
- Add supports_profile_identity(adapter_family: Option<&str>) -> bool as a separate capability and export it from core lib.rs. It is true for Xtool A30/A30M Bluetooth SPP, ELM327 Bluetooth SPP, OBDLink SX and OpenPort 2.0; false for other/unknown families. Keep supports_vehicle_diagnostics false for OpenPort.
- Change only the profile-identity capability gates in engine.rs (current lines 603 and 641) and commands/vehicle_profiles.rs identity_supported metadata. Leave full DTC/Mode 06 diagnostic gating unchanged.

- [x] Add protocol/worker regression tests for valid requests/responses and rejection of non-whitelisted PIDs, malformed JSON and unknown fields; add a failing capability test proving identity-only OpenPort support does not imply full diagnostic support.
- [x] Run cargo test -p lognook-j2534-protocol and cargo test -p lognook-j2534-worker. Final protocol and worker tests pass.
- [x] Run cargo test -p lognook-core profile_identity. Final profile-identity and capability tests pass.
- [x] Extend J2534 session/worker with one serialized read-only Mode 09 query; preserve responder/raw bytes/latency and return negative, malformed and timeout outcomes without losing later identity-query outcomes.
- [x] Extend OpenPort2Adapter::execute_research_test only for DiagnosticReadKind::{Vin, CalibrationId, EcuName} matching PIDs 02/04/0A. Keep full DTC/Mode 06 support false.
- [x] Test fake J2534 responses for 0902/0904/090A, responder route, partial read, negative/malformed response, timeout and attempted disallowed PID. Assert no DTC/Mode06 frame is emitted.
- [x] Test supports_profile_identity accepts OpenPort only for identity reads while supports_vehicle_diagnostics continues to reject full vehicle diagnostics there; verify engine/profile recognition gates use the former only.
- [x] Run protocol, worker, and OpenPort tests; `cargo test --workspace -j 4` passes. Preserve the changes in the shared tree without committing or pushing.

### Task 5: Versioned context in native recordings

**Files:**
- Modify: crates/lognook-core/src/model.rs
- Modify: crates/lognook-core/src/recording.rs
- Modify: crates/lognook-core/src/recording_view.rs
- Modify: crates/lognook-core/src/engine.rs
- Modify: crates/lognook-core/src/pipeline.rs
- Modify: crates/lognook-core/src/pipeline_worker.rs
- Modify: crates/lognook-core/src/pipeline_worker/live_samples.rs
- Modify: crates/lognook-core/src/pipeline_worker/trigger.rs
- Modify: apps/desktop/src-tauri/src/commands.rs
- Modify: apps/desktop/src-tauri/src/commands/sessions.rs
- Modify: apps/desktop/src-tauri/src/commands/vehicle_profiles.rs
- Test: crates/lognook-core/src/recording.rs, crates/lognook-core/src/recording_view.rs, crates/lognook-core/src/pipeline_worker.rs
- Test: apps/desktop/src-tauri/src/commands/sessions.rs

**Interfaces:**
- Adds RecordingVehicleContextV1 { snapshot: VehicleIdentitySnapshot, context_from_elapsed_ns: i64 }; the boundary uses the same elapsed_ns clock as samples.
- RecordingWriter::create_with_vehicle_context(path: &Path, context: Option<RecordingVehicleContextV1>) -> Result<RecordingWriter, LogNookError>; LogNookEngine::start_recording_with_context(path: &Path, snapshot: Option<VehicleIdentitySnapshot>, expected_generation: u64) -> Result<(), LogNookError>; LogNookEngine::start_recording_from_connect_with_context(path: &Path, snapshot: Option<VehicleIdentitySnapshot>, expected_generation: u64) -> Result<RetrospectiveRecordingStart, LogNookError>. Existing no-context methods remain wrappers. The common owner computes the boundary at actual activation; callers do not pass zero or a wall-clock value.
- Add LogNookEngine::update_vehicle_recording_context(snapshot: Option<VehicleIdentitySnapshot>, expected_generation: u64) -> Result<(), LogNookError> and the matching pipeline command. The pipeline stores only the current generation snapshot; it clears it on disconnect, generation change or profile invalidation. Automatic-trigger capture reads this current snapshot at trigger activation, not the stale arm-time snapshot.
- Add optional RecordingInfo.vehicle_context with serde default. Persist under vehicle_context_v1 in the existing recording metadata table; keep recording schema version 1.

- [x] Add regression tests for new/old recording round-trip, full VIN origin/revision, absent old context, and unchanged sample/raw-byte rows.
- [x] Run cargo test -p lognook-core recording. Final context round-trip and boundary tests pass.
- [x] Thread an optional scoped snapshot through normal Record and Record From Connect, computing context_from_elapsed_ns on the common sample/session clock at activation. For Record From Connect, samples before the boundary remain unassociated.
- [x] Update the pipeline's current generation snapshot after successful recognition/profile changes and clear it on reconnect/disconnect or invalidation. Automatic-trigger writer creation freezes the latest snapshot only when the trigger fires and only if its generation still matches.
- [x] For automatic-trigger prehistory, keep samples before trigger activation unassociated. A manual VIN entered after those samples is never stamped onto them; later corrections do not mutate an active/completed snapshot.
- [x] Extend session listing metadata with the optional context summary; old .lognook sessions and playback continue to work.
- [x] Test normal Record boundary is nonzero when acquisition elapsed time is nonzero; test Record From Connect/autotrigger earlier backfill remains unassociated; test context refresh after trigger arm is used at fire, while stale generation is omitted.
- [x] Run recording and session tests; `cargo test --workspace -j 4` passes. Preserve the changes in the shared tree without committing or pushing.

### Task 6: Vehicle identity, diagnostics and history UI

**Files:**
- Modify: apps/desktop/src/VehicleProfilePanel.svelte
- Modify: apps/desktop/src/VehicleDiagnosticsPanel.svelte
- Modify: apps/desktop/src/SessionBrowser.svelte
- Modify: apps/desktop/src/sessionBrowser.ts
- Modify: apps/desktop/src/vehicleProfiles.ts
- Modify: apps/desktop/src/vehicleDiagnostics.ts
- Modify: apps/desktop/src/App.svelte
- Test: apps/desktop/tests/vehicle-profiles.test.mjs
- Test: apps/desktop/tests/vehicle-diagnostics.test.mjs
- Test: apps/desktop/tests/session-browser.test.mjs

**Interfaces:**
- Add typed frontend VehicleIdentityRecord, VinDecodeResult, DiagnosticHistoryRow/Filter, DiagnosticReportDetail, HistoryIndexStatus and RecordingVehicleContext types matching Rust payloads.
- vehicleProfiles.ts exposes typed invokes for identity detail, offline decode, owner detail correction and legacy VIN linking. vehicleDiagnostics.ts exposes history list/detail/import status/cancel.
- SessionBrowser gets a separate Vehicle History tab with Diagnostics and Recordings sections; existing recording actions remain recording-only.

- [x] Write failing UI tests for full/masked VIN state, clipboard failure, partial/unknown decode labels, unresolved year, scoped history filters and paginated cursor handling.
- [x] Run pnpm.cmd run test:ui. Final UI helper and history tests pass.
- [x] Extend Profiles with full VIN show/copy, offline decode, source/revision labels, separated owner details and legacy hash-only recovery. Manual VIN linking requires exact digest match and stays labeled manual.
- [x] Extend Vehicle Diagnostics with a disconnected-capable local summary/history entry point, OpenPort identity-only action and per-ECU partial/conflicting outcomes. Keep DTC/Mode06 read disabled on OpenPort.
- [x] Add paged history by scope/VIN, date and ECU. Show conflict/unassigned items, import progress/malformed count, checksum-verified report detail, and later offline derivations separately from captured context.
- [x] Group recordings and reports under the same scoped vehicle but keep distinct detail/player actions. Label whether an existing native session Share/Export action includes full VIN; add no approval prompt and do not add VIN columns to CSV by default.
- [x] Run `pnpm.cmd run test:ui`; 211/211 tests pass. Preserve the changes in the shared tree without committing or pushing.

### Task 7: Typed MCP and Agent Access parity

**Files:**
- Modify: apps/desktop/src-tauri/src/agent/protocol.rs
- Modify: apps/desktop/src-tauri/src/agent/tools.rs
- Modify: apps/desktop/src-tauri/src/agent/mcp.rs
- Test: apps/desktop/src-tauri/src/agent/protocol.rs
- Test: apps/desktop/src-tauri/src/agent/tools.rs

**Interfaces:**
- Extend AgentVehicleProfileOperation with GetVehicleIdentity, DecodeVinOffline, SetManualVehicleDetail, LinkLegacyVin, ListDiagnosticHistory, GetDiagnosticReport, HistoryImportStatus, StartHistoryImport and CancelHistoryImport.
- Use the same VehicleProfileService and typed results as Tauri. Agent Access gates new operations. Full VIN is returned only by explicit identity/report detail; list/status stays masked.
- read_vehicle_diagnostics persists through the same report service and returns its report ID. stop_operation cancels history import/identity reads and reports final status.

- [x] Add schema regression tests for unknown fields, missing filter/identity, invalid page size and attempts to set ECU validation or report source.
- [x] Run cargo test -p lognook-desktop --lib. Final Agent protocol and tool tests pass.
- [x] Route actions through the shared service; preserve owner_ui/agent_access provenance; prevent manual values from changing ECU identity, profile match or OEM eligibility.
- [x] Test locked/unlocked access, offline decode with zero network requests, report detail SHA failure, import status/cancel and UI/MCP result parity.
- [x] Update tool descriptions/instructions for local-only decode, explicit full-VIN detail, history flow and OpenPort identity-only limits. Run Agent protocol and tool tests; `cargo test --workspace -j 4` passes. Preserve the shared worktree without committing or pushing.

### Task 8: Documentation, integration verification and review

**Files:**
- Modify: docs/VEHICLE_PROFILES.md, docs/FEATURE_BACKLOG.md, PROJECT_STATUS.md
- Verify: feature files and tests above

- [x] Update profile/history documentation with local full-VIN retention, offline/WMI limits and provenance, manual corrections, legacy profile/report import, OpenPort scope, recording boundaries and the distinction between research and physical validation.
- [x] Update PROJECT_STATUS.md and docs/FEATURE_BACKLOG.md only after implementation and verification. Do not mark this plan or unimplemented code as delivered.
- [x] Run cargo test -p lognook-core --test vin_decode --test vehicle_profiles --test profile_resume --test vehicle_history; cargo test -p lognook-j2534-protocol; and cargo test -p lognook-desktop --lib.
- [x] Run pnpm.cmd run test:ui and pnpm.cmd run build (use pnpm fallback if pnpm.cmd is unavailable).
- [x] Build worker with cargo build -p lognook-j2534-worker --target i686-pc-windows-msvc --release. Build standalone app with cargo build -p lognook-desktop --release --features tauri/custom-protocol -j 8.
- [x] Run git diff --check. Review migrations, JSONL immutability, all recording start paths, MCP lock/cancel behavior, scoped identities and preservation of unrelated uncommitted work.
- [x] Complete an independent Luna max review and follow-up; its findings were fixed. Record the final acceptance results; no commit or push was made.
