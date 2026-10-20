# Diagnostic signal presets implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Status:** First implementation slice completed on 2026-10-19 with Luna high assistance and inline continuation.

**Goal:** Add vehicle-aware task presets and user-created generic/model/vehicle
presets from a compact top-bar Presets picker, with shared UI/MCP behavior.

**Architecture:** Reuse vehicle profiles for identity and user storage, the
authoritative PID catalog for definitions, and the acquisition owner for signal
selection. Recipes are versioned channel intents with explicit alternatives;
resolution and preview are shared Rust operations.

**Tech stack:** Existing Rust/rusqlite/serde/schemars, Tauri 2, Svelte/TypeScript.
No new runtime dependency or cloud service is planned.

**Spec:** ../specs/2026-10-19-diagnostic-presets-design.md

## Global constraints

- Preserve the current dirty worktree and other agents' changes.
- Preserve imported OEM auto_request and validation states; suggestions do not
  authorize or verify manufacturer-specific definitions.
- Reuse profiles.sqlite for custom preset metadata and existing PID SQLite for
  definitions; do not duplicate the signal catalog.
- Preserve raw measurements, definition references and recorded provenance.
- Browsing, model selection, saving and preview never transmit.
- Apply uses the existing serialized acquisition owner and current generation.
- Apply is unavailable during recording/playback or conflicting operations.
- No commit, push, hardware test, driver or firmware change is part of planning.
  At execution, stage only the files belonging to the requested implementation.

## Review focus

1. A manual model choice must not be accepted as verified ECU compatibility.
2. A CVT/roughness preset without defining channels must not claim readiness;
   optional knock data must keep its actual correction/count/multiplier meaning.
3. Reconnect, identity/revision changes or Agent lock must not apply stale selections.
4. A failed selection switch must leave the former acquisition schedule usable.
5. Broad vehicle applicability and long names must fit the dialog at 760px window
   width without hiding actions, recreating the previous catalog sidebar issue.

## Files and responsibilities

- New `crates/lognook-core/src/signal_presets.rs`: recipe/context/preview models,
  built-in recipe loading and public preset types.
- New `crates/lognook-core/src/signal_presets/resolve.rs`: explicit reference and
  concept-slot matching, eligibility, availability and deterministic ranking.
- Existing `crates/lognook-core/src/lib.rs`: export the shared preset API.
- New `crates/lognook-core/resources/signal-presets.json`: four versioned recipe
  families containing references and metadata, without copied decoder payloads.
- New `crates/lognook-core/src/vehicle_profiles/store_presets.rs`: custom preset
  operations through the existing VehicleProfileStore connection.
- Existing `vehicle_profiles/{store.rs,identity.rs}`, `vehicle_profiles.rs`:
  migration and Transmission owner-detail field; no replacement identity system.
- Existing `acquisition.rs` and `engine.rs`: atomic selection switching and
  acknowledged state publication through the current adapter owner.
- New `apps/desktop/src-tauri/src/commands/signal_presets.rs`: shared operations,
  generation-bound previews, existing profile eligibility reuse and application.
- Existing `apps/desktop/src-tauri/src/{commands.rs,lib.rs}` and
  `agent/{protocol.rs,tools.rs,mcp.rs}`: command registration and UI/MCP exposure
  of the same operations.
- New `apps/desktop/src/{signalPresets.ts,PresetPicker.svelte}`: typed commands and
  a focused dialog; App.svelte owns only the top-bar trigger and selection events.
- Existing `apps/desktop/src/{vehicleProfiles.ts,VehicleProfilePanel.svelte}`:
  transmission context typing and editing.

## Task 1: Recipe model and generic resolution

**Produces:** `SignalPreset`, `PresetVehicleCriteria`, `PresetContext`,
`PresetAvailability`, `PresetResolvedSelection`; public
`resolve_preset(preset, context, capabilities, catalog_evidence)` returning eligible
and skipped channels plus task-defining coverage. Use owned, serializable models
and typed IDs consistent with ProfileSelectionSnapshot/OemDefinitionReference.

- [x] Add fixture tests in `crates/lognook-core/tests/signal_presets.rs` asserting:
  absent model retains generic choices; supported zero-valued signals are valid;
  an unsupported channel is skipped; a CVT recipe with RPM alone is incomplete;
  unverified OEM references are never automatically eligible; adapter/protocol
  mismatches produce reasons; exact compatible scope outranks generic scope;
  each family includes eligible instantaneous knock correction by default;
  absent knock data is not replaced with generic timing or sensor voltage;
  learned corrections, counters and DAM keep separate names/units; missing
  optional knock does not block CVT eligibility.
- [x] Run `cargo test -p lognook-core --test signal_presets`; observe the missing
  preset API failure before implementation.
- [x] Implement the public models/resolver and Core/Extended recipes for Engine
  tuning, CVT, Roughness/misfires and Trims/air–fuel. OEM alternatives require
  exact source references; do not select by a fuzzy name search. Add the shared
  engine-context bundle and target/actual companion channels from the spec,
  deduplicating selected sources and retaining task/slow-context priorities.
- [x] Run the focused tests to PASS and check the four recipes deserialize and
  resolve without a database/adapter connection for generic browsing.

## Task 2: Custom presets and vehicle scope

**Consumes:** Task 1 recipe/context types and existing ProfileSelectionSnapshot.
**Produces:** VehicleProfileStore methods `list_signal_presets`,
`get_signal_preset`, `save_signal_preset`, `delete_signal_preset`; revision-checked
custom rows and explicit Generic/ModelPowertrain/VehicleProfile scopes.

- [x] Add migration/store tests in `tests/signal_presets.rs`: schema v2 data
  survives; generic and per-model records round-trip; a stale revision cannot
  overwrite an edit; built-ins are copied rather than edited; typed transmission
  detail retains owner/Agent attribution; vehicle-specific presets do not match
  a different profile with the same make/model.
- [x] Run the tests and observe failure for the unimplemented store operations.
- [x] Add the transactional schema v3 migration inside the existing profile
  store. Persist recipe revision, scope criteria and selection/reference JSON.
  Keep raw source definitions in the PID catalog. Add Transmission to existing
  Rust/TypeScript owner-detail enums and their validation/UI choices.
- [x] Run preset and existing vehicle-profile migration/identity tests to PASS.

## Task 3: Atomic apply through the acquisition owner

**Consumes:** Resolved selections, current capabilities and OBD generation.
**Produces:** generation-bound `prepare_preset`/`apply_preset` shared operations;
`PresetApplyMode::{Replace,Add}`; `PresetPreview` with preset/context/source
revisions, eligible/skipped channels and an opaque preview ID.

- [ ] Add behavioral tests to `tests/preset_apply.rs`: Add deduplicates; Replace
  swaps exactly the previewed selection; a queued exchange completes before a
  schedule switch; failed validation leaves old polling usable; reconnect and
  revised context/preset invalidate previews; recording/playback rejects Apply;
  browsing/preview produces no adapter writes; no new adapter session is opened.
- [ ] Run `cargo test -p lognook-core --test preset_apply` and observe failure.
- [ ] Extend the existing acquisition command owner with an acknowledged
  selection replacement operation. Reuse standard/OEM eligibility and derived
  dependency logic from Profile Resume. Existing standby replacement must keep
  working; live switching requires no active recording or replay.
- [ ] Add shared Tauri preset commands for list/get/suggest/preview/apply/save/
  delete/status/cancel. Reuse the existing profile connection-generation and
  preview conventions, without pretending presets are vehicle identities.
- [ ] Run preset apply, acquisition, profile-resume and OEM polling tests to PASS.

## Task 4: Compact top-bar picker and preset editor

**Consumes:** The Task 3 shared operations and typed preset results.
**Produces:** top-bar Presets button; Suggested/All presets/My presets tabs;
vehicle-context selector; preview with Replace/Add; Save current selection.

- [ ] Add meaningful UI tests in `apps/desktop/tests/signal-presets.test.mjs`:
  generic browsing needs no vehicle; engine/transmission requirements are shown;
  preview exposes skipped/task-defining channels; stale responses cannot replace
  a newer preview; Save preserves separate logging/graph selections.
- [ ] Run the focused UI tests and observe the missing wrapper/state failure.
- [ ] Implement the dialog in PresetPicker.svelte with a compact vehicle chip,
  scoped suggestions and readable availability counts. Reuse existing vehicle
  metadata/profile choices. Keep scope lists and source detail expandable.
- [ ] Wire App.svelte to open the dialog and consume backend selection events.
  Offline selection stages intent; connection resolves fresh support before
  Apply. Saving and previewing keep the current live selection running.
- [ ] Run `pnpm.cmd run test:ui` and `pnpm.cmd run build`; expect zero failures
  and zero Svelte errors/warnings. Visually check desktop widths 760px and 1320px,
  long model names, many scopes, missing OEM channels and an open recording.

## Task 5: MCP parity, documentation and release checks

**Consumes:** All shared operations; no alternate MCP resolver or transport.
**Produces:** typed signal_presets tool with list/get/suggest/preview/apply/save/
delete/status/cancel, operation ownership and equivalent eligible/skipped results.

- [ ] Add agent schema/behavior tests for preset discovery, generic/model scope,
  locked access, cancellation, reconnect/revision changes and lock before Apply.
  Reuse existing rules for stopping Agent-owned background acquisition at expiry.
- [ ] Run those tests, implement the tool through the shared command service,
  then run them to PASS. Confirm save never promotes an OEM source definition.
- [ ] Update PROJECT_STATUS, architecture, AGENT_ACCESS, VEHICLE_PROFILES and a
  focused `docs/SIGNAL_PRESETS.md` guide. Label built-ins, custom recipes, reviewed
  applicability and physically verified mappings precisely.
- [ ] Run `cargo test --workspace`, the UI suite/build and relevant formatting/
  diff checks. Build the release desktop and matching x86 helper where required;
  report existing blockers without changing toolchains, drivers or firmware.
- [ ] Provide actual template/channel counts, test results, local artifact paths
  and an explicit statement of physical validation performed or still pending.

## Planning-time self-review

- Product scope covers the top-bar entry, vehicle prompt/suggestions, four
  families, Core/Extended sets and generic/model/vehicle custom presets.
- Task 3 explicitly owns the missing live-selection-switch capability: current
  profile resume cannot simply be called on an arbitrary running connection.
- Each review-focus condition has fixture or visual coverage above.
- Initial model-specific recipes remain subject to exact mapping verification;
  the interface must work when only generic recommendations are available.
- Recipe import/export and a broader canonical-concept database are deferred.
- No runtime implementation, build, hardware command, commit or push had been
  made at the time this plan was drafted.

## Delivery status — first implementation slice

- Delivered versioned built-ins, custom preset storage/context, top-bar picker,
  preview/apply through the existing acquisition owner, MCP parity and current
  project documentation. The app supports generic, model/powertrain and exact
  saved-profile scopes; only generic built-ins ship because no exact, physically
  verified model-specific OEM mappings were available.
- `cargo test --workspace --locked` passed. `pnpm.cmd run test:ui` passed 248
  tests; `pnpm.cmd run build` completed with zero Svelte errors/warnings. Release
  desktop app and matching 32-bit J2534 helper builds passed.
- The picker was inspected at 1322px and 762px widths. The simulation-only live
  preview reported 9 eligible and 9 skipped channels; it was not applied.
- Deep Tauri-service concurrency coverage for every listed stale-context,
  cancellation and operation-conflict case remains less extensive than the
  unit coverage for the resolver, store, acquisition owner, UI helpers and MCP
  registration. No physical vehicle or adapter was tested. Per-signal rate
  budgeting remains follow-up work; stored recipe priorities do not alter the
  scheduler.
- No commit or push was made. Existing local work remains in the worktree.
