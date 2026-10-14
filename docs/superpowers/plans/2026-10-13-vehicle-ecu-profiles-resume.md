# Vehicle/ECU Profiles and Resume Selection Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Recognize a vehicle and ECU variants, persist its exact selections, and restore the eligible subset through a one-click Resume preview.

**Architecture:** A Rust application-state profile store and resolver sit beside the read-only research catalog. Recognition uses the existing serialized adapter owner; a staged profile connection keeps polling in standby until the user resumes. UI and Agent Access call the same resolution/application service.

**Tech Stack:** Existing Rust, rusqlite, serde, Tauri, Svelte/TypeScript; sha2 for versioned VIN/definition digests.

**Spec:** [Proposed design](../specs/2026-10-13-saved-can-replay-vehicle-profiles-design.md), profile and resume sections. Owner explicitly chose preview plus Resume, rather than automatic selection restoration.

## Global constraints

- Persist at `app_data_dir()/profiles.sqlite`; never modify the authoritative PID database for preferences or observations.
- Match VIN plus ECU protocol/address/observed calibration evidence; COM port, provider or make/model is insufficient.
- Keep observed fields distinct from owner-entered labels and manual associations.
- Fresh connection capabilities and current catalog/source/decoder/validation checks determine eligibility.
- Saved unverified OEM definitions cannot enter polling; graph-only research stays graph-only.
- Failed/partial refresh and skipped selections preserve the previous saved snapshot.
- Profile application requires stopped recording/playback/research/scans/replay; it never starts recording or CAN TX.
- Unsupported identity reads (currently OpenPort) retain ordinary generic operation and explicit manual profile association.

## Review focus

- A late identity reply or Resume token cannot act on a newer connection, even with the same adapter/port.
- ECU calibration mismatch or missing previously observed evidence prevents an exact match.
- Same vehicle with another adapter can match; same adapter attached to another vehicle cannot.
- Derived/CAN/OEM IDs can remain saved but be ineligible under current dependencies/context.
- Failed store migrations/refreshes/applications retain previous data and running selection.

## Task 1: Typed profile store and fingerprint resolver

**Files:** Create `crates/lognook-core/src/vehicle_profiles.rs`, `src/vehicle_profiles/store.rs`, `src/vehicle_profiles/resolve.rs`, `tests/vehicle_profiles.rs`; export through `src/lib.rs`.

**Interfaces:** `VehicleIdentityObservation` stores connection generation/time, observed VIN digest and per-ECU identity evidence/outcomes. `VehicleProfile` stores ID/name/revision/vehicle identity and `EcuProfileVariant` rows. `ProfileSelection` keeps separate log/graph refs plus snapshots of user-authored derived/CAN definitions; OEM refs point to catalog candidate/source/decoder revisions. `ProfileMatchState` = Exact, ReviewRequired, IdentityUnavailable, Conflict. `VehicleProfileStore::open(path: &Path)`, `list()`, `get(id)`, `save(profile, expected_revision)` return typed results. `resolve_profile_match(profile, observation) -> ProfileMatch` is pure. Limits: name 120 characters, 500 selected refs each, no empty identity treated as exact.

- [ ] Create fixtures for Ford versus Scion, identical make/year with different VINs, changed adapter/port, module calibration change/missing evidence, additional modules, manual identity, simulator and CAN-only scopes.
- [ ] Define canonical fingerprint encoding v1 with structured sorted identities; validate VIN format, hash VIN, display masked VIN. Tests distinguish ambiguous delimiters, observation order and missing versus empty data.
- [ ] Implement transactional schema v1 (`profiles`, `ecu_variants`, `identity_observations`, `selection_snapshots`) through rusqlite with expected-revision checks. Reject newer/corrupt stores without replacement. Keep user-created definitions as snapshots and OEM/PID data as references.
- [ ] Test reopen/round trip, transactional rollback, optimistic revision conflicts, malformed selections, bounded data and failed observation refresh preserving the prior profile. Run `cargo test -p lognook-core --test vehicle_profiles`.

## Task 2: Standby connection and serialized selection replacement

**Files:** Modify `crates/lognook-core/src/acquisition.rs`, `src/engine.rs`, `src/vehicle_diagnostics/model.rs`; extend acquisition tests and create `tests/profile_resume.rs`.

**Interfaces:** `DiagnosticRequest::identity_queries() -> VecDeque<DiagnosticQueryRequest>` only VIN/calibration/ECU-name reads using existing request limits. `AcquisitionHandle::replace_signals(signals: Vec<SignalDefinition>)` queues an acknowledged command; empty signals mean standby. `LogNookEngine::connect_for_profile(adapter)` returns fresh capabilities while retaining an idle owner. `LogNookEngine::apply_profile_selection(plan, expected_obd_generation)` atomically commits a validated selection with unchanged session origin and supported derived pipeline.

- [ ] Test standby emits no periodic PID reads, still services queued identity queries, and waits without busy looping. Ordinary non-profile connect behavior remains covered.
- [ ] Add connection-generation checks and identity-only queued reads, no whole diagnostic sweep. Respect original adapter waits and a 10-second recognition budget; cancellation occurs between exchanges and an in-flight read settles through existing bounded timeout.
- [ ] Add all-or-nothing signal replacement at a worker boundary; validate support/dependencies before enqueue, acknowledge before publishing new selection. Preserve old selection/pipeline on failure. An empty eligible set does not resume polling.
- [ ] Test one owner/no overlapped adapter calls, session timestamp continuity, stale generation, failed apply rollback, no recording/active job mutation and unsupported identity adapter fallback. Run `cargo test -p lognook-core --test profile_resume --test vehicle_profiles` plus acquisition library tests.

## Task 3: Shared desktop profile/recognition service

**Files:** Create `apps/desktop/src-tauri/src/vehicle_profiles.rs`, `src/commands/vehicle_profiles.rs`; wire AppState and commands in `src/lib.rs`/`src/commands.rs`; use current catalog, diagnostics and adapter factories.

**Interfaces:** `ProfileResumePreview { preview_id, profile_id, profile_revision, connection_generation, match, eligible_log_refs, eligible_graph_refs, skipped }`; skipped items carry stable typed reasons. Commands: list/get/save/update profile, recognize connection, prepare resume, apply resume, discard resume. One connection-bound preview; changing generation/profile revision invalidates it. `ProfileEligibilityContext` contains fresh supported signals, verified identity, current definition snapshots and adapter/CAN capabilities.

- [ ] Resolve standard support from the current connection; never assign one ECU's bitmap to another module. Recompile derived definitions against available compatible inputs; validate CAN field source/context. Query OEM candidate eligibility through `OemCatalog::oem_poll_plan` and existing exact-profile gates, including source/decoder revision comparisons.
- [ ] Test missing/stale OEM refs, auto_request=0, wrong catalog vehicle, missing derived input, missing CAN source and partial identity failure. Assert skipped refs remain stored and no validation state is promoted.
- [ ] Implement store/engine coordination without holding engine locks across blocking identity reads or file operations. Verify generation and profile revision again at Apply. Publish one authoritative selection update for UI and MCP.
- [ ] Recording profile-context metadata is deferred from v1; profiles do not modify `.lognook` files. If added later, keep old recording reads compatible and make the added context optional.

## Task 4: Resume UI, Agent MCP and acceptance

**Files:** Create `apps/desktop/src/VehicleProfilePanel.svelte`, `src/vehicleProfiles.ts`; integrate `App.svelte`, `VehicleDiagnosticsPanel.svelte`, `types.ts`, `workspace.ts`; add `tests/vehicle-profiles.test.mjs`; update Agent `protocol.rs`, `tools.rs`, `mcp.rs` and status types.

**Interfaces:** UI connection -> fresh support/standby -> recognition -> profile match/preview -> Resume. First profile copies current global selections only after explicit Save. Typed MCP `list_vehicle_profiles`, `get_vehicle_profile`, `save_vehicle_profile`, `recognize_vehicle`, `prepare_profile_resume`, `apply_profile_resume`, `discard_profile_resume` use the same service. Status includes profile/match/standby state; `stop_operation` cancels recognition and invalidates pending apply.

- [ ] Implement compact profile identification and Resume preview with eligible versus skipped reasons. Offer explicit manual association when identity is unavailable. Never silently use global old-vehicle selections in profile recognition mode.
- [ ] Test preview/cancel/apply state, graph-only research, unavailable signals remaining saved, mismatched identity, manual fallback, malformed global preferences and stale async results.
- [ ] Test MCP lock behavior, same eligibility as UI, cancellation, duplicate apply, expected revision/generation rejection, and one selection event after successful apply.
- [ ] Update architecture, Agent Access, contributor-facing workflow, feature backlog and PROJECT_STATUS with implemented status and limitations.
- [ ] Run `cargo test --workspace`, `npm run test:ui`, `npm run build`, changed-file formatting and `git diff --check`. Rebuild Windows debug app and report its exact path. Physically test recognition/reconnect only in a separately authorized session; Ford/Scion results remain independent.
